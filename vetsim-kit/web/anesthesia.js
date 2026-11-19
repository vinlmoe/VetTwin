// Onglet « Anesthésie » de VetTwins : session en temps réel de la prémédication au réveil.
// Le moteur (WebAssembly) porte toute la physiologie ; ce fichier n'affiche que ses sorties et
// lui transmet les actions. Usage pédagogique uniquement.
import { COLORS, timeChart, setup } from './charts.js';

const fmt = (v, d = 0) => (v === undefined || v === null || Number.isNaN(v) ? '—' : Number(v).toFixed(d).replace('.', ','));
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const clock = (t) => { const m = Math.floor(t / 60), s = Math.floor(t % 60); return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}:${String(s).padStart(2, '0')}`; };

const PLANE_FR = { awake: 'vigile', sedated: 'sédation', light: 'plan léger', surgical: 'plan chirurgical', deep: 'plan profond', excessive: 'surdosage' };
const ALARM_FR = {
  hypotension: 'Hypotension (PAM < 60)', hypertension: 'Hypertension', bradycardia: 'Bradycardie', tachycardia: 'Tachycardie',
  desaturation: 'Désaturation (SpO₂ < 90 %)', apnea: 'Apnée', hypercapnia: 'Hypercapnie (PaCO₂ > 60)', hypothermia: 'Hypothermie (< 36 °C)',
  movement: 'Mouvement', cardiac_arrest: 'Arrêt cardiorespiratoire',
};
const DEATH_FR = {
  cardiac_arrest: 'arrêt cardiaque', circulatory_collapse: 'effondrement circulatoire', irreversible_shock: 'choc irréversible',
  oxygen_debt: "dette d'oxygène", no_steady_state: 'aucun état viable', vicious_circle: 'spirale de décompensation',
};
const CLASS_FR = {
  alpha2_agonist: 'α2-agoniste', phenothiazine: 'phénothiazine', benzodiazepine: 'benzodiazépine', opioid: 'opioïde',
  hypnotic: 'hypnotique', dissociative: 'dissociatif', local_anesthetic: 'anesthésique local', antagonist: 'antagoniste',
};
const MS_FR = [['loc', 'Perte de conscience'], ['intubation', 'Intubation'], ['vaporizer_off', 'Arrêt du vaporisateur'],
  ['swallowing', 'Retour de la déglutition'], ['extubation', 'Extubation'], ['head_lift', 'Relevé de la tête'], ['sternal', 'Décubitus sternal']];

const A = {
  list: null, session: null, key: null, hist: [], snap: null, waves: null, running: false, speed: 1, timer: null,
  procedure: '', rec: null, lastWave: 0, err: null,
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
  if (A.key !== key) stop();
  v.innerHTML = layout(ctl, inputs, key);
  wire(v, ctl, inputs, key, ctx);
  if (A.snap) paint(v);
}

function stop() {
  if (A.timer) clearInterval(A.timer);
  A.timer = null; A.running = false;
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
  A.session = r.session; A.key = key; A.hist = [r.snapshot]; A.snap = r.snapshot; A.rec = null; A.waves = null; A.lastWave = -1e9;
}

function event(ctl, ev) {
  A.err = null;
  try { ctl.call('message', JSON.stringify({ op: 'anesthesia_event', options: { session: A.session, event: ev } })); } catch (e) { A.err = e.message; }
}

function advance(ctl, wallS, v) {
  // pas de 1 s jusqu'à ×10, de 5 s au-delà : coût borné à ~12 évaluations par seconde réelle
  const sim = wallS * A.speed, dt = A.speed > 10 ? 5 : 1, n = Math.max(1, Math.round(sim / dt));
  const wantWaves = performance.now() - A.lastWave > 3000;
  try {
    const r = ctl.call('message', JSON.stringify({ op: 'anesthesia_step', options: { session: A.session, dt_s: dt, n, all: true, waves_s: wantWaves ? 4 : 0, seed: 1 } }));
    for (const s of r.snapshots) A.hist.push(s);
    if (A.hist.length > 20000) A.hist.splice(0, A.hist.length - 20000);
    A.snap = r.snapshots[r.snapshots.length - 1];
    if (r.ecg) { A.waves = { ecg: r.ecg, fs: r.ecg_fs, pr: r.pressures, at: performance.now() }; A.lastWave = performance.now(); }
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
    <p class="sub">Patient de la barre latérale (poids, maladie). Toutes les valeurs du modèle sont pédagogiques ; beaucoup sont des placeholders (niveau D). Ne jamais utiliser pour une anesthésie réelle.</p>
    ${A.err ? `<p class="warn">${esc(A.err)}</p>` : ''}
  </div>
  ${started ? `
  <div class="anesth-grid">
    <div class="monitor card" id="an-monitor"></div>
    <div class="card">
      <h3>Signes cliniques</h3>
      <div id="an-signs"></div>
    </div>
  </div>
  <div class="card">
    <h3>Tendances</h3>
    <div class="trend"><span>FC (vert), PAM (rouge)</span><canvas id="an-t1"></canvas></div>
    <div class="trend"><span>SpO₂ (bleu), EtCO₂ (violet)</span><canvas id="an-t2"></canvas></div>
    <div class="trend"><span>Profondeur Δ en équivalents MAC (bleu), nociception nette ×2 (rouge), stimulus ×2 (orange)</span><canvas id="an-t3"></canvas></div>
  </div>
  <div class="anesth-actions">
    <div class="card">
      <h3>Médicaments</h3>
      <label>Agent <select id="an-agent">${opt(L.agents, f.agent, agentLabel)}</select></label>
      <div class="row"><label>Dose (mg/kg) <input id="an-dose" type="number" step="any" min="0" value="${f.dose}"></label>
        <label>Voie <select id="an-route">${routes.map((r) => `<option ${r === f.route ? 'selected' : ''}>${r}</option>`).join('')}</select></label>
        <label title="durée de l'injection IV">Injection (s) <input id="an-push" type="number" min="0" value="${f.push}"></label>
        <button id="an-bolus">Injecter</button></div>
      <div class="row"><label>Perfusion (mg/kg/h) <input id="an-cri" type="number" step="any" min="0" value="${f.cri}"></label>
        <button id="an-cri-go">Régler</button></div>
      <p class="sub" id="an-agent-note">${esc(ag.notes || '')}</p>
    </div>
    <div class="card">
      <h3>Voies aériennes, halogéné</h3>
      <div class="row">
        <button id="an-intub">${A.snap.intubated ? 'Extuber' : 'Intuber'}</button>
        <button id="an-o2" ${A.snap.intubated ? 'disabled' : ''}>O₂ au masque</button>
      </div>
      ${L.inhalants.length ? `<div class="row"><label>Agent <select id="an-vap">${opt(L.inhalants, f.vap, (i) => `${i.name} (MAC ${fmt(i.mac_pct, 2)} %) [${i.evidence}]`)}</select></label>
        <label>Vaporisateur (%) <input id="an-pct" type="number" step="0.1" min="0" value="${f.pct}"></label>
        <button id="an-vap-go">Régler</button><button id="an-vap-off">Fermer</button></div>
      <div class="row"><label>Gaz frais (L/min) <input id="an-fgf" type="number" step="0.1" min="0" value="${f.fgf}"></label><button id="an-fgf-go">Régler</button></div>` : ''}
      <div class="row"><label>FR <input id="an-rr" type="number" min="0" value="${f.rr}"></label><label>VT (mL/kg) <input id="an-vt" type="number" min="0" value="${f.vt}"></label>
        <button id="an-ippv" ${A.snap.intubated ? '' : 'disabled'}>Ventilation contrôlée</button><button id="an-spont">Spontanée</button></div>
    </div>
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
  </div>` : ''}`;
}

function num(id) { const x = parseFloat(document.getElementById(id).value); return Number.isFinite(x) ? x : null; }

function wire(v, ctl, inputs, key, ctx) {
  const on = (id, f) => { const el = document.getElementById(id); if (el) el.onclick = f; };
  const rerender = () => renderAnesthesia(v, ctx);
  const ev = (e) => { event(ctl, e); advance(ctl, 0.0001, v); rerender(); };
  const sel = document.getElementById('an-proc'); if (sel) sel.onchange = () => { A.procedure = sel.value; };
  on('an-start', () => { try { start(ctl, inputs, key); A.err = null; } catch (e) { A.err = e.message; } rerender(); });
  on('an-play', () => {
    if (A.running) stop(); else { A.running = true; let last = performance.now(); A.timer = setInterval(() => { const now = performance.now(); advance(ctl, (now - last) / 1000, v); last = now; }, 1000); }
    rerender();
  });
  for (const b of v.querySelectorAll('[data-speed]')) b.onclick = () => { A.speed = +b.dataset.speed; rerender(); };
  on('an-step', () => { const s0 = A.speed; A.speed = 60; advance(ctl, 1, v); A.speed = s0; rerender(); });
  if (!A.session || A.key !== key) return;
  const f = A.form;
  const ag = document.getElementById('an-agent');
  if (ag) ag.onchange = () => { f.agent = ag.value; rerender(); };
  const keep = (id, k) => { const el = document.getElementById(id); if (el) el.onchange = () => { f[k] = el.value; }; };
  keep('an-dose', 'dose'); keep('an-push', 'push'); keep('an-route', 'route'); keep('an-cri', 'cri'); keep('an-vap', 'vap'); keep('an-pct', 'pct'); keep('an-fgf', 'fgf');
  keep('an-rr', 'rr'); keep('an-vt', 'vt'); keep('an-block', 'block'); keep('an-terr', 'territory'); keep('an-fl', 'fluids'); keep('an-bo', 'bolus'); keep('an-bl', 'bleed');
  on('an-bolus', () => { const d = num('an-dose'); if (d !== null) { const p = num('an-push'); ev({ type: 'bolus', agent: ag.value, mg_per_kg: d, route: document.getElementById('an-route').value, ...(p !== null ? { duration_s: p } : {}) }); } });
  on('an-cri-go', () => { const d = num('an-cri'); ev({ type: 'cri', agent: ag.value, mg_per_kg_h: d || 0 }); });
  on('an-intub', () => {
    if (!A.snap.intubated) { event(ctl, { type: 'intubate' }); if (!A.err) event(ctl, { type: 'fresh_gas', l_min: num('an-fgf') || 0 }); advance(ctl, 0.0001, v); rerender(); }
    else ev({ type: 'extubate' });
  });
  on('an-o2', () => ev({ type: 'oxygen', on: A.snap.fio2 < 0.3 }));
  on('an-vap-go', () => { event(ctl, { type: 'fresh_gas', l_min: num('an-fgf') || 0 }); ev({ type: 'vaporizer', agent: document.getElementById('an-vap').value, pct: num('an-pct') || 0 }); });
  on('an-vap-off', () => ev({ type: 'vaporizer', agent: document.getElementById('an-vap').value, pct: 0 }));
  on('an-fgf-go', () => ev({ type: 'fresh_gas', l_min: num('an-fgf') || 0 }));
  on('an-ippv', () => ev({ type: 'ventilation', mode: 'controlled', rr: num('an-rr') || 0, vt_ml_kg: num('an-vt') || 0 }));
  on('an-spont', () => ev({ type: 'ventilation', mode: 'spontaneous' }));
  for (const b of v.querySelectorAll('[data-step]')) b.onclick = () => ev({ type: 'procedure_step', index: +b.dataset.step });
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
  const m = document.getElementById('an-monitor'); if (!m) return;
  const vi = s.vitals || {};
  const gas = s.inhalants.find((i) => i.vaporizer > 0) || s.inhalants.reduce((a, b) => (b.fe > (a ? a.fe : -1) ? b : a), null);
  const big = (lab, val, unit, cls) => `<div class="mv ${cls}"><span>${lab}</span><b>${val}</b><i>${unit}</i></div>`;
  m.innerHTML = `
    ${s.dead ? `<div class="mon-dead">Décès : ${esc(DEATH_FR[s.dead] || s.dead)}</div>` : ''}
    <div class="mon-alarms">${s.alarms.map((a) => `<span>${esc(ALARM_FR[a] || a)}</span>`).join('')}</div>
    <div class="mon-waves"><canvas id="an-ecg"></canvas><canvas id="an-art"></canvas><canvas id="an-cap"></canvas></div>
    <div class="mon-vals">
      ${big('FC', fmt(vi.hr), '/min', 'g')}
      ${big('PA', `${fmt(vi.sap)}/${fmt(vi.dap)}`, `(${fmt(vi.map)}) mmHg`, 'r')}
      ${big('SpO₂', fmt(vi.spo2), '%', 'b')}
      ${big('EtCO₂', fmt(s.etco2), 'mmHg', 'y')}
      ${big('FR', fmt(s.rr), s.controlled ? '/min (VC)' : '/min', 'y')}
      ${big('T°', fmt(s.temp_c, 1), '°C', 'w')}
      ${gas ? big(`Fi/Fe ${esc(gas.id)}`, `${fmt(gas.fi, 1)}/${fmt(gas.fe, 1)}`, '%', 'w') : ''}
      ${big('Δ', fmt(s.depth.delta, 2), 'éq. MAC', 'w')}
    </div>`;
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
  drawWaves(s);
  drawTrends();
}

function drawWaves(s) {
  const W = A.waves;
  const ecg = document.getElementById('an-ecg'), art = document.getElementById('an-art'), cap = document.getElementById('an-cap');
  if (!ecg) return;
  if (W && !s.dead) {
    timeChart(ecg, { series: [{ y: W.ecg, dt: 1 / W.fs, color: '#35e08a' }], x0: 0, x1: W.ecg.length / W.fs, yMin: -1.5, yMax: 2.5, left: 4 });
    if (W.pr) timeChart(art, { series: [{ y: W.pr.ao, dt: 1 / W.pr.fs, color: '#ff6b6b' }], x0: 0, x1: W.pr.ao.length / W.pr.fs, yMin: 0, yMax: 200, left: 30 });
  } else { setup(ecg); setup(art); }
  // capnogramme schématique : plateau à l'EtCO2, au rythme respiratoire
  const { ctx, w, h } = setup(cap);
  ctx.strokeStyle = '#f2d45c'; ctx.lineWidth = 1.6; ctx.beginPath();
  const period = s.rr > 0 ? 60 / s.rr : Infinity, span = 15, Y = (p) => h - 4 - (p / 60) * (h - 8);
  for (let i = 0; i <= w; i++) {
    const t = (i / w) * span, ph = Number.isFinite(period) ? (t % period) / period : 0;
    const p = !Number.isFinite(period) ? 0 : ph < 0.4 ? 0 : ph < 0.45 ? s.etco2 * (ph - 0.4) / 0.05 : ph < 0.9 ? s.etco2 * (0.92 + 0.08 * (ph - 0.45) / 0.45) : s.etco2 * (1 - (ph - 0.9) / 0.1);
    if (i === 0) ctx.moveTo(i, Y(p)); else ctx.lineTo(i, Y(p));
  }
  ctx.stroke();
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
