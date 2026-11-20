// Onglet « Anesthésie » de VetTwins : session en temps réel de la prémédication au réveil.
// Le moteur (WebAssembly) porte toute la physiologie ; ce fichier n'affiche que ses sorties et
// lui transmet les actions. Usage pédagogique uniquement.
import { COLORS, timeChart } from './charts.js';
import { MonitorView, PROBES, ALARM_FR, DEATH_FR } from './monitor-view.js';
import { Room, roomConfig } from './room.js';

const fmt = (v, d = 0) => (v === undefined || v === null || Number.isNaN(v) ? '—' : Number(v).toFixed(d).replace('.', ','));
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const clock = (t) => { const m = Math.floor(t / 60), s = Math.floor(t % 60); return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}:${String(s).padStart(2, '0')}`; };

const PLANE_FR = { awake: 'vigile', sedated: 'sédation', light: 'plan léger', surgical: 'plan chirurgical', deep: 'plan profond', excessive: 'surdosage' };
const CLASS_FR = {
  alpha2_agonist: 'α2-agoniste', phenothiazine: 'phénothiazine', benzodiazepine: 'benzodiazépine', opioid: 'opioïde',
  hypnotic: 'hypnotique', dissociative: 'dissociatif', local_anesthetic: 'anesthésique local', antagonist: 'antagoniste',
};
const MS_FR = [['loc', 'Perte de conscience'], ['intubation', 'Intubation'], ['vaporizer_off', 'Arrêt du vaporisateur'],
  ['swallowing', 'Retour de la déglutition'], ['extubation', 'Extubation'], ['head_lift', 'Relevé de la tête'], ['sternal', 'Décubitus sternal']];

const A = {
  list: null, session: null, key: null, hist: [], snap: null, waves: null, running: false, speed: 10, timer: null, last: null,
  monv: new MonitorView(), sound: false, raf: 0,
  syr: [], pumps: [], nextId: 1,
  machine: { o2: 2, circuit: 'circle', vap: null, pct: 0, vent: false, rr: 10, vt: 12 },
  prep: { agent: null, conc: '', vol: 10, pAgent: null, pConc: '', pVol: 50, pRate: 5 },
  procedure: '', rec: null, lastWave: 0, err: null,
  // salle de simulation (room.js) : relais, capteurs posés, site du stéthoscope, dernier temps opératoire
  room: null, roomSt: { connected: false }, probes: null, steth: null, procIdx: -1, ctl: null, view: null, ctx: null, inputs: null,
  form: { agent: null, dose: '', route: 'iv', push: 30, cri: '', vap: 'isoflurane', pct: 2, fgf: 2, rr: 10, vt: 12, block: null, territory: null, fluids: 5, bolus: 10, bleed: 5 },
};

/** Point d'entrée : `ctx = {ctl, inputs, rerender}`. */
export function renderAnesthesia(v, ctx) {
  const { ctl, inputs } = ctx;
  if (!A.list) {
    try { A.list = ctl.call('message', JSON.stringify({ op: 'anesthesia_list' })); } catch (e) {
      v.innerHTML = `<div class="card"><p>Modules d'anesthésie indisponibles : ${esc(e.message)}</p></div>`; return;
    }
    A.form.agent = A.list.agents[0] && A.list.agents[0].id;
    A.form.vap = A.list.inhalants[0] ? A.list.inhalants[0].id : null;
    A.form.block = (A.list.agents.find((a) => a.routes.local) || {}).id || null;
  }
  const key = JSON.stringify([inputs.bw, inputs.disease, inputs.stage]);
  A.bw = inputs.bw; A.ctl = ctl; A.view = v; A.ctx = ctx; A.inputs = inputs; A.curKey = key;
  roomInit();
  if (A.key !== key) stop();
  v.innerHTML = layout(ctl, inputs, key);
  wire(v, ctl, inputs, key, ctx);
  if (A.snap) paint(v);
}

function stop() {
  if (A.timer) clearInterval(A.timer);
  A.timer = null; A.running = false;
  roomSnapshot();
}

function start(ctl, inputs, key) {
  stop();
  const pid = ctl.call('newPatient', JSON.stringify({ bw: inputs.bw, seed: inputs.seed, variability: false })).patient_id;
  if (inputs.disease) {
    const o = { id: inputs.disease }; if (inputs.stage) o.stage = inputs.stage;
    ctl.call('addDisease', pid, JSON.stringify(o));
  }
  const opts = { seed: inputs.seed || 1 };
  if (A.procedure) opts.procedure = A.procedure;
  const r = ctl.call('message', JSON.stringify({ op: 'anesthesia_start', patient: pid, options: opts }));
  A.session = r.session; A.key = key; A.hist = [r.snapshot]; A.snap = r.snapshot; A.rec = null; A.waves = null; A.lastWave = -1e9; A.monv.reset();
  // nouvelle session : seringues et pousse-seringues préparés conservés (pleins, à l'arrêt)
  for (const x of A.syr) x.left = x.vol;
  for (const x of A.pumps) { x.left = x.vol; x.on = false; }
  A.machine.vent = false; A.procIdx = -1;
  roomSession();
  event(ctl, { type: 'fresh_gas', l_min: A.machine.o2 });
  event(ctl, { type: 'circuit', kind: A.machine.circuit });
  if (A.machine.vap && A.machine.pct > 0) event(ctl, { type: 'vaporizer', agent: A.machine.vap, pct: A.machine.pct });
}

/** Libellé d'une action pour la barre d'état. */
function describe(ev) {
  const d = {
    bolus: () => `${ev.agent} ${String(ev.mg_per_kg).replace('.', ',')} mg/kg ${ev.route.toUpperCase()}${ev.route === 'iv' ? ` (injection ${ev.duration_s ?? 30} s)` : ''}`,
    cri: () => (ev.mg_per_kg_h ? `perfusion de ${ev.agent} ${ev.mg_per_kg_h} mg/kg/h` : `arrêt de la perfusion de ${ev.agent}`),
    vaporizer: () => (ev.pct ? `vaporisateur ${ev.agent} ${ev.pct} %` : `vaporisateur ${ev.agent} fermé`),
    fresh_gas: () => `gaz frais ${ev.l_min} L/min`, intubate: () => 'intubation', extubate: () => 'extubation',
    ventilation: () => (ev.mode === 'controlled' ? `ventilation contrôlée ${ev.rr}/min, ${ev.vt_ml_kg} mL/kg` : 'ventilation spontanée'),
    oxygen: () => (ev.on ? 'oxygène au masque' : "arrêt de l'oxygène"), stimulus: () => 'stimulus test (30 s)',
    procedure_step: () => 'temps opératoire', end_surgery: () => 'fin de chirurgie', local_block: () => `bloc ${ev.agent} (${ev.territory})`,
    fluids: () => `cristalloïde ${ev.ml_kg_h} mL/kg/h`, fluid_bolus: () => `bolus de ${ev.ml_kg} mL/kg`, hemorrhage: () => `hémorragie ${ev.ml_kg} mL/kg`,
    warming: () => (ev.on ? 'réchauffement actif' : 'arrêt du réchauffement'),
  }[ev.type];
  return d ? d() : ev.type;
}

function event(ctl, ev) {
  A.err = null;
  try {
    ctl.call('message', JSON.stringify({ op: 'anesthesia_event', options: { session: A.session, event: ev } }));
    A.last = { t: A.snap ? A.snap.t_s : 0, text: describe(ev), ok: true };
  } catch (e) {
    A.err = e.message.replace(/^.*entrée invalide : /, '');
    A.last = { t: A.snap ? A.snap.t_s : 0, text: `${describe(ev)} : refusé (${A.err})`, ok: false };
  }
}

function play(ctl, v) {
  if (A.running || !A.session || (A.snap && A.snap.dead)) return;
  A.running = true;
  let last = performance.now();
  A.timer = setInterval(() => { const now = performance.now(); advance(ctl, (now - last) / 1000, v); last = now; }, 1000);
}

function advance(ctl, wallS, v) {
  // pas de 1 s jusqu'à ×10, de 5 s au-delà : coût borné à ~12 évaluations par seconde réelle
  const sim = wallS * A.speed, dt = A.speed > 10 ? 5 : 1, n = Math.max(1, Math.round(sim / dt));
  const wantWaves = A.monv.pending === 0;
  try {
    const r = ctl.call('message', JSON.stringify({ op: 'anesthesia_step', options: { session: A.session, dt_s: dt, n, all: true, waves_s: wantWaves ? 6 : 0, seed: 1 } }));
    const t0 = A.snap ? A.snap.t_s : 0;
    for (const s of r.snapshots) A.hist.push(s);
    if (A.hist.length > 20000) A.hist.splice(0, A.hist.length - 20000);
    A.snap = r.snapshots[r.snapshots.length - 1];
    if (r.ecg) {
      A.waves = { ecg: r.ecg, fs: r.ecg_fs, pr: r.pressures, at: performance.now() }; A.lastWave = performance.now(); A.monv.feed(r);
      if (A.room) A.room.send({ kind: 'waves', ecg: r.ecg, ecg_fs: r.ecg_fs, pressures: r.pressures });
    }
    pumpTick(ctl, A.snap.t_s - t0);
    roomSnapshot();
    if (A.snap.dead) stop();
  } catch (e) { A.err = e.message; stop(); }
  paint(v);
}

// ------------------------------------------------------------------ gabarit
function opt(list, sel, label) { return list.map((x) => `<option value="${x.id}"${x.id === sel ? ' selected' : ''}>${esc(label(x))}</option>`).join(''); }

function layout(ctl, inputs, key) {
  const L = A.list, f = A.form;
  const started = A.session && A.key === key;
  const proc = L.procedures.find((p) => p.id === A.procedure);
  const agentLabel = (a) => `${a.name} (${a.classes.map((c) => CLASS_FR[c] || c).join(', ')}) [${a.evidence}]`;
  const ag = L.agents.find((a) => a.id === f.agent) || {};
  const routes = ['iv', ...(ag.routes && ag.routes.im ? ['im'] : []), ...(ag.routes && ag.routes.sc ? ['sc'] : [])];
  if (!routes.includes(f.route)) f.route = 'iv';
  const territories = proc ? proc.territories : ['skin'];
  if (!territories.includes(f.territory)) f.territory = territories[0];
  return `
  <div class="card anesth-head">
    <div class="row">
      <label>Scénario chirurgical <select id="an-proc"><option value="">Aucun</option>${opt(L.procedures, A.procedure, (p) => p.name)}</select></label>
      <button id="an-start" class="${started ? '' : 'primary'}">${started ? 'Recommencer' : 'Démarrer la session'}</button>
      <button id="an-play" ${started && !(A.snap && A.snap.dead) ? '' : 'disabled'}>${A.running ? 'Pause' : 'Lecture'}</button>
      <span class="seg">${[1, 10, 60].map((x) => `<button data-speed="${x}" class="${A.speed === x ? 'on' : ''}">×${x}</button>`).join('')}</span>
      <button id="an-step" ${started && !A.running ? '' : 'disabled'}>+1 min</button>
      <span class="clock" id="an-clock">${started ? clock(A.snap.t_s) : '—'}</span>
    </div>
    ${A.room ? '<p class="room-line" id="an-room"></p>' : ''}
    <p class="sub">Patient de la barre latérale (poids, maladie). Toutes les valeurs du modèle sont pédagogiques ; beaucoup sont des placeholders (niveau D). Ne jamais utiliser pour une anesthésie réelle.</p>
    ${A.err ? `<p class="warn">${esc(A.err)}</p>` : ''}
  </div>
  ${started ? `<div class="anesth-status" id="an-status"></div>` : ''}
  ${started ? `
  <div class="anesth-grid">
    <div class="monitor card" id="an-monitor">
      <div class="mon-top"><div id="an-mon-head"></div><button id="an-sound" class="mon-sound">${A.sound ? '🔊 Son' : '🔇 Son coupé'}</button></div>
      <div class="mon-waves"><canvas id="an-ecg"></canvas><canvas id="an-art"></canvas><canvas id="an-cap"></canvas></div>
      <div class="mon-vals" id="an-mon-vals"></div>
    </div>
    <div class="card">
      <h3>Signes cliniques</h3>
      <div id="an-signs"></div>
    </div>
  </div>
  ${stations(true)}
  <div class="card">
    <h3>Tendances</h3>
    <div class="trend"><span>FC (vert), PAM (rouge)</span><canvas id="an-t1"></canvas></div>
    <div class="trend"><span>SpO₂ (bleu), EtCO₂ (violet)</span><canvas id="an-t2"></canvas></div>
    <div class="trend"><span>Profondeur Δ en équivalents MAC (bleu), nociception nette ×2 (rouge), stimulus ×2 (orange)</span><canvas id="an-t3"></canvas></div>
  </div>
  <div class="anesth-actions">
    <div class="card">
      <h3>Chirurgie et douleur</h3>
      ${proc ? `<div class="steps">${proc.steps.map((s, i) => `<button data-step="${i}" title="intensité ${fmt(s.intensity, 1)}, ${fmt(s.duration_s / 60, 0)} min">${esc(s.label)}</button>`).join('')}
        <button id="an-end" class="danger">Fin de chirurgie</button></div>` : '<p class="sub">Choisir un scénario avant de démarrer pour disposer des temps opératoires.</p>'}
      <div class="row"><button id="an-pinch">Stimulus test (pince, 30 s)</button></div>
      ${A.form.block ? `<div class="row"><label>Bloc <select id="an-block">${opt(L.agents.filter((a) => a.routes.local), f.block, (a) => a.name)}</select></label>
        <label>Territoire <select id="an-terr">${territories.map((t) => `<option ${t === f.territory ? 'selected' : ''}>${t}</option>`).join('')}</select></label>
        <button id="an-block-go">Réaliser</button></div>` : ''}
    </div>
    <div class="card">
      <h3>Fluides, thermique</h3>
      <div class="row"><label>Cristalloïde (mL/kg/h) <input id="an-fl" type="number" min="0" value="${f.fluids}"></label><button id="an-fl-go">Régler</button></div>
      <div class="row"><label>Bolus (mL/kg) <input id="an-bo" type="number" min="0" value="${f.bolus}"></label><button id="an-bo-go">Administrer</button></div>
      <div class="row"><label>Hémorragie (mL/kg) <input id="an-bl" type="number" min="0" value="${f.bleed}"></label><button id="an-bl-go">Simuler</button></div>
      <div class="row"><button id="an-warm">Réchauffement actif</button><button id="an-warm-off">Arrêt</button></div>
    </div>
  </div>
  <div class="card">
    <h3>Feuille d'anesthésie</h3>
    <div class="row"><button id="an-rec">Actualiser</button></div>
    <div id="an-record"></div>
  </div>` : `<p class="sub prep-hint">Préparez seringues, pousse-seringues et machine, puis démarrez la session.</p>${stations(false)}`}`;
}

function num(id) { const x = parseFloat(document.getElementById(id).value); return Number.isFinite(x) ? x : null; }

function wire(v, ctl, inputs, key, ctx) {
  const on = (id, f) => { const el = document.getElementById(id); if (el) el.onclick = f; };
  const rerender = () => renderAnesthesia(v, ctx);
  const ev = (e) => { event(ctl, e); advance(ctl, 0.0001, v); rerender(); };
  const sel = document.getElementById('an-proc'); if (sel) sel.onchange = () => { A.procedure = sel.value; };
  on('an-start', () => {
    try { start(ctl, inputs, key); A.err = null; A.last = { t: 0, text: 'session démarrée', ok: true }; } catch (e) { A.err = e.message; }
    rerender(); play(ctl, v); rerender();
  });
  on('an-play', () => { if (A.running) stop(); else play(ctl, v); rerender(); });
  for (const b of v.querySelectorAll('[data-speed]')) b.onclick = () => { A.speed = +b.dataset.speed; rerender(); };
  on('an-step', () => { const s0 = A.speed; A.speed = 60; advance(ctl, 1, v); A.speed = s0; rerender(); });
  on('an-sound', () => {
    A.sound = A.monv.setSound(!A.sound);
    const b = document.getElementById('an-sound'); if (b) b.textContent = A.sound ? '🔊 Son' : '🔇 Son coupé';
  });
  wireStations(v, ctl, ev, rerender);
  if (!A.session || A.key !== key) return;
  const f = A.form;
  const ag = document.getElementById('an-agent');
  if (ag) ag.onchange = () => { f.agent = ag.value; rerender(); };
  const keep = (id, k) => { const el = document.getElementById(id); if (el) el.onchange = () => { f[k] = el.value; }; };
  keep('an-dose', 'dose'); keep('an-push', 'push'); keep('an-route', 'route'); keep('an-cri', 'cri'); keep('an-vap', 'vap'); keep('an-pct', 'pct'); keep('an-fgf', 'fgf');
  keep('an-rr', 'rr'); keep('an-vt', 'vt'); keep('an-block', 'block'); keep('an-terr', 'territory'); keep('an-fl', 'fluids'); keep('an-bo', 'bolus'); keep('an-bl', 'bleed');
  for (const b of v.querySelectorAll('[data-step]')) b.onclick = () => { A.procIdx = +b.dataset.step; ev({ type: 'procedure_step', index: +b.dataset.step }); };
  on('an-end', () => ev({ type: 'end_surgery' }));
  on('an-pinch', () => ev({ type: 'stimulus', intensity: 1.0, duration_s: 30, territory: f.territory || 'skin' }));
  on('an-block-go', () => ev({ type: 'local_block', agent: document.getElementById('an-block').value, territory: document.getElementById('an-terr').value }));
  on('an-fl-go', () => ev({ type: 'fluids', ml_kg_h: num('an-fl') || 0 }));
  on('an-bo-go', () => ev({ type: 'fluid_bolus', ml_kg: num('an-bo') || 0 }));
  on('an-bl-go', () => ev({ type: 'hemorrhage', ml_kg: num('an-bl') || 0 }));
  on('an-warm', () => ev({ type: 'warming', on: true }));
  on('an-warm-off', () => ev({ type: 'warming', on: false }));
  on('an-rec', () => { try { A.rec = ctl.call('message', JSON.stringify({ op: 'anesthesia_record', options: { session: A.session } })); } catch (e) { A.err = e.message; } paintRecord(); });
  paintRecord();
}

// ------------------------------------------------------------------ dessin
function paint(v) {
  const s = A.snap; if (!s) return;
  const c = document.getElementById('an-clock'); if (c) c.textContent = clock(s.t_s);
  paintStatus();
  paintRoom();
  paintStations();
  const m = document.getElementById('an-mon-vals'); if (!m) return;
  const vi = s.vitals || {};
  const gas = s.inhalants.find((i) => i.vaporizer > 0) || s.inhalants.reduce((a, b) => (b.fe > (a ? a.fe : -1) ? b : a), null);
  const big = (lab, val, unit, cls) => `<div class="mv ${cls}"><span>${lab}</span><b>${val}</b><i>${unit}</i></div>`;
  const head = document.getElementById('an-mon-head');
  if (head) head.innerHTML = `${s.dead ? `<div class="mon-dead">Décès : ${esc(DEATH_FR[s.dead] || s.dead)}</div>` : ''}
    <div class="mon-alarms">${s.alarms.map((a) => `<span>${esc(ALARM_FR[a] || a)}</span>`).join('')}</div>`;
  m.innerHTML = `
      ${big('FC', fmt(vi.hr), '/min', 'g')}
      ${big('PA', `${fmt(vi.sap)}/${fmt(vi.dap)}`, `(${fmt(vi.map)}) mmHg`, 'r')}
      ${big('SpO₂', fmt(vi.spo2), '%', 'b')}
      ${big('EtCO₂', fmt(s.etco2), 'mmHg', 'y')}
      ${big('FR', fmt(s.rr), s.controlled ? '/min (VC)' : '/min', 'y')}
      ${big('T°', fmt(s.temp_c, 1), '°C', 'w')}
      ${gas ? big(`Fi/Fe ${esc(gas.id)}`, `${fmt(gas.fi, 1)}/${fmt(gas.fe, 1)}`, '%', 'w') : ''}
      ${big('Δ', fmt(s.depth.delta, 2), 'éq. MAC', 'w')}`;
  const signs = document.getElementById('an-signs');
  if (signs) {
    const r = s.reflexes, d = s.depth, n = s.nociception;
    const yes = (b) => (b ? 'présent' : 'absent');
    signs.innerHTML = `<table class="vals"><tbody>
      <tr><td>Plan</td><td><b>${PLANE_FR[d.plane]}</b></td></tr>
      <tr><td>Conscience</td><td>${fmt(100 * d.awareness)} %</td></tr>
      <tr><td>Réflexe palpébral</td><td>${yes(r.palpebral)}</td></tr>
      <tr><td>Position du globe</td><td>${r.eye_ventromedial ? 'ventromédiale' : 'centrale'}${r.mydriasis ? ', mydriase' : ''}</td></tr>
      <tr><td>Tonus de la mâchoire</td><td>${['absent', 'diminué', 'présent'][r.jaw_tone]}</td></tr>
      <tr><td>Déglutition</td><td>${yes(r.swallowing)}</td></tr>
      <tr><td>Mouvement</td><td>${r.movement ? '<b class="warn">oui</b>' : 'non'}</td></tr>
      <tr><td>Épargne de MAC</td><td>${fmt(100 * d.mac_reduction)} %</td></tr>
      <tr><td>Nociception nette</td><td>${fmt(n.nu, 2)} (stimulus ${fmt(n.stimulus, 2)}, antinociception ${fmt(100 * n.antinociception)} %)</td></tr>
      <tr><td>Sensibilisation centrale</td><td>${fmt(100 * n.sensitization)} %</td></tr>
      <tr><td>Indice de douleur (modèle, 0–10)</td><td>${n.pain_score === null ? 'non évaluable' : fmt(n.pain_score, 1)}</td></tr>
      ${s.agitation ? '<tr><td colspan="2" class="warn">Réveil agité : douleur probable</td></tr>' : ''}
      ${vi.conduction_block ? '<tr><td colspan="2" class="warn">Trouble de conduction (bloc AV)</td></tr>' : ''}
      <tr><td>pH / PaCO₂ / PaO₂</td><td>${fmt(vi.ph, 2)} / ${fmt(vi.paco2)} / ${fmt(vi.pao2)} mmHg</td></tr>
      <tr><td>Débit cardiaque</td><td>${fmt(vi.co, 2)} L/min</td></tr>
      <tr><td>Lactate / glucose</td><td>${fmt(vi.lactate, 1)} / ${fmt(vi.glucose, 1)} mmol/L</td></tr>
      <tr><td>Hématocrite</td><td>${fmt(vi.hct)} %</td></tr>
    </tbody></table>
    <details><summary>Concentrations</summary><table class="vals"><tbody>
      ${s.agents.filter((a) => a.cp > 0 || a.ce > 0).map((a) => `<tr><td>${esc(a.id)}</td><td>Cp ${fmt(a.cp, 4)} / Ce ${fmt(a.ce, 4)} mg/L${a.cri_mg_kg_h ? ` (perfusion ${fmt(a.cri_mg_kg_h, 3)} mg/kg/h)` : ''}</td></tr>`).join('')}
      ${s.inhalants.filter((i) => i.fe > 0.001).map((i) => `<tr><td>${esc(i.id)}</td><td>Fe ${fmt(i.fe, 2)} %, cerveau ${fmt(i.brain_mac, 2)} MAC</td></tr>`).join('')}
    </tbody></table></details>`;
  }
  startMonitor();
  drawTrends();
}

function drawTrends() {
  const H = A.hist; if (!H.length) return;
  const t = H.map((s) => s.t_s / 60);
  const x1 = Math.max(10, t[t.length - 1]), x0 = Math.max(0, x1 - 60);
  const ser = (f, color) => ({ t, y: H.map((s) => { const x = f(s); return x === null || x === undefined ? NaN : x; }), color });
  const c1 = document.getElementById('an-t1'), c2 = document.getElementById('an-t2'), c3 = document.getElementById('an-t3');
  if (!c1) return;
  timeChart(c1, { series: [ser((s) => s.vitals && s.vitals.hr, COLORS.ecg), ser((s) => s.vitals && s.vitals.map, COLORS.ao)], x0, x1, ticks: 'min', yMin: 0, refLines: [60] });
  timeChart(c2, { series: [ser((s) => s.vitals && s.vitals.spo2, COLORS.lv), ser((s) => s.etco2, COLORS.pa)], x0, x1, ticks: 'min', yMin: 0, yMax: 100 });
  timeChart(c3, { series: [ser((s) => s.depth.delta, COLORS.lv), ser((s) => 2 * s.nociception.nu, COLORS.ao), ser((s) => 2 * s.nociception.stimulus, COLORS.la)], x0, x1, ticks: 'min', yMin: 0, refLines: [1] });
}

function paintRecord() {
  const el = document.getElementById('an-record'); if (!el) return;
  const r = A.rec;
  if (!r) { el.innerHTML = '<p class="sub">Relevé automatique toutes les 5 minutes ; appuyer sur « Actualiser ».</p>'; return; }
  const ms = MS_FR.filter(([k]) => r.milestones[k] !== null).map(([k, l]) => `<li>${l} : ${clock(r.milestones[k])}</li>`).join('');
  el.innerHTML = `
    <table class="vals record"><thead><tr><th>Temps</th><th>FC</th><th>PAS/PAD (PAM)</th><th>SpO₂</th><th>EtCO₂</th><th>FR</th><th>T°</th><th>Fe agent</th><th>Δ</th><th>Plan</th></tr></thead><tbody>
    ${r.record.map((x) => `<tr><td>${clock(x.t_s)}</td><td>${fmt(x.hr)}</td><td>${fmt(x.sap)}/${fmt(x.dap)} (${fmt(x.map)})</td><td>${fmt(x.spo2)}</td><td>${fmt(x.etco2)}</td><td>${fmt(x.rr)}</td><td>${fmt(x.temp_c, 1)}</td><td>${fmt(x.fe_agent, 2)}</td><td>${fmt(x.delta, 2)}</td><td>${PLANE_FR[x.plane]}</td></tr>`).join('')}
    </tbody></table>
    <div class="row" style="align-items:flex-start;gap:24px">
      <div><h4>Jalons</h4><ul>${ms || '<li>—</li>'}</ul></div>
      <div><h4>Journal</h4><ul class="log">${r.log.map((x) => `<li>${clock(x.t_s)} ${esc(x.event)}</li>`).join('')}</ul></div>
    </div>`;
}

/** Barre d'état fixe : temps, lecture, constantes clés, plan, dernière action. */
function paintStatus() {
  const el = document.getElementById('an-status'); const s = A.snap; if (!el || !s) return;
  const hdr = document.querySelector('header.case');
  el.style.top = (hdr ? hdr.offsetHeight : 0) + 'px';
  const vi = s.vitals || {};
  const state = s.dead ? '<b class="warn">décès</b>' : A.running ? `▶ lecture ×${A.speed}` : '<b class="warn">⏸ en pause : appuyer sur Lecture</b>';
  const last = A.last ? `<span class="${A.last.ok ? '' : 'warn'}">${clock(A.last.t)} ${esc(A.last.text)}</span>` : '';
  el.innerHTML = `<span class="clock">${clock(s.t_s)}</span> ${state}
    <span>FC <b>${fmt(vi.hr)}</b></span><span>PA <b>${fmt(vi.sap)}/${fmt(vi.dap)} (${fmt(vi.map)})</b></span>
    <span>SpO₂ <b>${fmt(vi.spo2)}</b></span><span>EtCO₂ <b>${fmt(s.etco2)}</b></span>
    <span>plan <b>${PLANE_FR[s.depth.plane]}</b></span>${s.alarms.length ? `<span class="warn">${s.alarms.map((a) => ALARM_FR[a] || a).join(', ')}</span>` : ''}
    <span class="last">${last}</span>`;
}

// ------------------------------------------------------------------ moniteur défilant et son (monitor-view.js)
function startMonitor() {
  if (A.raf) return;
  const frame = (now) => {
    const ecgC = document.getElementById('an-ecg');
    if (!ecgC || !A.session) { A.raf = 0; return; }
    A.raf = requestAnimationFrame(frame);
    A.monv.draw(now, A.snap, { ecg: ecgC, art: document.getElementById('an-art'), cap: document.getElementById('an-cap') });
  };
  A.raf = requestAnimationFrame(frame);
}

// ------------------------------------------------------------------ postes : pharmacie, pousse-seringues, machine
// Présentations commerciales (docs/ANESTHESIE_SOURCES.md, § Présentations) : aide à la
// préparation des seringues, pas des paramètres du modèle. Concentrations telles qu'étiquetées
// (sel pour la dexmédétomidine, la méthadone, l'atipamézole) ; la dose du modèle est celle de
// l'étiquette. Relevées dans les résumés de la base UE et de la BDPM (RCP non ouverts).
const PRES = {
  propofol: [['Vetofol, PropoVet', 10]],
  alfaxalone: [['Alfaxan Multidose', 10]],
  ketamine: [['Imalgène 1000, Kétamine 1000', 100]],
  dexmedetomidine: [['Dexdomitor 0,5 mg/mL', 0.5], ['Dexdomitor 0,1 mg/mL', 0.1]],
  methadone: [['Comfortan, Insistor', 10]],
  fentanyl: [['Fentadon', 0.05]],
  midazolam: [['midazolam 5 mg/mL (humain)', 5], ['midazolam 1 mg/mL (humain)', 1]],
  lidocaine: [['Lurocaïne, Lidor', 20]],
  bupivacaine: [['bupivacaïne 0,5 % (humain)', 5], ['bupivacaïne 0,25 % (humain)', 2.5]],
  atipamezole: [['Antisedan, Atipazole', 5]],
  naloxone: [['naloxone 0,4 mg/mL (humain)', 0.4]],
  acepromazine: [['Calmivet', 5]],
};
const presOf = (id) => PRES[id] || [['concentration libre', 1]];
const CONC = Object.fromEntries(Object.entries(PRES).map(([k, v]) => [k, v[0][1]]));
const productOf = (id, c) => { const m = presOf(id).find(([, x]) => x === c); return m ? m[0] : 'concentration libre'; };
const n2 = (x, d = 2) => fmt(x, Math.abs(x) >= 10 ? 1 : d);
const agentName = (id) => { const a = A.list.agents.find((x) => x.id === id); return a ? a.name : id; };
const routesOf = (id) => { const a = A.list.agents.find((x) => x.id === id); return a ? ['iv', ...(a.routes.im ? ['im'] : []), ...(a.routes.sc ? ['sc'] : [])] : ['iv']; };

/** Débit perfusé par agent (mg/kg/h), somme des pousse-seringues en marche. */
function criOf(agent) {
  return A.pumps.filter((p) => p.on && p.agent === agent && p.left > 0).reduce((s, p) => s + (p.rate * p.conc) / A.bw, 0);
}
function syncCri(ctl, agent) {
  if (A.session) event(ctl, { type: 'cri', agent, mg_per_kg_h: criOf(agent) });
}
/** Volumes perfusés pendant `dt` s simulées ; arrêt automatique d'une seringue vide. */
function pumpTick(ctl, dt) {
  if (!(dt > 0)) return;
  for (const p of A.pumps) {
    if (!p.on) continue;
    p.left = Math.max(0, p.left - (p.rate * dt) / 3600);
    if (p.left <= 0) {
      p.on = false; syncCri(ctl, p.agent);
      A.last = { t: A.snap.t_s, text: `pousse-seringue ${agentName(p.agent)} vide : perfusion arrêtée`, ok: false };
    }
  }
}

function stations(started) {
  const L = A.list, P = A.prep, M = A.machine;
  if (!P.agent) P.agent = 'propofol'; if (!P.pAgent) P.pAgent = 'fentanyl';
  if (!M.vap && L.inhalants.length) M.vap = L.inhalants[0].id;
  const c0 = P.conc === '' ? CONC[P.agent] ?? 1 : +P.conc, pc0 = P.pConc === '' ? CONC[P.pAgent] ?? 1 : +P.pConc;
  const dis = started ? '' : 'disabled';
  const syr = A.syr.map((x) => {
    const r = routesOf(x.agent), frac = x.vol ? x.left / x.vol : 0;
    return `<div class="syr ${x.left <= 0 ? 'empty' : ''}">
      <div class="syr-head"><b>${esc(agentName(x.agent))}</b> ${n2(x.conc, 3)} mg/mL <span class="sub">${esc(productOf(x.agent, x.conc))}</span><button class="mini" data-syr-del="${x.id}" title="jeter">✕</button></div>
      <div class="bar"><i style="width:${(100 * frac).toFixed(0)}%"></i></div>
      <div class="sub">reste ${n2(x.left, 1)} / ${n2(x.vol, 1)} mL (${n2(x.left * x.conc)} mg)</div>
      <div class="row">
        <label>Volume (mL) <input data-syr-vol="${x.id}" type="number" step="0.1" min="0" value="${x.inj ?? ''}"></label>
        <label>Voie <select data-syr-route="${x.id}">${r.map((v) => `<option ${v === (x.route || 'iv') ? 'selected' : ''}>${v}</option>`).join('')}</select></label>
        <label>Durée (s) <input data-syr-dur="${x.id}" type="number" min="0" value="${x.dur ?? 30}"></label>
        <button data-syr-go="${x.id}" ${dis || (x.left <= 0 ? 'disabled' : '')}>Injecter</button>
      </div>
      <div class="sub dose" data-syr-dose="${x.id}">${x.inj ? `= ${n2((x.inj * x.conc) / A.bw, 3)} mg/kg` : ''}</div>
    </div>`;
  }).join('') || '<p class="sub">Aucune seringue préparée.</p>';
  const pumps = A.pumps.map((x) => `<div class="pump ${x.on ? 'on' : ''}">
      <div class="syr-head"><b>${esc(agentName(x.agent))}</b> ${n2(x.conc, 3)} mg/mL<button class="mini" data-pump-del="${x.id}" title="retirer">✕</button></div>
      <div class="pump-lcd"><span data-pump-rate="${x.id}">${n2(x.rate, 1)} mL/h</span><span>${n2((x.rate * x.conc) / A.bw, 3)} mg/kg/h · ${n2((1000 * x.rate * x.conc) / A.bw / 60, 2)} µg/kg/min</span></div>
      <div class="bar"><i id="pump-bar-${x.id}" style="width:${(100 * x.left / x.vol).toFixed(0)}%"></i></div>
      <div class="sub" id="pump-left-${x.id}">reste ${n2(x.left, 1)} / ${n2(x.vol, 1)} mL</div>
      <div class="row"><label>Débit (mL/h) <input data-pump-newrate="${x.id}" type="number" step="0.1" min="0" value="${x.rate}"></label>
        <button data-pump-set="${x.id}">Régler</button>
        <button data-pump-toggle="${x.id}" class="${x.on ? 'on' : ''}" ${dis || (x.left <= 0 ? 'disabled' : '')}>${x.on ? '⏸ Pause' : '▶ Démarrer'}</button></div>
    </div>`).join('') || '<p class="sub">Aucun pousse-seringue préparé.</p>';
  const inh = L.inhalants.find((i) => i.id === M.vap);
  const intub = A.snap && started ? A.snap.intubated : false;
  return `<div class="anesth-stations">
    <div class="card station">
      <h3>💉 Pharmacie : seringues</h3>
      <div class="prep">
        <label>Agent <select id="pr-agent">${opt(L.agents, P.agent, (a) => a.name)}</select></label>
        <label>Présentation <select id="pr-pres">${presOf(P.agent).map(([n, c]) => `<option value="${c}" ${c === c0 ? 'selected' : ''}>${esc(n)} : ${n2(c, 3)} mg/mL</option>`).join('')}${presOf(P.agent).some(([, c]) => c === c0) ? '' : `<option selected>autre</option>`}</select></label>
        <label>mg/mL <input id="pr-conc" type="number" step="any" min="0" value="${c0}"></label>
        <label>Volume (mL) <input id="pr-vol" type="number" step="0.1" min="0" value="${P.vol}"></label>
        <button id="pr-go" class="primary">Préparer</button>
      </div>
      <p class="sub">${n2(c0 * P.vol)} mg, soit ${n2((c0 * P.vol) / A.bw, 3)} mg/kg pour ${fmt(A.bw, 1)} kg.</p>
      <div class="syr-list">${syr}</div>
    </div>
    <div class="card station">
      <h3>⏱ Pousse-seringues (CRI)</h3>
      <div class="prep">
        <label>Agent <select id="pp-agent">${opt(L.agents, P.pAgent, (a) => a.name)}</select></label>
        <label>mg/mL (dilution) <input id="pp-conc" type="number" step="any" min="0" value="${pc0}"></label>
        <label>Volume (mL) <input id="pp-vol" type="number" step="1" min="0" value="${P.pVol}"></label>
        <label>Débit (mL/h) <input id="pp-rate" type="number" step="0.1" min="0" value="${P.pRate}"></label>
        <button id="pp-go" class="primary">Préparer</button>
      </div>
      <p class="sub">${n2((P.pRate * pc0) / A.bw, 3)} mg/kg/h, soit ${n2((1000 * P.pRate * pc0) / A.bw / 60, 2)} µg/kg/min. Diluer : baisser la concentration.</p>
      <div class="syr-list">${pumps}</div>
    </div>
    <div class="card station machine">
      <h3>🫁 Machine d'anesthésie</h3>
      <div class="mach-row"><span>O₂ (gaz frais)</span><button class="mini" data-m="o2:-0.5">−</button><b>${fmt(M.o2, 1)} L/min</b><button class="mini" data-m="o2:0.5">+</button></div>
      <div class="mach-row"><span>Circuit</span><select id="m-circuit"><option value="circle" ${M.circuit === 'circle' ? 'selected' : ''}>filtre (réinhalation)</option><option value="non_rebreathing" ${M.circuit === 'non_rebreathing' ? 'selected' : ''}>sans réinhalation</option></select></div>
      ${inh ? `<div class="mach-row"><span>Vaporisateur</span><select id="m-vap">${opt(L.inhalants, M.vap, (i) => `${i.name} (MAC ${fmt(i.mac_pct, 1)} %)`)}</select></div>
      <div class="mach-row vap"><span></span><button class="mini" data-m="pct:-0.25">−</button><b class="dial">${fmt(M.pct, 2)} %</b><button class="mini" data-m="pct:0.25">+</button><button class="mini" data-m="pct:off">fermer</button><span class="sub">max ${fmt(inh.vaporizer_max_pct, 0)} %</span></div>` : ''}
      <div class="mach-row"><span>Ventilateur</span><label>FR <input id="m-rr" type="number" min="0" value="${M.rr}"></label><label>VT mL/kg <input id="m-vt" type="number" min="0" value="${M.vt}"></label>
        <button id="m-vent" class="${M.vent ? 'on' : ''}" ${started && intub ? '' : 'disabled'}>${M.vent ? 'VC en marche' : 'VC arrêtée'}</button></div>
      <div class="mach-live" id="an-m-live"></div>
      <h3>Patient</h3>
      <div class="row"><button id="m-intub" ${dis}>${intub ? 'Extuber' : 'Intuber et brancher'}</button>
        <button id="m-mask" ${started && !intub ? '' : 'disabled'}>O₂ au masque</button></div>
      <div class="mach-live" id="an-m-extub"></div>
    </div>
  </div>`;
}

function wireStations(v, ctl, ev, rerender) {
  const g = (id) => document.getElementById(id), P = A.prep, M = A.machine;
  const val = (id) => { const x = parseFloat(g(id).value); return Number.isFinite(x) ? x : null; };
  const bind = (id, f, evName = 'onchange') => { const el = g(id); if (el) el[evName] = f; };
  bind('pr-agent', () => { P.agent = g('pr-agent').value; P.conc = ''; rerender(); });
  bind('pr-conc', () => { P.conc = g('pr-conc').value; rerender(); });
  bind('pr-pres', () => { const c = parseFloat(g('pr-pres').value); if (Number.isFinite(c)) { P.conc = String(c); rerender(); } });
  bind('pr-vol', () => { P.vol = val('pr-vol') ?? P.vol; rerender(); });
  bind('pr-go', () => {
    const conc = val('pr-conc'), vol = val('pr-vol');
    if (!(conc > 0 && vol > 0)) return;
    A.syr.push({ id: A.nextId++, agent: P.agent, conc, vol, left: vol, route: routesOf(P.agent).includes('im') && P.agent === 'dexmedetomidine' ? 'im' : 'iv', dur: 30 });
    A.last = { t: A.snap ? A.snap.t_s : 0, text: `seringue préparée : ${agentName(P.agent)} ${n2(conc)} mg/mL, ${n2(vol, 1)} mL`, ok: true };
    rerender();
  }, 'onclick');
  bind('pp-agent', () => { P.pAgent = g('pp-agent').value; P.pConc = ''; rerender(); });
  bind('pp-conc', () => { P.pConc = g('pp-conc').value; rerender(); });
  bind('pp-vol', () => { P.pVol = val('pp-vol') ?? P.pVol; rerender(); });
  bind('pp-rate', () => { P.pRate = val('pp-rate') ?? P.pRate; rerender(); });
  bind('pp-go', () => {
    const conc = val('pp-conc'), vol = val('pp-vol'), rate = val('pp-rate');
    if (!(conc > 0 && vol > 0 && rate >= 0)) return;
    A.pumps.push({ id: A.nextId++, agent: P.pAgent, conc, vol, left: vol, rate, on: false });
    A.last = { t: A.snap ? A.snap.t_s : 0, text: `pousse-seringue préparé : ${agentName(P.pAgent)} ${n2(conc, 3)} mg/mL, ${n2(rate, 1)} mL/h`, ok: true };
    rerender();
  }, 'onclick');
  const find = (arr, id) => arr.find((x) => x.id === +id);
  for (const el of v.querySelectorAll('[data-syr-vol]')) el.oninput = () => {
    const x = find(A.syr, el.dataset.syrVol); x.inj = parseFloat(el.value) || null;
    const d = v.querySelector(`[data-syr-dose="${x.id}"]`); if (d) d.textContent = x.inj ? `= ${n2((x.inj * x.conc) / A.bw, 3)} mg/kg` : '';
  };
  for (const el of v.querySelectorAll('[data-syr-route]')) el.onchange = () => { find(A.syr, el.dataset.syrRoute).route = el.value; };
  for (const el of v.querySelectorAll('[data-syr-dur]')) el.onchange = () => { find(A.syr, el.dataset.syrDur).dur = parseFloat(el.value); };
  for (const el of v.querySelectorAll('[data-syr-go]')) el.onclick = () => {
    const x = find(A.syr, el.dataset.syrGo), ml = Math.min(x.inj || 0, x.left);
    if (!(ml > 0)) { A.last = { t: A.snap.t_s, text: 'indiquer le volume à injecter (mL)', ok: false }; paintStatus(); return; }
    const route = x.route || 'iv', e = { type: 'bolus', agent: x.agent, mg_per_kg: (ml * x.conc) / A.bw, route };
    if (route === 'iv' && Number.isFinite(x.dur)) e.duration_s = x.dur;
    event(ctl, e);
    if (A.last && A.last.ok) { x.left -= ml; A.last.text += ` (${n2(ml, 2)} mL de la seringue)`; }
    advance(ctl, 0.0001, v); rerender();
  };
  for (const el of v.querySelectorAll('[data-syr-del]')) el.onclick = () => { A.syr = A.syr.filter((x) => x.id !== +el.dataset.syrDel); rerender(); };
  for (const el of v.querySelectorAll('[data-pump-set]')) el.onclick = () => {
    const x = find(A.pumps, el.dataset.pumpSet), r = parseFloat(v.querySelector(`[data-pump-newrate="${x.id}"]`).value);
    if (!(r >= 0)) return; x.rate = r;
    if (x.on) syncCri(ctl, x.agent);
    A.last = { t: A.snap ? A.snap.t_s : 0, text: `${agentName(x.agent)} : débit ${n2(r, 1)} mL/h`, ok: true }; rerender();
  };
  for (const el of v.querySelectorAll('[data-pump-toggle]')) el.onclick = () => {
    const x = find(A.pumps, el.dataset.pumpToggle); x.on = !x.on; syncCri(ctl, x.agent);
    if (A.last && A.last.ok) A.last.text = `pousse-seringue ${agentName(x.agent)} ${x.on ? `démarré (${n2(x.rate, 1)} mL/h)` : 'en pause'}`;
    advance(ctl, 0.0001, v); rerender();
  };
  for (const el of v.querySelectorAll('[data-pump-del]')) el.onclick = () => {
    const x = find(A.pumps, el.dataset.pumpDel); A.pumps = A.pumps.filter((y) => y !== x); if (x.on) syncCri(ctl, x.agent); rerender();
  };
  const live = (e) => { if (A.session) ev(e); else rerender(); };
  for (const el of v.querySelectorAll('[data-m]')) el.onclick = () => {
    const [k, d] = el.dataset.m.split(':');
    if (k === 'o2') { M.o2 = Math.max(0, Math.round((M.o2 + +d) * 10) / 10); live({ type: 'fresh_gas', l_min: M.o2 }); }
    if (k === 'pct') {
      const inh = A.list.inhalants.find((i) => i.id === M.vap);
      M.pct = d === 'off' ? 0 : Math.min(inh ? inh.vaporizer_max_pct : 8, Math.max(0, M.pct + +d));
      live({ type: 'vaporizer', agent: M.vap, pct: M.pct });
    }
  };
  bind('m-circuit', () => { M.circuit = g('m-circuit').value; live({ type: 'circuit', kind: M.circuit }); });
  bind('m-vap', () => {
    const old = M.vap; M.vap = g('m-vap').value;
    if (A.session && M.pct > 0) event(ctl, { type: 'vaporizer', agent: old, pct: 0 });
    M.pct = 0; rerender();
  });
  bind('m-rr', () => { M.rr = val('m-rr') ?? M.rr; if (M.vent) live({ type: 'ventilation', mode: 'controlled', rr: M.rr, vt_ml_kg: M.vt }); });
  bind('m-vt', () => { M.vt = val('m-vt') ?? M.vt; if (M.vent) live({ type: 'ventilation', mode: 'controlled', rr: M.rr, vt_ml_kg: M.vt }); });
  bind('m-vent', () => {
    M.vent = !M.vent;
    live(M.vent ? { type: 'ventilation', mode: 'controlled', rr: M.rr, vt_ml_kg: M.vt } : { type: 'ventilation', mode: 'spontaneous' });
    if (A.last && !A.last.ok) M.vent = false;
  }, 'onclick');
  bind('m-intub', () => {
    if (!A.session) return;
    if (A.snap.intubated) {
      event(ctl, { type: 'extubate' }); M.vent = false;
      if (M.pct > 0 && A.last) A.last.text += ' (vaporisateur encore ouvert : sans effet sur le patient extubé)';
    } else event(ctl, { type: 'intubate' });
    advance(ctl, 0.0001, v); rerender();
  }, 'onclick');
  bind('m-mask', () => ev({ type: 'oxygen', on: A.snap.fio2 < 0.3 }), 'onclick');
}

/** Mise à jour à chaque pas : volumes des pompes, lecture de la machine, critères d'extubation. */
function paintStations() {
  const s = A.snap; if (!s) return;
  for (const x of A.pumps) {
    const b = document.getElementById(`pump-bar-${x.id}`), l = document.getElementById(`pump-left-${x.id}`);
    if (b) b.style.width = `${(100 * x.left / x.vol).toFixed(0)}%`;
    if (l) l.textContent = `reste ${n2(x.left, 1)} / ${n2(x.vol, 1)} mL (perfusé ${n2(x.vol - x.left, 1)} mL)`;
  }
  const g = s.inhalants.find((i) => i.id === A.machine.vap);
  const live = document.getElementById('an-m-live');
  if (live) live.innerHTML = `FiO₂ ${fmt(100 * s.fio2)} % · ${g ? `Fi/Fe ${esc(g.id)} ${fmt(g.fi, 2)}/${fmt(g.fe, 2)} % (${fmt(g.brain_mac, 2)} MAC cérébrale)` : ''}
    · ${s.controlled ? `ventilation contrôlée ${fmt(s.rr)}/min` : `spontanée ${fmt(s.rr)}/min`}${s.apnea ? ' · <b class="warn">apnée</b>' : ''}`;
  const ex = document.getElementById('an-m-extub');
  if (ex) {
    const r = s.reflexes, d = s.depth, vapOpen = A.machine.pct > 0;
    const ok = (b) => (b ? '✓' : '✗');
    if (s.intubated) {
      const ready = r.swallowing && !s.controlled && !s.apnea;
      ex.innerHTML = `<b>Intubé, branché au circuit.</b> Critères d'extubation : ${ok(r.swallowing)} déglutition · ${ok(!s.controlled)} ventilation spontanée · ${ok(!s.apnea)} pas d'apnée · ${ok(!vapOpen)} vaporisateur fermé
        <br>${ready ? '<b class="ok">Extubation possible.</b>' : 'Extubation prématurée : risque d’hypoxémie et d’obstruction (non modélisée).'} Conscience ${fmt(100 * d.awareness)} %.`;
    } else {
      ex.innerHTML = `<b>Non intubé</b> (${s.fio2 > 0.3 ? 'O₂ au masque' : 'air ambiant'}). Intubation possible : ${ok(!r.swallowing)} déglutition abolie · tonus de la mâchoire ${['absent', 'diminué', 'présent'][r.jaw_tone]}.`;
    }
  }
}

// ------------------------------------------------------------------ salle de simulation (relais, boîtiers)
// La régie est la seule à exécuter le moteur. Les boîtiers (docs/INTEGRATION_MATERIEL.tex)
// envoient des entrées en unités de la salle (seringue : mg/mL et mL) ; elles sont traduites ici
// en événements du moteur (mg/kg avec le poids du patient), et le verdict du moteur leur revient.
// Les instantanés sont diffusés au moniteur de salle, au lecteur d'auscultation et au pouls.

function roomInit() {
  if (A.room) return;
  const cfg = roomConfig(); if (!cfg) return;
  A.room = new Room(cfg, 'regie', {
    onMessage: (m) => {
      if (m.kind === 'device') {
        let res;
        try { res = deviceInput(String(m.device || '?'), m.input || {}); } catch (e) { res = { ok: false, text: e.message }; }
        A.room.send({ kind: 'result', req: m.req, ok: res.ok, text: res.text });
      }
      if (m.kind === 'replaced') { A.err = 'Une autre régie s’est connectée au relais : cette page ne reçoit plus les boîtiers.'; A.room.close(); refresh(); }
      if (m.kind === 'welcome') { roomSession(); roomProbes(); roomSteth(); roomSnapshot(); }
    },
    onStatus: (st) => { A.roomSt = st; paintRoom(); },
  });
  // battement de cœur : l'état reste frais pour les boîtiers de sortie même en pause
  setInterval(() => { if (!A.running) roomSnapshot(); }, 2000);
}

const patientDesc = () => (A.inputs ? { bw: A.inputs.bw, disease: A.inputs.disease, stage: A.inputs.stage, seed: A.inputs.seed || 1 } : null);
function roomSession() { if (A.room) A.room.send({ kind: 'session', session: A.session, patient: patientDesc(), procedure: A.procedure || null }); }
function roomProbes() { if (A.room) A.room.send({ kind: 'probes', probes: A.probes }); }
function roomSteth() { if (A.room) A.room.send({ kind: 'steth', site: A.steth, patient: patientDesc() }); }
function roomSnapshot() {
  if (!A.room || !A.snap) return;
  const s = A.snap, vi = s.vitals || {};
  A.room.send({
    kind: 'snapshot', t_s: s.t_s, snap: s, probes: A.probes, running: A.running, speed: A.speed, patient: patientDesc(),
    pulse: { hr: vi.hr ?? null, sap: vi.sap ?? null, dap: vi.dap ?? null, map: vi.map ?? null, dead: !!s.dead, running: A.running },
  });
}

/** Réaffiche l'onglet s'il est à l'écran ; sans casser une saisie du formateur en cours. */
function refresh() {
  if (!A.view || !document.getElementById('an-start')) return;
  const f = document.activeElement;
  if (f && A.view.contains(f) && /^(INPUT|SELECT|TEXTAREA)$/.test(f.tagName)) paint(A.view);
  else renderAnesthesia(A.view, A.ctx);
}

const inRange = (x, lo, hi) => (typeof x === 'number' && Number.isFinite(x) && x >= lo && x <= hi ? x : null);

/** Entrée d'un boîtier → événement du moteur ; renvoie {ok, text} (verdict du moteur). */
function deviceInput(dev, inp) {
  const tag = `[${dev}] `;
  const done = (ok, text) => { A.last = { t: A.snap ? A.snap.t_s : 0, text: tag + text, ok }; paintStatus(); paintRoom(); return { ok, text }; };
  // capteurs et stéthoscope : hors moteur, possibles avant le démarrage
  if (inp.type === 'probes') {
    const p = { ...(A.probes || {}) };
    for (const [k] of PROBES) if (typeof inp[k] === 'boolean') p[k] = inp[k];
    A.probes = p; roomProbes(); roomSnapshot();
    return done(true, 'capteurs : ' + PROBES.map(([k]) => `${k} ${p[k] === false ? '✗' : '✓'}`).join(' '));
  }
  if (inp.type === 'stethoscope') {
    A.steth = inp.site === 'base' || inp.site === 'apex' ? inp.site : null; roomSteth();
    return done(true, A.steth ? `stéthoscope posé (${A.steth})` : 'stéthoscope retiré');
  }
  if (!A.session || A.key !== A.curKey) return done(false, 'session non démarrée');
  if (A.snap && A.snap.dead) return done(false, 'patient décédé');
  const M = A.machine, L = A.list;
  let e = null, after = () => {};
  switch (inp.type) {
    case 'inject': {
      const ag = L.agents.find((a) => a.id === inp.agent);
      if (!ag) return done(false, `agent inconnu « ${inp.agent} »`);
      const conc = inRange(inp.conc_mg_ml, 1e-6, 1000), ml = inRange(inp.ml, 1e-3, 100);
      if (conc === null || ml === null) return done(false, 'conc_mg_ml et ml attendus (nombres > 0)');
      const route = inp.route || 'iv';
      if (!routesOf(ag.id).includes(route)) return done(false, `voie ${route} non disponible pour ${ag.name}`);
      e = { type: 'bolus', agent: ag.id, mg_per_kg: (ml * conc) / A.bw, route };
      if (route === 'iv') e.duration_s = inRange(inp.duration_s, 0, 3600) ?? 30;
      after = () => { A.last.text += ` (${n2(ml, 2)} mL à ${n2(conc, 3)} mg/mL)`; };
      break;
    }
    case 'vaporizer': {
      const agent = inp.agent || M.vap, inh = L.inhalants.find((i) => i.id === agent);
      if (!inh) return done(false, `halogéné inconnu « ${agent} »`);
      const pct = inRange(inp.pct, 0, 100);
      if (pct === null) return done(false, 'pct attendu (nombre ≥ 0)');
      const p = Math.min(inh.vaporizer_max_pct, pct);
      if (agent !== M.vap && M.pct > 0) event(A.ctl, { type: 'vaporizer', agent: M.vap, pct: 0 });
      M.vap = agent; M.pct = p;
      e = { type: 'vaporizer', agent, pct: p };
      if (p < pct) after = () => { A.last.text += ` (butée du vaporisateur : ${fmt(inh.vaporizer_max_pct, 0)} %)`; };
      break;
    }
    case 'fresh_gas': {
      const l = inRange(inp.l_min, 0, 20); if (l === null) return done(false, 'l_min attendu (0 à 20)');
      M.o2 = l; e = { type: 'fresh_gas', l_min: l }; break;
    }
    case 'oxygen': e = { type: 'oxygen', on: inp.on !== false }; break;
    case 'procedure_next': {
      const proc = L.procedures.find((p) => p.id === A.procedure);
      if (!proc) return done(false, 'aucun scénario chirurgical choisi');
      const i = A.procIdx + 1;
      if (i >= proc.steps.length) return done(false, 'tous les temps opératoires sont réalisés');
      e = { type: 'procedure_step', index: i };
      after = () => { A.procIdx = i; A.last.text += ` : ${proc.steps[i].label}`; };
      break;
    }
    case 'end_surgery': case 'intubate': e = { type: inp.type }; break;
    case 'extubate': e = { type: 'extubate' }; after = () => { M.vent = false; }; break;
    case 'warming': e = { type: 'warming', on: inp.on !== false }; break;
    case 'fluids': { const x = inRange(inp.ml_kg_h, 0, 100); if (x === null) return done(false, 'ml_kg_h attendu'); e = { type: 'fluids', ml_kg_h: x }; break; }
    case 'fluid_bolus': { const x = inRange(inp.ml_kg, 0, 100); if (x === null) return done(false, 'ml_kg attendu'); e = { type: 'fluid_bolus', ml_kg: x }; break; }
    case 'hemorrhage': { const x = inRange(inp.ml_kg, 0, 100); if (x === null) return done(false, 'ml_kg attendu'); e = { type: 'hemorrhage', ml_kg: x }; break; }
    case 'stimulus':
      e = { type: 'stimulus', intensity: inRange(inp.intensity, 0, 10) ?? 1.0, duration_s: inRange(inp.duration_s, 1, 3600) ?? 30, territory: inp.territory || A.form.territory || 'skin' };
      break;
    case 'circuit': e = { type: 'circuit', kind: inp.kind }; after = () => { M.circuit = inp.kind; }; break;
    case 'ventilation':
      if (inp.mode === 'controlled') {
        M.rr = inRange(inp.rr, 1, 60) ?? M.rr; M.vt = inRange(inp.vt_ml_kg, 1, 30) ?? M.vt;
        e = { type: 'ventilation', mode: 'controlled', rr: M.rr, vt_ml_kg: M.vt }; after = () => { M.vent = true; };
      } else { e = { type: 'ventilation', mode: 'spontaneous' }; after = () => { M.vent = false; }; }
      break;
    default: return done(false, `entrée inconnue « ${inp.type} »`);
  }
  event(A.ctl, e);
  const ok = !!(A.last && A.last.ok);
  if (ok) after();
  A.last.text = tag + A.last.text;
  advance(A.ctl, 0.0001, A.view);
  refresh();
  return { ok, text: A.last.text.slice(tag.length) };
}

/** Ligne d'état de la salle (régie) : relais, capteurs posés, stéthoscope, lien du moniteur. */
function paintRoom() {
  const el = document.getElementById('an-room'); if (!el || !A.room) return;
  const st = A.roomSt;
  const p = A.probes;
  const probes = PROBES.map(([k, l]) => `<span class="${p && p[k] === false ? 'off' : 'on'}" title="${esc(l)}">${k}</span>`).join('');
  const q = location.search;
  el.innerHTML = `<b>Salle</b> : ${st.connected ? '<span class="ok">relais connecté</span>' : '<span class="warn">relais déconnecté (nouvel essai…)</span>'}
    · capteurs ${p ? '' : '(pas de boîtier : tous affichés) '}<span class="probes">${probes}</span>
    · stéthoscope ${A.steth ? `<b>${A.steth}</b>` : 'non posé'}
    · <a href="monitor.html${q}" target="_blank">moniteur de salle</a> · <a href="ausc.html${q}" target="_blank">lecteur d'auscultation</a>`;
}
