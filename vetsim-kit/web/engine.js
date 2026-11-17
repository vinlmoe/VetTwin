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

  /** Exposition aux médicaments : schéma examiné à t, ou concentrations fixes. */
  static exposure(inp) {
    if (inp.plan && inp.plan.regimens.length && inp.atH !== null && inp.atH !== undefined) {
      return { doses: planDoses(inp.plan), at_h: inp.atH };
    }
    return { conc: inp.conc || {} };
  }

  evaluate(inp, ecgSeconds = 8) {
    const { id, kase } = this.patient(inp);
    const st = this.call('state', id, JSON.stringify({ ...Controller.exposure(inp), signals: true, duration_s: ecgSeconds, seed: inp.seed }));
    // paramètres comparés au chien sain de même poids (schéma du modèle)
    const hp = this.call('newPatient', JSON.stringify({ bw: inp.bw, variability: false })).patient_id;
    const healthy = this.call('state', hp, '{}').params;
    const drugIds = inp.plan && inp.atH !== null && inp.atH !== undefined ? new Set(inp.plan.regimens.map((r) => r.drug)) : new Set(Object.keys(inp.conc || {}));
    const disease = this.catalog.diseases.find((d) => d.id === inp.disease);
    const drugs = this.catalog.drugs.filter((d) => drugIds.has(d.id));
    const changes = Object.keys(st.params).filter((k) => {
      const a = healthy[k], b = st.params[k];
      return a !== b && Math.abs(b - a) > 1e-12 * Math.max(Math.abs(a), Math.abs(b));
    }).map((k) => ({
      param: k, healthy: healthy[k], patient: st.params[k],
      blocks: this.catalog.blocks.filter((b) => b.params.includes(k)).map((b) => b.id),
      sources: [...(disease && disease.effects.some((e) => e.param === k) ? [disease.name] : []),
        ...drugs.filter((d) => d.effects.some((e) => e.param === k)).map((d) => d.name)],
    }));
    return { st, kase, changes, disease: inp.disease, drugIds: [...drugIds] };
  }

  auscultate(inp, hr, site, durationS = 8) {
    const { id } = this.patient(inp);
    const o = { ...Controller.exposure(inp), site, seed: inp.seed, duration_s: durationS, with_ecg: true, with_pressures: true };
    if (hr) o.hr = hr;
    const r = this.call('pcg', id, JSON.stringify(o));
    const bytes = Uint8Array.from(atob(r.wav_base64), (c) => c.charCodeAt(0));
    const view = new DataView(bytes.buffer);
    const n = (bytes.length - 44) / 2;
    const samples = new Float32Array(n);
    for (let i = 0; i < n; i++) samples[i] = view.getInt16(44 + 2 * i, true) / 32767;
    return { ...r, samples, wav: bytes };
  }

  treatment(inp, plan) {
    const { id } = this.patient(inp);
    const h = plan.horizonH, step = h <= 48 ? 0.1 : h <= 168 ? 0.25 : 0.5;
    const grid = Array.from({ length: Math.floor(h / step) + 1 }, (_, i) => +(i * step).toFixed(4));
    const sStep = Math.max(1, Math.ceil(h / 96));
    const sgrid = Array.from({ length: Math.floor(h / sStep) + 1 }, (_, i) => i * sStep);
    const r = this.call('timecourse', id, JSON.stringify({ doses: planDoses(plan), t_end_h: h, t_eval_h: grid, state_times_h: sgrid }));
    const states = {};
    for (const k of Object.keys(r.states_at[0] || {})) states[k] = r.states_at.map((s) => s[k]);
    return { ...r, states, plan };
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
