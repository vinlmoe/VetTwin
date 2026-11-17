// VetTwins (web) : interface du patient virtuel canin. Le moteur vetsim tourne en
// WebAssembly dans le navigateur (aucun serveur de calcul, aucune donnée envoyée).
// Usage pédagogique uniquement.
import { Controller, planDoses } from './engine.js';
import { COLORS, timeChart, spectrogram, computeSpectrogram, dayLabel } from './charts.js';

const $ = (sel) => document.querySelector(sel);
const fmt = (v, d) => (v === undefined || v === null || Number.isNaN(v) ? '—' : Number(v).toFixed(d).replace('.', ','));
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

const S = {
  ctl: null, tab: 'patient',
  inputs: { bw: 20, disease: null, stage: null, seed: 1, plan: null, atH: null },
  res: null,
  ausc: { hr: null, site: 'base', r: null, spec: null, window: 8, start: 0, cursor: null, playing: false, audio: null, src: null, loop: false, zoomOnly: false, played: false },
  treat: { regimens: [], days: 3, r: null, cursor: 0, shown: ['hr', 'map', 'k'], ref: null, editing: null, open: null },
  model: { selected: null },
};

// ------------------------------------------------------------------ données d'affichage
const HEMO = [['hr', 'FC', '/min', 0], ['sap', 'PAS', 'mmHg', 0], ['dap', 'PAD', 'mmHg', 0], ['map', 'PAM', 'mmHg', 0],
  ['co', 'Débit cardiaque', 'L/min', 2], ['ef', 'FE', '%', 0, 100], ['reg_fraction', 'Fraction régurgitée', '%', 0, 100],
  ['la_mean', 'POG moyenne', 'mmHg', 1], ['pap_mean', 'PAP moyenne', 'mmHg', 1], ['cvp', 'PVC', 'mmHg', 1]];
const GASES = [['ph', 'pH artériel', '', 3], ['pco2', 'PaCO₂', 'mmHg', 1], ['pao2', 'PaO₂', 'mmHg', 0], ['hco3', 'HCO₃⁻', 'mmol/L', 1],
  ['be', 'Excès de base', 'mmol/L', 1], ['sao2', 'SaO₂', '%', 1], ['lactate', 'Lactate', 'mmol/L', 2], ['ph_v', 'pH veineux mêlé', '', 3],
  ['pco2_v', 'PvCO₂', 'mmHg', 1], ['po2_v', 'PvO₂', 'mmHg', 0]];
const CHEM = [['na', 'Na⁺', 'mmol/L', 0], ['k', 'K⁺', 'mmol/L', 1], ['cl', 'Cl⁻', 'mmol/L', 0], ['anion_gap', 'Trou anionique', 'mmol/L', 1],
  ['glucose', 'Glucose', 'mmol/L', 1], ['creat', 'Créatinine', 'µmol/L', 0], ['urea', 'Urée', 'mmol/L', 1], ['phos', 'Phosphate', 'mmol/L', 2],
  ['alb', 'Albumine', 'g/L', 1], ['hct', 'Hématocrite', '%', 0]];
const INDICATORS = [...HEMO, ['sv', "Volume d'éjection", 'mL', 1], ['lv_edp', 'PTDVG', 'mmHg', 1], ['edv', 'VTDVG', 'mL', 1],
  ...GASES.slice(0, 7), ['rr', 'FR', '/min', 0], ['po2_v', 'PvO₂', 'mmHg', 0], ...CHEM, ['gfr', 'DFG', 'mL/min', 1],
  ['do2_kg', 'DO₂', 'mL/min/kg', 1], ['er', "Extraction d'O₂", '%', 0, 100], ['s_tone', 'Tonus sympathique', '', 2], ['v_tone', 'Tonus vagal', '', 2]];

// ------------------------------------------------------------------ démarrage
async function main() {
  try {
    S.ctl = await Controller.create();
  } catch (e) {
    $('#case-label').textContent = 'Le moteur WebAssembly n\'a pas pu être chargé : ' + e.message;
    return;
  }
  const cat = S.ctl.catalog;
  $('#api').textContent = 'API ' + S.ctl.apiVersion;
  $('#disease').innerHTML = '<option value="">Aucune (sain)</option>' + cat.diseases.map((d) => `<option value="${d.id}">${esc(d.name)} [${d.evidence}]</option>`).join('');
  $('#disease').onchange = () => { fillStages(); };
  $('#evaluate').onclick = () => evaluate();
  for (const b of document.querySelectorAll('#tabs button')) b.onclick = () => { S.tab = b.dataset.tab; render(); };
  window.addEventListener('resize', () => render());
  fillStages();
  applyHash();
  evaluate();
  if (S.treat.autoSim) { S.treat.autoSim = false; try { S.treat.r = S.ctl.treatment(S.inputs, { regimens: S.treat.regimens, horizonH: S.treat.days * 24 }); S.treat.ref = Object.keys(S.treat.r.conc_mg_L)[0]; } catch (e) { showError(e); } render(); }
}

/**
 * Cas partageable par l'adresse (#...) : bw, disease, stage, tab,
 * plan=drogue:mg/kg:voie:intervalle h:nb prises[:début h],... ; days ; t (curseur, h) ; sim=1.
 * Ex. : #disease=mmvd&stage=C&tab=treatment&plan=furosemide:2:po:12:14,pimobendan:0.25:po:12:14&days=7&sim=1
 */
function applyHash() {
  const q = new URLSearchParams(location.hash.slice(1));
  if (q.get('bw')) $('#bw').value = q.get('bw');
  if (q.get('disease')) { $('#disease').value = q.get('disease'); fillStages(); }
  if (q.get('stage')) $('#stage').value = q.get('stage');
  if (q.get('tab')) S.tab = q.get('tab');
  if (q.get('plan')) {
    S.treat.regimens = q.get('plan').split(',').map((x) => {
      const [drug, mg, route, iv, n, st] = x.split(':');
      return { drug, mgPerKg: +mg, route, intervalH: +iv, count: +n, startH: +(st || 0), overrides: {} };
    });
  }
  if (q.get('days')) S.treat.days = +q.get('days');
  if (q.get('t')) S.treat.cursor = +q.get('t');
  S.treat.autoSim = q.get('sim') === '1' && S.treat.regimens.length > 0;
  if (q.get('window')) S.ausc.window = +q.get('window');
}

function fillStages() {
  const d = S.ctl.catalog.diseases.find((x) => x.id === $('#disease').value);
  $('#stage-wrap').hidden = !d;
  $('#stage').innerHTML = d ? d.stages.map((s) => `<option value="${s.name}">${s.name} (s ${s.lo}–${s.hi})</option>`).join('') : '';
}

function evaluate() {
  S.inputs = { ...S.inputs, bw: parseFloat($('#bw').value) || 20, disease: $('#disease').value || null, stage: $('#stage').value || null };
  try {
    $('#error').hidden = true;
    S.res = S.ctl.evaluate(S.inputs);
    stopAusc(false); S.ausc.r = null; S.ausc.played = false; // nouvelle auscultation pour ce patient
  } catch (e) { showError(e); }
  render();
}

function showError(e) { $('#error').hidden = false; $('#error').textContent = 'Erreur : ' + e.message; }

function caseLabel() {
  const k = S.res && S.res.kase;
  const d = S.ctl.catalog.diseases.find((x) => x.id === S.inputs.disease);
  let s = d && k ? `${d.name}, stade ${k.stage} (s = ${fmt(k.severity, 2)})` : 'Chien sain';
  s += `, ${S.inputs.bw} kg`;
  if (S.inputs.plan && S.inputs.atH !== null) s += ` · sous traitement, examiné à ${dayLabel(S.inputs.atH)} (t = ${fmt(S.inputs.atH, 1)} h)`;
  return s;
}

function render() {
  if (!S.res) return;
  for (const b of document.querySelectorAll('#tabs button')) b.classList.toggle('on', b.dataset.tab === S.tab);
  $('#case-label').textContent = caseLabel();
  // bandeau traitement et liste des concentrations fixes
  const tb = $('#treatment-banner');
  tb.hidden = !(S.inputs.plan && S.inputs.atH !== null);
  if (!tb.hidden) {
    tb.innerHTML = `Traitement en cours : patient examiné à ${dayLabel(S.inputs.atH)}.
      <div class="row" style="margin-top:6px"><button id="tb-edit">Modifier</button><button id="tb-stop">Arrêter</button></div>`;
    $('#tb-edit').onclick = () => { S.tab = 'treatment'; render(); };
    $('#tb-stop').onclick = () => { S.inputs.plan = null; S.inputs.atH = null; S.inputs.death = null; evaluate(); };
  }
  renderOutcome(S.res.st.outcome);
  const v = $('#view');
  ({ patient: renderPatient, respiration: renderRespiration, model: renderModel, auscultation: renderAuscultation, treatment: renderTreatment })[S.tab](v);
}

// ------------------------------------------------------------------ Issue (décompensation, décès)
const MECH_FR = {
  acidemia_depression: 'dépression myocardique et vasoplégie par acidémie',
  hypoxic_depression: 'dépression myocardique par hypoxie tissulaire',
  coronary_ischemia: 'ischémie myocardique (pression de perfusion coronaire basse)',
  hypokalemic_weakness: 'faiblesse des muscles respiratoires (hypokaliémie)',
  hyperkalemic_bradycardia: 'frein hyperkaliémique de la FC (K⁺ sur le nœud sinusal)',
  anaerobic_metabolism: "métabolisme anaérobie (extraction d'O₂ élevée, lactate)",
  pulmonary_edema: 'œdème pulmonaire (POG élevée)',
  renal_hypoperfusion: 'hypoperfusion rénale (PAM basse)',
  baroreflex_exhausted: 'baroréflexe épuisé (tonus sympathique maximal)',
};
const DEATH_FR = {
  cardiac_arrest: 'arrêt cardiaque', circulatory_collapse: 'effondrement circulatoire',
  oxygen_debt: "dette d'oxygène (consommation > apport)", no_steady_state: 'aucun état stationnaire viable',
  vicious_circle: 'spirale de décompensation non résolue',
};
const isDead = () => S.res && S.res.st.outcome && S.res.st.outcome.state === 'dead';

function renderOutcome(o) {
  const b = $('#outcome-banner');
  b.className = '';
  if (!o || o.state === 'stable') { b.hidden = true; return; }
  b.hidden = false;
  b.classList.add(o.state);
  const D = '<div class="note">Mécanismes physiologiques du modèle ; seuils et pentes : placeholders (evidence D), ordres de grandeur pédagogiques.</div>';
  if (o.state === 'dead') {
    b.innerHTML = `<b>Décès du patient virtuel : ${esc(o.label_fr)}${o.absorbing_from_h !== undefined ? ` (survenu à ${dayLabel(o.absorbing_from_h)})` : ''}</b>`
      + `<div>Aucun état physiologique viable n'existe pour ce patient. ${'map' in S.res.st ? 'Les valeurs affichées sont celles de la dernière passe viable avant l\'effondrement.' : ''}</div>` + D;
  } else {
    b.innerHTML = `<b>Patient décompensé</b>` + o.mechanisms.map((m) => `<div>• ${esc(m.label_fr)} : ${num(m.value)} (seuil ${num(m.threshold)})${m.factor < 1 ? `, facteur ${fmt(m.factor, 2)}` : ''}</div>`).join('') + D;
  }
}

const deadView = (what) => `<div class="card dead-view">Patient décédé : pas de ${what}.</div>`;

const card = (title, inner = '', extra = '') => `<div class="card" ${extra}><h3>${title}</h3>${inner}</div>`;
const canvas = (id, h) => `<canvas id="${id}" style="height:${h}px"></canvas>`;
const legend = (items) => `<div class="legend">${items.map(([l, c]) => `<span><i style="background:${c}"></i>${l}</span>`).join('')}</div>`;

function valuesTable(rows, st) {
  return `<table class="values">${rows.map(([k, l, u, d, sc]) => `<tr><td>${l}</td><td class="v">${fmt(st[k] * (sc || 1), d)}</td><td class="u">${u}</td></tr>`).join('')}</table>`;
}

// ------------------------------------------------------------------ Patient
function renderPatient(v) {
  const st = S.res.st, sg = st.signals;
  if (!sg) {
    v.innerHTML = deadView('ECG ni de pression pulsée')
      + ('map' in st ? `<div class="grid3">${card('Hémodynamique (dernière passe viable)', valuesTable(HEMO, st))}${card('Gaz du sang', valuesTable(GASES, st))}${card('Biochimie', valuesTable(CHEM, st))}</div>` : '');
    return;
  }
  const pr = sg.pressures;
  v.innerHTML = card(`ECG, dérivation II (${Math.round(sg.ecg.length / sg.ecg_fs)} s, ${fmt(st.hr, 0)} /min)`, canvas('c-ecg', 150))
    + card('Pressions sur deux battements (mmHg) : aorte, VG, OG', canvas('c-pr', 170) + legend([['Aorte', COLORS.ao], ['VG', COLORS.lv], ['OG', COLORS.la]]) + `<div class="note">PCG : ${sg.pcg_features.grade ? `souffle ${sg.pcg_features.murmur}, grade ${sg.pcg_features.grade}/6` : 'pas de souffle'}</div>`)
    + `<div class="grid3">${card('Hémodynamique', valuesTable(HEMO, st))}${card('Gaz du sang', valuesTable(GASES, st))}${card('Biochimie', valuesTable(CHEM, st))}</div>`
    + card('Signes émergents', st.emergent_signs.length ? st.emergent_signs.map((s) => `<div>• ${esc(s.label_fr)}<div class="note">&nbsp;&nbsp;${esc(s.source)}</div></div>`).join('') : '<div class="note">Aucun signe dérivé au-delà des seuils sourcés.</div>');
  const dur = sg.ecg.length / sg.ecg_fs;
  timeChart($('#c-ecg'), { series: [{ y: sg.ecg, dt: 1 / sg.ecg_fs, color: COLORS.ecg }], x0: 0, x1: dur, paper: true });
  const two = (a) => [...a, ...a];
  const p = pr.dt * pr.ao.length * 2;
  timeChart($('#c-pr'), { series: [two(pr.ao), two(pr.lv), two(pr.la)].map((y, i) => ({ y, dt: pr.dt, color: [COLORS.ao, COLORS.lv, COLORS.la][i] })), x0: 0, x1: p, yMin: 0 });
}

// ------------------------------------------------------------------ Respiration
function renderRespiration(v) {
  if (!S.res.st.signals) { v.innerHTML = deadView('respiration'); return; }
  const rs = S.res.st.signals.respiration, dt = 1 / rs.fs, dur = rs.volume_mL.length * dt;
  const avg = (a) => a.reduce((x, y) => x + y, 0) / a.length;
  v.innerHTML = `<div class="note" style="font-size:13px">FR ${fmt(rs.rr_per_min, 0)} /min · volume courant alvéolaire ${fmt(rs.vt_alv_mL, 0)} mL (VA/FR ; l'espace mort n'est pas modélisé) · inspiration sur 40 % du cycle</div>`
    + card(`Volume pulmonaire au-dessus de la fin d'expiration (mL), ${Math.round(dur)} s`, canvas('r-vol', 110))
    + card('Débit respiratoire (mL/s) : > 0 inspiration, < 0 expiration', canvas('r-flow', 110))
    + card('Fréquence cardiaque instantanée (/min) : arythmie sinusale respiratoire', canvas('r-hr', 110))
    + card('Pressions artérielles instantanées (mmHg) : systolique, moyenne, diastolique', canvas('r-ap', 160) + legend([['PAS', COLORS.ao], ['PAM', COLORS.pa], ['PAD', COLORS.lv]])
      + `<div class="note">PAS et PAD = PAM instantanée du modèle autonome ± écart du battement simulé (PAS ${fmt(avg(rs.sap), 0)}, PAD ${fmt(avg(rs.dap), 0)} mmHg en moyenne) : pression pulsée supposée constante.</div>`);
  const T = (id, series) => timeChart($(id), { series, x0: 0, x1: dur });
  T('#r-vol', [{ y: rs.volume_mL, dt, color: COLORS.accent }]);
  T('#r-flow', [{ y: rs.flow_mL_s, dt, color: '#5b8c2a' }]);
  T('#r-hr', [{ y: rs.hr, dt, color: COLORS.ecg }]);
  T('#r-ap', [{ y: rs.sap, dt, color: COLORS.ao }, { y: rs.map, dt, color: COLORS.pa }, { y: rs.dap, dt, color: COLORS.lv }]);
}

// ------------------------------------------------------------------ Schéma du modèle
const NODES = [['patient', 0, 0, 'b'], ['volume', 1, 0, 'b'], ['autonomic', 2, 0, 'b'], ['disease', 3, 0, 's'], ['drugs', 4, 0, 's'],
  ['systemic_circulation', 0, 1, 'b'], ['right_heart', 1, 1, 'b'], ['valves', 2, 1, 'b'], ['left_heart', 3, 1, 'b'], ['pulmonary_circulation', 4, 1, 'b'],
  ['kidney', 0, 2, 'b'], ['metabolism', 1, 2, 'b'], ['oxygen_transport', 2, 2, 'b'], ['acid_base', 3, 2, 'b'], ['ventilation', 4, 2, 'b'],
  ['liver', 0, 3, 'b'], ['ecg', 1, 3, 'b'], ['pcg', 2, 3, 'o'], ['gases', 3, 3, 'o'], ['respiration', 4, 3, 'o']];
const EDGES = [['patient', 'volume', '85 mL/kg × bvf'], ['volume', 'systemic_circulation', 'volume veineux'],
  ['autonomic', 'systemic_circulation', 'PAM ⇄ r_sys, recrutement', 1], ['autonomic', 'right_heart', 'inotropie'], ['autonomic', 'left_heart', 'inotropie, FC'],
  ['systemic_circulation', 'right_heart', 'retour veineux'], ['right_heart', 'valves', 'tricuspide, pulmonaire', 1], ['valves', 'left_heart', 'mitrale, aortique', 1],
  ['right_heart', 'pulmonary_circulation', 'débit pulmonaire', 0, -0.22], ['pulmonary_circulation', 'left_heart', 'remplissage (POG)'],
  ['left_heart', 'systemic_circulation', 'éjection aortique', 0, 0.16], ['systemic_circulation', 'kidney', 'PAM, DC → perfusion'],
  ['systemic_circulation', 'metabolism', 'DC → DO₂'], ['oxygen_transport', 'metabolism', 'CaO₂'], ['metabolism', 'acid_base', 'lactate', 0, 0.25],
  ['acid_base', 'ventilation', 'pH ⇄ PaCO₂', 1], ['ventilation', 'oxygen_transport', 'PaO₂', 0, -0.25], ['acid_base', 'oxygen_transport', 'pH (Bohr)'],
  ['pulmonary_circulation', 'ventilation', 'POG → œdème (A-a)'], ['ventilation', 'autonomic', 'respiration → RSA', 0, 0.12], ['autonomic', 'ecg', 'instants R', 0, -0.08],
  ['acid_base', 'ecg', 'K⁺', 0, 0.1], ['valves', 'pcg', 'débits valvulaires', 0, 0.05], ['acid_base', 'gases', 'pH, HCO₃⁻, BE'],
  ['oxygen_transport', 'gases', 'SaO₂, PvO₂'], ['ventilation', 'respiration', 'FR, volume courant']];

function renderModel(v) {
  const cat = S.ctl.catalog, r = S.res;
  const disease = cat.diseases.find((d) => d.id === r.disease);
  const drugs = cat.drugs.filter((d) => r.drugIds.includes(d.id));
  const dBlocks = new Set(disease ? disease.effects.flatMap((e) => e.blocks) : []);
  const gBlocks = new Set(drugs.flatMap((d) => d.effects.flatMap((e) => e.blocks)));
  const count = {}; for (const c of r.changes) for (const b of c.blocks) count[b] = (count[b] || 0) + 1;
  const label = (id) => ({ disease: disease ? `Maladie : ${disease.name.match(/\(([^)]+)\)/)?.[1] || disease.name}` : 'Maladie (module JSON)',
    drugs: drugs.length ? 'Médicaments : ' + drugs.map((d) => d.name).join(', ') : 'Médicaments (PK/PD)', pcg: 'PCG / auscultation',
    gases: 'Gaz du sang, biochimie', respiration: 'Courbes respiratoires' }[id] || (cat.blocks.find((b) => b.id === id) || {}).label_fr || id);
  const W = 1000, H = 560, nw = W / 5 - 40, nh = H / 4 - 46;
  const C = (n) => [(n[1] + 0.5) * W / 5, (n[2] + 0.5) * H / 4];
  const byId = Object.fromEntries(NODES.map((n) => [n[0], n]));
  const ctrl = (a, b, bend) => { const m = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], L = Math.hypot(b[0] - a[0], b[1] - a[1]); return [m[0] - (b[1] - a[1]) / L * bend * L, m[1] + (b[0] - a[0]) / L * bend * L]; };
  const clip = (c, to) => { const dx = to[0] - c[0], dy = to[1] - c[1]; const s = Math.min(dx ? nw / 2 / Math.abs(dx) : 1e9, dy ? nh / 2 / Math.abs(dy) : 1e9, 1); return [c[0] + dx * s, c[1] + dy * s]; };
  let svg = `<svg viewBox="0 0 ${W} ${H}" style="width:100%;background:white;border-radius:12px">
    <defs>${['g', 'r', 'b'].map((k) => `<marker id="ah-${k}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="${{ g: '#8a99a6', r: COLORS.ao, b: COLORS.lv }[k]}"/></marker>`).join('')}</defs>`;
  const path = (a, b, bend, color, k, dash, bidir) => {
    const c = ctrl(a, b, bend), s = clip(a, c), e = clip(b, c);
    return { d: `<path d="M${s[0]},${s[1]} Q${c[0]},${c[1]} ${e[0]},${e[1]}" fill="none" stroke="${color}" stroke-width="1.6" ${dash ? 'stroke-dasharray="7 6"' : ''} marker-end="url(#ah-${k})" ${bidir ? `marker-start="url(#ah-${k})"` : ''}/>`, mid: [0.25 * s[0] + 0.5 * c[0] + 0.25 * e[0], 0.25 * s[1] + 0.5 * c[1] + 0.25 * e[1]], len: Math.hypot(e[0] - s[0], e[1] - s[1]) };
  };
  let labels = '';
  for (const [f, t, lab, bidir, bend = 0] of EDGES) {
    const p = path(C(byId[f]), C(byId[t]), bend, '#8a99a6', 'g', false, bidir);
    svg += p.d;
    if (!(byId[f][2] === byId[t][2] && p.len < lab.length * 6 + 12)) labels += `<text x="${p.mid[0]}" y="${p.mid[1] + 3}" font-size="10" fill="${COLORS.muted}" text-anchor="middle" paint-order="stroke" stroke="white" stroke-width="4">${esc(lab)}</text>`;
  }
  for (const b of dBlocks) if (byId[b]) svg += path(C(byId.disease), C(byId[b]), 0.08, COLORS.ao, 'r', true).d;
  for (const b of gBlocks) if (byId[b]) svg += path(C(byId.drugs), C(byId[b]), -0.08, COLORS.lv, 'b', true).d;
  svg += labels;
  for (const n of NODES) {
    const [x, y] = C(n), id = n[0], k = count[id];
    let fill = 'white', stroke = '#b8c4ce';
    if ((id === 'disease' && disease) || (dBlocks.has(id) && k)) { fill = '#fde7e4'; stroke = COLORS.ao; }
    else if ((id === 'drugs' && drugs.length) || (gBlocks.has(id) && k)) { fill = '#e3edf9'; stroke = COLORS.lv; }
    else if (n[3] !== 'b') { fill = '#f1f4f6'; stroke = '#8a99a6'; }
    const sel = S.model.selected === id;
    const words = label(id).split(' '); const lines = []; let cur = '';
    for (const w of words) { if ((cur + ' ' + w).trim().length > 24) { lines.push(cur); cur = w; } else cur = (cur + ' ' + w).trim(); }
    lines.push(cur); if (k && n[3] === 'b') lines.push(`${k} paramètre${k > 1 ? 's' : ''} modifié${k > 1 ? 's' : ''}`);
    svg += `<g data-node="${id}" style="cursor:${n[3] === 'b' ? 'pointer' : 'default'}"><rect x="${x - nw / 2}" y="${y - nh / 2}" width="${nw}" height="${nh}" rx="10" fill="${fill}" stroke="${sel ? COLORS.ink : stroke}" stroke-width="${sel ? 3 : 1.5}"/>`
      + lines.map((l, i) => `<text x="${x}" y="${y + (i - (lines.length - 1) / 2) * 14 + 4}" font-size="12" text-anchor="middle" fill="${COLORS.ink}" ${k && i === lines.length - 1 ? 'font-weight="600"' : ''}>${esc(l)}</text>`).join('') + '</g>';
  }
  svg += '</svg>';
  let detail = '';
  const blk = cat.blocks.find((b) => b.id === S.model.selected);
  if (blk) {
    const ch = Object.fromEntries(r.changes.map((c) => [c.param, c]));
    const coup = EDGES.filter((e) => e[0] === blk.id || e[1] === blk.id).map((e) => `${e[3] ? '⇄' : e[0] === blk.id ? '→' : '←'} ${label(e[0] === blk.id ? e[1] : e[0])} (${e[2]})`).join(' · ');
    detail = card(esc(blk.label_fr), `<div class="note">Couplages : ${esc(coup)}</div>` + blk.params.map((p) => ch[p] ? `<div><b>• ${p} : ${num(ch[p].healthy)} → ${num(ch[p].patient)}</b> <span class="note">(${esc(ch[p].sources.join(', ') || 'effet indirect')})</span></div>` : `<div class="note">• ${p}</div>`).join(''));
  }
  const table = card('Paramètres modifiés par rapport au chien sain de même poids', r.changes.length
    ? `<table class="values">${r.changes.map((c) => `<tr><td>${c.param}</td><td>${esc(c.blocks.map((b) => (cat.blocks.find((x) => x.id === b) || {}).label_fr).join(', '))}</td><td class="v">${num(c.healthy)} → ${num(c.patient)}</td><td>${esc(c.sources.join(', '))}</td></tr>`).join('')}</table>`
    : '<div class="note">Aucun : patient sain.</div>');
  v.innerHTML = `<div class="legend"><span><i style="background:${COLORS.ao}"></i>modifié par la maladie</span><span><i style="background:${COLORS.lv}"></i>modifié par un médicament</span><span>Cliquer un bloc pour ses paramètres.</span></div>` + svg + detail + table;
  for (const g of v.querySelectorAll('g[data-node]')) g.onclick = () => { const id = g.dataset.node; if (cat.blocks.some((b) => b.id === id)) { S.model.selected = S.model.selected === id ? null : id; render(); } };
}

function num(x) { const a = Math.abs(x); return fmt(x, a === 0 ? 0 : a >= 100 ? 1 : a >= 10 ? 2 : a >= 1 ? 3 : a >= 0.01 ? 4 : 6); }

// ------------------------------------------------------------------ Auscultation
function renderAuscultation(v) {
  const A = S.ausc;
  if (isDead()) { v.innerHTML = deadView('bruit cardiaque : silence à l\'auscultation'); return; }
  if (!A.r) {
    try { A.r = S.ctl.auscultate(S.inputs, A.hr, A.site); A.spec = computeSpectrogram(A.r.samples, A.r.fs); A.cursor = null; A.start = 0; A.window = Math.min(A.window, A.r.duration_s); }
    catch (e) { showError(e); return; }
  }
  const r = A.r, dur = r.duration_s;
  const chip = (lab, on, act) => `<button class="${on ? 'on' : ''}" data-act="${act}">${lab}</button>`;
  v.innerHTML = `<div class="row">FC : ${chip('Patient', A.hr === null, 'hr:')} ${chip('Repos 72', A.hr === 72, 'hr:72')} ${chip('90', A.hr === 90, 'hr:90')}
      &nbsp; Site : ${chip('Base', A.site === 'base', 'site:base')} ${chip('Apex (non calibré)', A.site === 'apex', 'site:apex')}</div>
    <div class="row"><button class="primary" id="play">${A.playing ? '■ Arrêter' : A.played ? '↻ Rejouer' : '▶ Écouter'}</button>
      <label style="flex-direction:row;align-items:center"><input type="checkbox" id="loop" ${A.loop ? 'checked' : ''}> en boucle</label>
      <label style="flex-direction:row;align-items:center"><input type="checkbox" id="zoomonly" ${A.zoomOnly ? 'checked' : ''}> fenêtre affichée seulement</label>
      <span>FC ${fmt(r.hr, 0)} /min · ${r.beats.length} battements · ${r.grade ? `souffle ${r.murmur}, grade ${r.grade}/6 (indicatif)` : 'pas de souffle'}${r.s3 ? ' · B3' : ''} · site ${r.site}</span></div>
    <div class="sliders"><label><span>Fenêtre : <b id="wl">${fmt(A.window, 1)}</b> s</span><input id="win" type="range" min="0.5" max="${dur}" step="0.1" value="${A.window}"></label>
      <label><span>Début : <b id="sl">${fmt(A.start, 2)}</b> s</span><input id="start" type="range" min="0" max="${Math.max(0.001, dur - A.window)}" step="0.01" value="${A.start}"></label></div>`
    + card(`Phonocardiogramme (${r.fs} Hz)`, canvas('a-pcg', 120))
    + card('Spectrogramme 0–500 Hz (dB)', canvas('a-spec', 110))
    + card('ECG, dérivation II (mêmes battements que le son)', canvas('a-ecg', 110))
    + card('Pressions (mmHg) : aorte, VG, OG', canvas('a-pr', 150) + legend([['Aorte', COLORS.ao], ['VG', COLORS.lv], ['OG', COLORS.la]]))
    + '<div class="note">Son synthétisé (module auscultation, calé sur deux enregistrements) ; pressions : battement simulé posé à chaque battement, forme non recalculée battement par battement. Le préréglage 72/min est un réglage d\'usage. Usage pédagogique uniquement.</div>';
  for (const b of v.querySelectorAll('button[data-act]')) b.onclick = () => {
    const [k, val] = b.dataset.act.split(':');
    if (k === 'hr') A.hr = val ? Number(val) : null; else A.site = val;
    stopAusc(false); A.r = null; A.played = false; render();
  };
  $('#win').oninput = (e) => { A.window = +e.target.value; A.start = Math.min(A.start, Math.max(0, dur - A.window)); drawAusc(); $('#wl').textContent = fmt(A.window, 1); $('#start').max = Math.max(0.001, dur - A.window); };
  $('#start').oninput = (e) => { A.start = +e.target.value; $('#sl').textContent = fmt(A.start, 2); drawAusc(); };
  $('#play').onclick = () => (A.playing ? stopAusc() : play());
  $('#loop').onchange = (e) => { A.loop = e.target.checked; if (A.src) A.src.loop = A.loop; };
  $('#zoomonly').onchange = (e) => { A.zoomOnly = e.target.checked; };
  drawAusc();
}

function drawAusc() {
  const A = S.ausc, r = A.r, x0 = A.start, x1 = A.start + A.window;
  timeChart($('#a-pcg'), { series: [{ y: r.samples, dt: 1 / r.fs, color: COLORS.accent }], x0, x1, yMin: -1, yMax: 1, cursor: A.cursor });
  spectrogram($('#a-spec'), A.spec, x0, x1, A.cursor);
  timeChart($('#a-ecg'), { series: [{ y: r.ecg, dt: 1 / r.ecg_fs, color: COLORS.ecg }], x0, x1, cursor: A.cursor });
  const p = r.pressures, hi = Math.max(...p.ao, ...p.lv) * 1.05;
  timeChart($('#a-pr'), { series: [p.ao, p.lv, p.la].map((y, i) => ({ y, dt: 1 / p.fs, color: [COLORS.ao, COLORS.lv, COLORS.la][i] })), x0, x1, yMin: 0, yMax: hi, cursor: A.cursor });
}

/** Lecture Web Audio ; le curseur suit l'horloge audio et la fenêtre suit le curseur. */
/** Lecture (ou relecture) : tout l'enregistrement, ou la fenêtre affichée ; en boucle si demandé. */
function play() {
  const A = S.ausc, r = A.r;
  stopAusc(false);
  const ctx = A.audio || (A.audio = new AudioContext());
  if (ctx.state === 'suspended') ctx.resume();
  const buf = ctx.createBuffer(1, r.samples.length, r.fs);
  buf.copyToChannel(r.samples, 0);
  const src = ctx.createBufferSource(); src.buffer = buf; src.connect(ctx.destination);
  const off = A.zoomOnly ? A.start : 0, len = A.zoomOnly ? Math.min(A.window, r.duration_s - off) : r.duration_s;
  src.loop = A.loop; src.loopStart = off; src.loopEnd = off + len;
  const t0 = ctx.currentTime + 0.05;
  if (A.loop) src.start(t0, off); else src.start(t0, off, len);
  A.src = src; A.playing = true; A.played = true;
  if (!A.zoomOnly) A.start = 0;
  const me = src;
  src.onended = () => { if (A.src !== me) return; A.playing = false; A.src = null; if (S.tab === 'auscultation') render(); };
  render();
  const tick = () => {
    if (A.src !== me || !A.playing) return;
    if (S.tab === 'auscultation' && $('#a-pcg')) {
      const el = ctx.currentTime - t0;
      const t = off + (A.loop ? ((el % len) + len) % len : Math.min(el, len));
      A.cursor = Math.max(0, t);
      if (!A.zoomOnly && t > A.start + 0.9 * A.window) A.start = Math.min(Math.max(0, r.duration_s - A.window), t - 0.1 * A.window);
      if (!A.zoomOnly && t < A.start) A.start = Math.max(0, t - 0.1 * A.window);
      drawAusc();
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

/** Arrête la lecture en cours. */
function stopAusc(rerender = true) {
  const A = S.ausc;
  if (A.src) { const s = A.src; A.src = null; try { s.stop(); } catch (e) { /* déjà arrêté */ } }
  A.playing = false;
  if (rerender && S.tab === 'auscultation') render();
}

// ------------------------------------------------------------------ Traitement (PK/PD)
function renderTreatment(v) {
  const T = S.treat, cat = S.ctl.catalog;
  const dname = (id) => cat.drugs.find((d) => d.id === id).name;
  const ed = T.editing !== null ? T.regimens[T.editing] : { drug: cat.drugs[0].id, mgPerKg: 1, route: 'po', intervalH: 12, count: 6, startH: 0 };
  let html = '<div class="warn">Exploration pédagogique de la PK/PD : les doses sont libres et n\'ont valeur ni de recommandation ni de posologie. PK de l\'atropine, du bêtabloquant et de l\'opioïde, et PD de tous les médicaments : placeholders (evidence D). Ne jamais utiliser pour décider d\'une dose ou d\'un traitement réel.</div>';
  html += card("Schéma d'administration", `<div class="row">
      <select id="t-drug">${cat.drugs.map((d) => `<option value="${d.id}" ${d.id === ed.drug ? 'selected' : ''}>${esc(d.name)} [${d.evidence}]</option>`).join('')}</select>
      <label>mg/kg<input id="t-dose" type="number" step="any" min="0" value="${ed.mgPerKg}"></label>
      <label>voie<select id="t-route"><option value="po" ${ed.route === 'po' ? 'selected' : ''}>per os</option><option value="iv" ${ed.route === 'iv' ? 'selected' : ''}>IV bolus</option></select></label>
      <label>toutes les (h)<input id="t-int" type="number" step="any" min="0.1" value="${ed.intervalH}"></label>
      <label>nb de prises<input id="t-n" type="number" min="1" max="200" value="${ed.count}"></label>
      <label>début (h)<input id="t-start" type="number" step="any" min="0" value="${ed.startH}"></label>
      <button class="primary" id="t-add">${T.editing === null ? 'Ajouter' : 'Enregistrer'}</button>${T.editing !== null ? '<button id="t-cancel">Annuler</button>' : ''}</div>`
    + T.regimens.map((r, i) => {
      const om = Object.values(r.overrides || {}).filter((x) => x === 0).length;
      let s = `<div class="row"><i style="display:inline-block;width:12px;height:12px;border-radius:3px;background:${COLORS.drugs[i % 5]}"></i>
        ${esc(dname(r.drug))} : ${r.mgPerKg} mg/kg ${r.route === 'po' ? 'per os' : 'IV'}, toutes les ${r.intervalH} h × ${r.count} (de ${dayLabel(r.startH)} à ${dayLabel(r.startH + (r.count - 1) * r.intervalH)})${om ? ` ; ${om} omise(s)` : ''}
        <button data-ed="${i}">Modifier</button><button data-op="${i}">${T.open === i ? 'Masquer les prises' : 'Prises individuelles'}</button><button data-rm="${i}">Retirer</button></div>`;
      if (T.open === i) {
        s += '<div class="note">Dose de chaque prise (mg/kg) ; 0 = prise omise</div><div class="row">'
          + Array.from({ length: r.count }, (_, k) => `<label>${dayLabel(r.startH + k * r.intervalH)}<input data-dose="${i}:${k}" type="number" step="any" min="0" style="width:84px" value="${r.overrides && r.overrides[k] !== undefined ? r.overrides[k] : r.mgPerKg}"></label>`).join('') + '</div>';
      }
      return s;
    }).join('')
    + `<div class="row">Durée simulée : <b id="t-dl">${T.days}</b> j <input id="t-days" type="range" min="1" max="14" step="1" value="${T.days}" style="width:240px">
       <button class="primary" id="t-sim" ${T.regimens.length ? '' : 'disabled'}>Simuler le traitement</button></div>`);
  const r = T.r;
  if (r) {
    const h = r.plan.horizonH, i = nearest(r.t, T.cursor);
    html += card(`Instant d'examen : ${dayLabel(T.cursor)} (t = ${fmt(T.cursor, 1)} h)`, `<div class="row">
        ${T.ref ? `<button id="t-trough">◀ creux</button><button id="t-peak">pic ▶</button>
        <select id="t-ref">${Object.keys(r.conc_mg_L).map((d) => `<option value="${d}" ${d === T.ref ? 'selected' : ''}>repère : ${esc(dname(d))}</option>`).join('')}</select>` : ''}
        <button class="danger" id="t-exam">Examiner le patient à cet instant</button></div>
        <input id="t-cur" type="range" min="0" max="${h}" step="0.05" value="${T.cursor}">
        <div class="note">${Object.keys(r.conc_mg_L).map((d) => `${esc(dname(d))} : ${fmt(r.conc_mg_L[d][i], 4)} mg/L, E = ${fmt(r.effect[d][i], 2)}`).join(' &nbsp; ')}</div>
        ${outcomeNote(r)}`);
    for (const d of Object.keys(r.conc_mg_L)) html += card(`Concentration plasmatique : ${esc(dname(d))} (mg/L) ; pointillés : CE50 = ${cat.drugs.find((x) => x.id === d).ec50_mg_L} mg/L`, canvas('tc-' + d, 120));
    html += card('Effet pharmacodynamique E(t) (0 à 1)', canvas('t-eff', 100));
    if (r.D.some((x) => x !== 0)) {
      html += card('Diurétique : déficit de volume sanguin D (mL)', canvas('t-D', 90))
        + card('Diurétique : diurèse induite cumulée avec récupération U (mL/kg)', canvas('t-U', 90)
          + '<div class="note">Débit urinaire induit u(t) = 0,25 · E(C) mL/kg/min (supplément dû au médicament ; la diurèse de base n\'est pas modélisée). U(t) = ∫ 60 u(s) e^(−(t−s)/24 h) ds : volume induit cumulé avec oubli exponentiel (τ = 24 h) ; ce n\'est pas le volume d\'urine émis mais les pertes non encore récupérées, qui fixent les électrolytes (K⁺ − 0,04 U, Cl⁻ − 0,5 U, Na⁺ − 0,3 U). D : part prélevée sur la volémie (30 % de u), nette du retour (τ = 4 h). Paramètres PD : placeholders (D).</div>');
    }
    html += card('Indicateurs à suivre (cliquer pour ajouter ou retirer)', `<div class="chips">${INDICATORS.filter(([k]) => k in r.states).map(([k, l]) => `<button data-ind="${k}" class="${T.shown.includes(k) ? 'on' : ''}">${l}</button>`).join('')}</div>`);
    for (const k of T.shown) {
      const ind = INDICATORS.find((x) => x[0] === k); if (!ind || !r.states[k]) continue;
      const j = nearest(r.state_t, T.cursor);
      html += card(`${ind[1]}${ind[2] ? ` (${ind[2]})` : ''} : ${fmt(r.states[k][j] * (ind[4] || 1), Math.max(ind[3], 2))} à ${dayLabel(r.state_t[j])}`, canvas('ti-' + k, 90));
    }
  }
  v.innerHTML = html;
  // événements
  $('#t-add').onclick = () => {
    const reg = { drug: $('#t-drug').value, mgPerKg: +$('#t-dose').value, route: $('#t-route').value, intervalH: +$('#t-int').value, count: Math.round(+$('#t-n').value), startH: +$('#t-start').value || 0, overrides: {} };
    if (!(reg.mgPerKg >= 0 && reg.intervalH > 0 && reg.count >= 1 && reg.count <= 200)) return;
    if (T.editing === null) T.regimens = [...T.regimens, reg];
    else { const old = T.regimens[T.editing]; reg.overrides = Object.fromEntries(Object.entries(old.overrides || {}).filter(([k]) => +k < reg.count)); T.regimens = T.regimens.map((x, j) => (j === T.editing ? reg : x)); T.editing = null; }
    render();
  };
  if ($('#t-cancel')) $('#t-cancel').onclick = () => { T.editing = null; render(); };
  for (const b of v.querySelectorAll('[data-ed]')) b.onclick = () => { T.editing = +b.dataset.ed; render(); };
  for (const b of v.querySelectorAll('[data-op]')) b.onclick = () => { T.open = T.open === +b.dataset.op ? null : +b.dataset.op; render(); };
  for (const b of v.querySelectorAll('[data-rm]')) b.onclick = () => { T.regimens = T.regimens.filter((_, j) => j !== +b.dataset.rm); T.editing = null; T.open = null; render(); };
  for (const inp of v.querySelectorAll('[data-dose]')) inp.onchange = () => {
    const [i, k] = inp.dataset.dose.split(':').map(Number), val = parseFloat(inp.value);
    if (!(val >= 0)) return;
    const reg = { ...T.regimens[i], overrides: { ...(T.regimens[i].overrides || {}) } };
    if (val === reg.mgPerKg) delete reg.overrides[k]; else reg.overrides[k] = val;
    T.regimens = T.regimens.map((x, j) => (j === i ? reg : x)); render();
  };
  $('#t-days').oninput = (e) => { T.days = +e.target.value; $('#t-dl').textContent = T.days; };
  $('#t-sim').onclick = () => {
    try {
      T.r = S.ctl.treatment(S.inputs, { regimens: T.regimens, horizonH: T.days * 24 });
      T.cursor = Math.min(T.cursor, T.days * 24);
      if (!T.ref || !(T.ref in T.r.conc_mg_L)) T.ref = Object.keys(T.r.conc_mg_L)[0];
      render();
    } catch (e) { showError(e); }
  };
  if (!r) return;
  const setCursor = (t) => { T.cursor = Math.max(0, Math.min(r.plan.horizonH, t)); render(); };
  $('#t-cur').oninput = (e) => { T.cursor = +e.target.value; drawTreatment(); };
  $('#t-cur').onchange = () => render();
  if ($('#t-trough')) {
    $('#t-trough').onclick = () => setCursor(extremum(r.t, r.conc_mg_L[T.ref], T.cursor, false, false));
    $('#t-peak').onclick = () => setCursor(extremum(r.t, r.conc_mg_L[T.ref], T.cursor, true, true));
    $('#t-ref').onchange = (e) => { T.ref = e.target.value; };
  }
  $('#t-exam').onclick = () => {
    const dead = r.death_t_h !== null && r.death_t_h !== undefined;
    const cause = dead ? r.outcomes.find((x) => x.state === 'dead').cause : null;
    S.inputs.plan = r.plan; S.inputs.atH = T.cursor;
    S.inputs.death = dead ? { t_h: r.death_t_h, cause, label_fr: DEATH_FR[cause] || 'décès' } : null;
    S.tab = 'patient'; evaluate();
  };
  for (const b of v.querySelectorAll('[data-ind]')) b.onclick = () => { const k = b.dataset.ind; T.shown = T.shown.includes(k) ? T.shown.filter((x) => x !== k) : [...T.shown, k]; render(); };
  for (const c of v.querySelectorAll('canvas')) c.onclick = (e) => {
    const rect = c.getBoundingClientRect(), left = 44;
    setCursor(((e.clientX - rect.left - left) / (rect.width - left)) * r.plan.horizonH);
  };
  drawTreatment();
}

/** Issue à l'instant du curseur et instant du décès éventuel. */
function outcomeNote(r) {
  const T = S.treat, o = r.outcomes[nearest(r.state_t, T.cursor)];
  let s = '';
  if (r.death_t_h !== null && r.death_t_h !== undefined) s += `<div class="banner">Décès du patient virtuel à ${dayLabel(r.death_t_h)} (t = ${fmt(r.death_t_h, 2)} h, grille d'états) : ${esc(DEATH_FR[r.outcomes.find((x) => x.state === 'dead').cause] || 'décès')}.</div>`;
  if (o && o.state === 'decompensated') s += `<div class="warn">À cet instant : décompensé (${o.mechanisms.map((m) => esc(MECH_FR[m] || m)).join(' ; ')}).</div>`;
  if (o && o.state === 'dead') s += '<div class="warn">À cet instant : patient décédé.</div>';
  return s + '<div class="note">Bandes : orange = décompensé, rouge = décès (placeholders D).</div>';
}

/** Intervalles de la grille d'états où le patient est décompensé ou décédé. */
function outcomeBands(r) {
  const out = [], t = r.state_t;
  for (let i = 0; i < t.length; i++) {
    const st = r.outcomes[i].state;
    if (st === 'stable') continue;
    const t0 = i ? (t[i - 1] + t[i]) / 2 : t[0], t1 = i < t.length - 1 ? (t[i] + t[i + 1]) / 2 : t[i];
    out.push({ t0, t1, color: st === 'dead' ? 'rgba(192,57,43,0.22)' : 'rgba(240,160,60,0.18)' });
  }
  return out;
}

function drawTreatment() {
  const T = S.treat, r = T.r, h = r.plan.horizonH;
  const marks = r.plan.regimens.flatMap((reg, i) => planDoses({ regimens: [reg] }).map((d) => ({ t: d.t_h, color: COLORS.drugs[i % 5] })));
  const col = (d) => COLORS.drugs[Math.max(0, r.plan.regimens.findIndex((x) => x.drug === d)) % 5];
  const base = { x0: 0, x1: h, cursor: T.cursor, marks, ticks: 'treatment', bands: outcomeBands(r) };
  for (const d of Object.keys(r.conc_mg_L)) {
    const c = document.getElementById('tc-' + d);
    if (c) timeChart(c, { ...base, series: [{ y: r.conc_mg_L[d], t: r.t, color: col(d) }], refLines: [S.ctl.catalog.drugs.find((x) => x.id === d).ec50_mg_L] });
  }
  timeChart($('#t-eff'), { ...base, series: Object.keys(r.effect).map((d) => ({ y: r.effect[d], t: r.t, color: col(d) })), yMin: 0, yMax: 1 });
  if ($('#t-D')) {
    timeChart($('#t-D'), { ...base, series: [{ y: r.D, t: r.t, color: COLORS.accent }] });
    timeChart($('#t-U'), { ...base, series: [{ y: r.U, t: r.t, color: '#5b8c2a' }] });
  }
  for (const k of T.shown) {
    const c = document.getElementById('ti-' + k), ind = INDICATORS.find((x) => x[0] === k);
    if (c && r.states[k]) timeChart(c, { ...base, series: [{ y: r.states[k].map((x) => x * (ind[4] || 1)), t: r.state_t, color: COLORS.ink }], points: true });
  }
}

function nearest(t, x) { let b = 0; for (let i = 0; i < t.length; i++) if (Math.abs(t[i] - x) < Math.abs(t[b] - x)) b = i; return b; }

/** Prochain pic (maximum local) après x, ou dernier creux (minimum local) avant x. */
function extremum(t, c, x, forward, max) {
  const idx = forward ? [...Array(c.length - 2).keys()].map((i) => i + 1).filter((i) => t[i] > x + 1e-9)
    : [...Array(c.length - 2).keys()].map((i) => c.length - 2 - i).filter((i) => t[i] < x - 1e-9);
  for (const i of idx) if (max ? c[i] >= c[i - 1] && c[i] > c[i + 1] : c[i] <= c[i - 1] && c[i] < c[i + 1]) return t[i];
  return x;
}

main();
