// Pilote du moteur WebAssembly (équivalent du DemoController Kotlin de kmp-demo).
import init, { Engine } from './pkg/vetsim_wasm.js';

export class EngineError extends Error {}

export class Controller {
  static async create() {
    await init();
    const c = new Controller();
    c.engine = new Engine();
    const bundle = await (await fetch('./modules.json')).text();
    c.call('loadModules', bundle);
    const cat = c.call('listModules');
    c.catalog = { diseases: cat.diseases, drugs: cat.drugs, blocks: cat.blocks };
    c.apiVersion = c.engine.apiVersion();
    return c;
  }

  /**
   * Patient temporaire : créé, passé à `f`, puis libéré dans le moteur (sans libération, chaque
   * calcul laissait un patient de plus en mémoire WebAssembly).
   */
  withPatient(inp, f) {
    const p = this.patient(inp);
    try { return f(p); } finally { this.drop(p.id); }
  }

  /** Libère un patient du moteur (sans effet s'il n'existe plus). */
  drop(id) {
    try { this.engine.message(JSON.stringify({ op: 'drop_patient', patient: id })); } catch (e) { /* moteur antérieur */ }
  }

  /** Paramètres du chien sain de même poids (référence du schéma du modèle), mis en cache. */
  healthyParams(bw) {
    this.healthyCache ??= new Map();
    if (!this.healthyCache.has(bw)) {
      const hp = this.call('newPatient', JSON.stringify({ bw, variability: false })).patient_id;
      try { this.healthyCache.set(bw, this.call('state', hp, '{}').params); } finally { this.drop(hp); }
    }
    return this.healthyCache.get(bw);
  }

  /** Blocs du modèle qui portent chaque paramètre (index construit une fois). */
  blocksOf(param) {
    if (!this.paramBlocks) {
      this.paramBlocks = new Map();
      for (const b of this.catalog.blocks) for (const k of b.params) {
        if (!this.paramBlocks.has(k)) this.paramBlocks.set(k, []);
        this.paramBlocks.get(k).push(b.id);
      }
    }
    return this.paramBlocks.get(param) || [];
  }

  /** Appel au moteur ; lève EngineError si la réponse est {ok: false}. */
  call(method, ...args) {
    const r = JSON.parse(this.engine[method](...args));
    if (!r.ok) throw new EngineError((r.errors || []).map((e) => `${e.path} ${e.message}`.trim()).join(' ; '));
    return r;
  }

  /** Patient sans variabilité (mode déterministe) avec sa maladie éventuelle. */
  patient(inp) {
    const id = this.call('newPatient', JSON.stringify({ bw: inp.bw, seed: inp.seed, variability: false })).patient_id;
    let kase = null;
    if (inp.disease) {
      const o = { id: inp.disease };
      if (inp.stage) o.stage = inp.stage;
      kase = this.call('addDisease', id, JSON.stringify(o)).case;
    }
    return { id, kase };
  }

  /** Exposition aux médicaments : toujours par la PK/PD (schéma examiné à l'instant atH). */
  static exposure(inp) {
    if (inp.plan && inp.plan.regimens.length && inp.atH !== null && inp.atH !== undefined) {
      return { doses: planDoses(inp.plan), at_h: inp.atH };
    }
    return {};
  }

  evaluate(inp, ecgSeconds = 8) {
    const { st, kase } = this.withPatient(inp, ({ id, kase }) => ({
      kase, st: this.call('state', id, JSON.stringify({ ...Controller.exposure(inp), signals: true, duration_s: ecgSeconds, seed: inp.seed })),
    }));
    // Décès absorbant : `state` est instantané ; si la simulation du traitement a daté un décès
    // avant l'instant examiné, le patient reste décédé (pas de variables, pas de signaux).
    if (inp.death && inp.atH !== null && inp.atH !== undefined && inp.atH >= inp.death.t_h) {
      for (const k of Object.keys(st)) if (typeof st[k] === 'number') delete st[k];
      st.outcome = { state: 'dead', cause: inp.death.cause, label_fr: inp.death.label_fr, mechanisms: [], absorbing_from_h: inp.death.t_h };
      st.signals = null; st.emergent_signs = [];
    }
    // paramètres comparés au chien sain de même poids (schéma du modèle)
    const healthy = this.healthyParams(inp.bw);
    const drugIds = inp.plan && inp.atH !== null && inp.atH !== undefined ? new Set(inp.plan.regimens.map((r) => r.drug)) : new Set();
    const disease = this.catalog.diseases.find((d) => d.id === inp.disease);
    const drugs = this.catalog.drugs.filter((d) => drugIds.has(d.id));
    const changes = Object.keys(st.params).filter((k) => {
      const a = healthy[k], b = st.params[k];
      return a !== b && Math.abs(b - a) > 1e-12 * Math.max(Math.abs(a), Math.abs(b));
    }).map((k) => ({
      param: k, healthy: healthy[k], patient: st.params[k],
      blocks: this.blocksOf(k),
      sources: [...(disease && disease.effects.some((e) => e.param === k) ? [disease.name] : []),
        ...drugs.filter((d) => d.effects.some((e) => e.param === k)).map((d) => d.name)],
    }));
    return { st, kase, changes, disease: inp.disease, drugIds: [...drugIds] };
  }

  auscultate(inp, hr, site, durationS = 8) {
    const o = { ...Controller.exposure(inp), site, seed: inp.seed, duration_s: durationS, with_ecg: true, with_pressures: true };
    if (hr) o.hr = hr;
    const r = this.withPatient(inp, ({ id }) => this.call('pcg', id, JSON.stringify(o)));
    const { samples, bytes } = decodeWav(r.wav_base64);
    delete r.wav_base64;
    return { ...r, samples, wav: bytes };
  }

  /** Cœur animé : volumes, pressions, ECG et événements de conduction sur une même fenêtre. */
  heart(inp, durationS = 8) {
    return this.withPatient(inp, ({ id }) => this.call('heart', id, JSON.stringify({ ...Controller.exposure(inp), seed: inp.seed, duration_s: durationS, fs: 200 })));
  }

  treatment(inp, plan) {
    return this.withPatient(inp, ({ id }) => this.treatmentOf(id, plan));
  }

  treatmentOf(id, plan) {
    const h = plan.horizonH, step = h <= 48 ? 0.1 : h <= 168 ? 0.25 : 0.5;
    const grid = Array.from({ length: Math.floor(h / step) + 1 }, (_, i) => +(i * step).toFixed(4));
    const sStep = Math.max(1, Math.ceil(h / 96));
    const sgrid = Array.from({ length: Math.floor(h / sStep) + 1 }, (_, i) => i * sStep);
    const r = this.call('timecourse', id, JSON.stringify({ doses: planDoses(plan), t_end_h: h, t_eval_h: grid, state_times_h: sgrid }));
    // séries d'indicateurs ; instants sans valeur (décès sans passe viable) : NaN
    const keys = new Set();
    for (const s of r.states_at) for (const [k, v] of Object.entries(s)) if (typeof v === 'number') keys.add(k);
    const states = {};
    for (const k of keys) states[k] = r.states_at.map((s) => (typeof s[k] === 'number' ? s[k] : NaN));
    const outcomes = r.states_at.map((s) => ({ state: s.outcome, mechanisms: s.mechanisms || [], cause: s.cause }));
    return { ...r, states, outcomes, plan };
  }
}

/** Prises effectives d'un traitement (dose 0 = prise omise). */
export function planDoses(plan) {
  const out = [];
  for (const r of plan.regimens) {
    for (let i = 0; i < r.count; i++) {
      const mg = r.overrides && r.overrides[i] !== undefined ? r.overrides[i] : r.mgPerKg;
      if (mg > 0) out.push({ t_h: r.startH + i * r.intervalH, drug: r.drug, mg_per_kg: mg, route: r.route });
    }
  }
  return out;
}

/** WAV PCM 16 bits mono (base64) → échantillons dans [−1, 1] et octets du fichier. */
export function decodeWav(b64) {
  const bin = atob(b64), bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  const view = new DataView(bytes.buffer), n = Math.max(0, (bytes.length - 44) >> 1), samples = new Float32Array(n);
  for (let i = 0; i < n; i++) samples[i] = view.getInt16(44 + 2 * i, true) / 32767;
  return { samples, bytes };
}

/** Calcul annulé parce qu'un calcul plus récent l'a remplacé. */
export class Superseded extends Error {}

/**
 * Calculs longs (cinétique sous traitement : plusieurs secondes en MMVD) dans un Web Worker,
 * pour que l'interface reste réactive. Un seul calcul à la fois : une nouvelle demande annule la
 * précédente (le worker est alors arrêté puis recréé, faute de pouvoir interrompre le WebAssembly).
 */
export class BackgroundEngine {
  constructor(url = './engine-worker.js') { this.url = url; this.worker = null; this.job = null; this.seq = 0; }

  run(method, ...args) {
    if (this.job) this.cancel();
    if (!this.worker) this.worker = new Worker(this.url, { type: 'module' });
    const id = ++this.seq, w = this.worker;
    return new Promise((resolve, reject) => {
      this.job = { id, reject };
      w.onmessage = (ev) => {
        const d = ev.data;
        if (!this.job || d.id !== this.job.id) return;
        this.job = null;
        if (d.error) reject(new EngineError(d.error)); else resolve(d.result);
      };
      w.onerror = (ev) => { if (this.job && this.job.id === id) { this.job = null; this.reset(); reject(new EngineError(ev.message || 'échec du calcul en arrière-plan')); } };
      w.postMessage({ id, method, args });
    });
  }

  get busy() { return this.job !== null; }

  /** Abandonne le calcul en cours (sa promesse est rejetée avec Superseded). */
  cancel() {
    if (!this.job) return;
    const { reject } = this.job;
    this.job = null;
    this.reset();
    reject(new Superseded());
  }

  reset() { if (this.worker) { this.worker.terminate(); this.worker = null; } }
}
