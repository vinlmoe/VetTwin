// VetTwins (web) : interface du patient virtuel canin. Le moteur vetsim tourne en
// WebAssembly dans le navigateur (aucun serveur de calcul, aucune donnée envoyée).
// Usage pédagogique uniquement.
import { Controller, BackgroundEngine, Superseded, planDoses } from './engine.js';
import { COLORS, timeChart, spectrogram, computeSpectrogram, dayLabel } from './charts.js';
import { mountHeart } from './heart.js';
import { renderAnesthesia } from './anesthesia.js';

const $ = (sel) => document.querySelector(sel);
const fmt = (v, d) => (v === undefined || v === null || Number.isNaN(v) ? '—' : Number(v).toFixed(d).replace('.', ','));
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

const S = {
  ctl: null, tab: 'patient', bg: new BackgroundEngine(),
  inputs: { bw: 20, disease: null, stage: null, seed: 1, plan: null, atH: null },
  res: null,
  ausc: { hr: null, site: 'base', r: null, spec: null, window: 8, start: 0, cursor: null, playing: false, audio: null, src: null, loop: false, zoomOnly: false, played: false, gain: 1, gainNode: null },
  mc: { n: 1, dog: 1, individual: true, src: { natural: false, disease: false, drug: true, measurement: false }, progression: 0, running: false, progress: 0, res: null, sig: null, worker: null, error: null },
  treat: { regimens: [], days: 3, r: null, cursor: 0, shown: ['hr', 'map', 'k'], ref: null, editing: null, open: null, pending: false, t0: 0 },
  model: { selected: null },
  heart: { r: null, t: 0, speed: 0.25, playing: true, raf: null, last: null },
};

// ------------------------------------------------------------------ données d'affichage
// Échelle unique de l'ECG (mV) : amplitudes comparables d'un patient à l'autre (onde T ample,
// QRS large d'un battement ventriculaire…), sans mise à l'échelle automatique.
const ECG_MIN = -1.5, ECG_MAX = 2.5;
const HEMO = [['hr', 'FC', '/min', 0], ['sap', 'PAS', 'mmHg', 0], ['dap', 'PAD', 'mmHg', 0], ['map', 'PAM', 'mmHg', 0],
  ['co', 'Débit cardiaque', 'L/min', 2], ['ef', 'FE', '%', 0, 100], ['reg_fraction', 'Fraction régurgitée', '%', 0, 100],
  ['la_mean', 'POG moyenne', 'mmHg', 1], ['pap_mean', 'PAP moyenne', 'mmHg', 1], ['cvp', 'PVC', 'mmHg', 1]];
const GASES = [['ph', 'pH artériel', '', 3], ['pco2', 'PaCO₂', 'mmHg', 1], ['pao2', 'PaO₂', 'mmHg', 0], ['hco3', 'HCO₃⁻', 'mmol/L', 1],
  ['be', 'Excès de base', 'mmol/L', 1], ['sao2', 'SaO₂', '%', 1], ['lactate', 'Lactate', 'mmol/L', 2], ['ph_v', 'pH veineux mêlé', '', 3],
  ['pco2_v', 'PvCO₂', 'mmHg', 1], ['po2_v', 'PvO₂', 'mmHg', 0]];
const CHEM = [['na', 'Na⁺', 'mmol/L', 0], ['k', 'K⁺', 'mmol/L', 1], ['cl', 'Cl⁻', 'mmol/L', 0], ['anion_gap', 'Trou anionique', 'mmol/L', 1],
  ['glucose', 'Glucose', 'mmol/L', 1], ['creat', 'Créatinine', 'µmol/L', 0], ['urea', 'Urée', 'mmol/L', 1], ['phos', 'Phosphate', 'mmol/L', 2],
  ['alb', 'Albumine', 'g/L', 1], ['hct', 'Hématocrite', '%', 0]];
// périphérie (bloc periph) : thermorégulation, pupille, digestif et sécrétions, SNC
const PERIPH = [['temp_c', 'Température centrale', '°C', 1], ['shiver', 'Frisson (VO₂ ajoutée)', '×', 2],
  ['panting', 'Halètement', '×', 2], ['pupil_mm', 'Diamètre pupillaire', 'mm', 1],
  ['plr_pct', 'Réflexe photomoteur', '%', 0], ['gi_motility', 'Motilité digestive', '%', 0, 100],
  ['les_mmhg', 'Sphincter œsophagien', 'mmHg', 0], ['salivation', 'Salivation', '%', 0, 100],
  ['sedation_score', 'Score de sédation', '/21', 1]];
const INDICATORS = [...HEMO, ['sv', "Volume d'éjection", 'mL', 1], ['lv_edp', 'PTDVG', 'mmHg', 1], ['edv', 'VTDVG', 'mL', 1],
  ...GASES.slice(0, 7), ['rr', 'FR', '/min', 0], ['po2_v', 'PvO₂', 'mmHg', 0], ...CHEM, ['gfr', 'DFG', 'mL/min', 1],
  ['do2_kg', 'DO₂', 'mL/min/kg', 1], ['er', "Extraction d'O₂", '%', 0, 100], ['s_tone', 'Tonus sympathique', '', 2], ['v_tone', 'Tonus vagal', '', 2],
  ...PERIPH];

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
  $('#disease').onchange = () => { fillStages(); markDirty(); };
  $('#stage').onchange = markDirty;
  $('#bw').oninput = markDirty;
  // Entrée dans le formulaire = Évaluer ; le libellé « Calcul… » s'affiche avant le calcul
  $('#case-form').onsubmit = (e) => { e.preventDefault(); busy($('#evaluate'), () => { S.treat.r = null; evaluate(); if (S.treat.regimens.length) resimulate(); }); };
  $('#copy-link').onclick = copyLink;
  wireTabs();
  // redimensionnement : un seul re-rendu par image, et seulement si la largeur change (sur mobile,
  // la barre d'adresse qui se replie modifie la hauteur à chaque défilement)
  let lastW = window.innerWidth, pend = 0;
  window.addEventListener('resize', () => {
    if (pend || window.innerWidth === lastW) return;
    pend = requestAnimationFrame(() => { pend = 0; lastW = window.innerWidth; render(); });
  });
  window.addEventListener('keydown', onKey);
  fillStages();
  applyHash();
  evaluate();
  if (S.treat.autoSim) { S.treat.autoSim = false; resimulate(); }
}

const TABS = ['patient', 'respiration', 'model', 'auscultation', 'heart', 'treatment', 'anesthesia'];

/** Onglets : clic, flèches gauche/droite, Début/Fin (motif ARIA « tablist »). */
function wireTabs() {
  const tabs = [...document.querySelectorAll('#tabs [role=tab]')];
  for (const b of tabs) b.onclick = () => selectTab(b.dataset.tab);
  $('#tabs').onkeydown = (e) => {
    const i = tabs.indexOf(document.activeElement);
    if (i < 0) return;
    const j = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (j === undefined) return;
    e.preventDefault(); e.stopPropagation();
    const b = tabs[(j + tabs.length) % tabs.length];
    b.focus(); selectTab(b.dataset.tab);
  };
}
function selectTab(tab) { if (tab !== S.tab) { S.tab = tab; render(); } }

/** Raccourcis globaux (hors champs de saisie) : espace = écouter/arrêter dans l'onglet Auscultation. */
function onKey(e) {
  if (e.target.closest && e.target.closest('input, select, textarea, button, [role=tab]')) return;
  if (S.tab === 'auscultation' && e.key === ' ' && S.ausc.r) { e.preventDefault(); if (S.ausc.playing) stopAusc(); else play(); }
}

/** Exécute un calcul synchrone après avoir affiché « Calcul… » sur le bouton. */
function busy(btn, f) {
  const old = btn.textContent;
  btn.textContent = 'Calcul…'; btn.disabled = true; document.body.classList.add('busy');
  requestAnimationFrame(() => setTimeout(() => {
    try { f(); } finally { btn.disabled = false; document.body.classList.remove('busy'); if (btn.textContent === 'Calcul…') btn.textContent = old; markDirty(); }
  }, 0));
}

/** Adresse décrivant le cas affiché (mêmes paramètres que applyHash). */
function caseHash() {
  const q = new URLSearchParams(), T = S.treat;
  q.set('bw', S.inputs.bw);
  if (S.inputs.disease) { q.set('disease', S.inputs.disease); if (S.inputs.stage) q.set('stage', S.inputs.stage); }
  q.set('tab', S.tab);
  if (T.regimens.length) {
    q.set('plan', T.regimens.map((r) => [r.drug, r.mgPerKg, r.route, r.intervalH, r.count, r.startH].join(':')).join(','));
    q.set('days', T.days);
    if (T.cursor) q.set('t', +T.cursor.toFixed(2));
    if (T.r || T.pending) q.set('sim', '1');
  }
  if (S.ausc.window !== 8) q.set('window', S.ausc.window);
  if (S.heart.speed !== 0.25) q.set('speed', S.heart.speed);
  return '#' + q.toString().replace(/%3A/gi, ':').replace(/%2C/gi, ',');
}

/** L'adresse suit le cas affiché : un rechargement le restitue, et elle se partage telle quelle. */
function syncHash() {
  const h = caseHash();
  if (location.hash !== h) history.replaceState(null, '', h);
}

async function copyLink() {
  syncHash();
  const b = $('#copy-link'), old = 'Copier le lien du cas';
  try { await navigator.clipboard.writeText(location.href); b.textContent = 'Lien copié ✓'; } catch (e) { prompt('Lien du cas :', location.href); }
  setTimeout(() => { b.textContent = old; }, 1800);
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
  if (TABS.includes(q.get('tab'))) S.tab = q.get('tab');
  if (q.get('plan')) {
    S.treat.regimens = q.get('plan').split(',').map((x) => {
      const [drug, mg, route, iv, n, st] = x.split(':');
      return { drug, mgPerKg: +mg, route: route === 'iv' ? 'iv' : 'po', intervalH: +iv, count: Math.round(+n), startH: +(st || 0), overrides: {} };
    }).filter((r) => S.ctl.catalog.drugs.some((d) => d.id === r.drug) && r.mgPerKg >= 0 && r.intervalH > 0 && r.count >= 1 && r.count <= 200);
  }
  if (q.get('days')) S.treat.days = Math.min(14, Math.max(1, Math.round(+q.get('days')) || 3));
  if (q.get('t')) S.treat.cursor = +q.get('t');
  S.treat.autoSim = q.get('sim') === '1' && S.treat.regimens.length > 0;
  if (q.get('window')) S.ausc.window = +q.get('window');
  if (q.get('speed')) S.heart.speed = +q.get('speed');
  if (q.get('ht')) { S.heart.t = +q.get('ht'); S.heart.playing = false; }
}

function fillStages() {
  const d = S.ctl.catalog.diseases.find((x) => x.id === $('#disease').value);
  $('#stage-wrap').hidden = !d;
  $('#stage').innerHTML = d ? d.stages.map((s) => `<option value="${s.name}">${s.name} (s ${s.lo}–${s.hi})</option>`).join('') : '';
}

/** Formulaire modifié depuis la dernière évaluation : l'affichage ne correspond plus au formulaire. */
function isDirty() {
  return (parseFloat($('#bw').value) || 20) !== S.inputs.bw || ($('#disease').value || null) !== S.inputs.disease
    || ($('#disease').value && ($('#stage').value || null) !== S.inputs.stage);
}
function markDirty() {
  const d = S.res && isDirty();
  $('#evaluate').classList.toggle('dirty', !!d);
  $('#evaluate').textContent = d ? 'Évaluer ce nouveau patient' : 'Évaluer';
  if (S.res) $('#case-label').innerHTML = esc(caseLabel()) + (d ? ' <span class="stale">· formulaire modifié, non évalué</span>' : '');
}

function evaluate() {
  S.inputs = { ...S.inputs, bw: parseFloat($('#bw').value) || 20, disease: $('#disease').value || null, stage: $('#stage').value || null };
  try {
    $('#error').hidden = true;
    S.res = S.ctl.evaluate(S.inputs);
    stopAusc(false); S.ausc.r = null; S.ausc.played = false; // nouvelle auscultation pour ce patient
    S.heart.r = null;
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
  for (const b of document.querySelectorAll('#tabs button')) {
    const on = b.dataset.tab === S.tab;
    b.classList.toggle('on', on); b.setAttribute('aria-selected', on ? 'true' : 'false'); b.tabIndex = on ? 0 : -1;
  }
  $('#view').setAttribute('aria-labelledby', 'tab-' + S.tab);
  markDirty();
  syncHash();
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
  ({ patient: renderPatient, respiration: renderRespiration, model: renderModel, auscultation: renderAuscultation, heart: renderHeart, treatment: renderTreatment,
    anesthesia: (el) => renderAnesthesia(el, { ctl: S.ctl, inputs: S.inputs }) })[S.tab](v);
}

// ------------------------------------------------------------------ Issue (décompensation, décès)
const MECH_FR = {
  acidemia_depression: 'dépression myocardique et vasoplégie par acidémie',
  hypoxic_depression: 'dépression myocardique par hypoxie tissulaire',
  coronary_ischemia: 'ischémie myocardique (pression de perfusion coronaire basse)',
  hypokalemic_weakness: 'faiblesse des muscles respiratoires (hypokaliémie)',
  atrial_standstill: "arrêt atrial hyperkaliémique (pas d'onde P ni de contraction atriale)",
  conduction_block: 'trouble de conduction (FC ventriculaire inférieure à la FC sinusale)',
  anaerobic_metabolism: "métabolisme anaérobie (extraction d'O₂ élevée, lactate)",
  pulmonary_edema: 'œdème pulmonaire (POG élevée)',
  renal_hypoperfusion: 'hypoperfusion rénale (PAM basse)',
  baroreflex_exhausted: 'baroréflexe épuisé (tonus sympathique maximal)',
};
const DEATH_FR = {
  cardiac_arrest: 'arrêt cardiaque', circulatory_collapse: 'effondrement circulatoire', irreversible_shock: 'choc irréversible',
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
  v.innerHTML = vitals(st) + card(`ECG, dérivation II (${Math.round(sg.ecg.length / sg.ecg_fs)} s, ${fmt(st.hr, 0)} /min ; échelle fixe −1,5 à +2,5 mV)`, canvas('c-ecg', 150))
    + card(`Pressions (mmHg) : aorte, VG, OG, sur le même temps que l'ECG (simulées battement par battement)`, canvas('c-pr', 170) + legend([['Aorte', COLORS.ao], ['VG', COLORS.lv], ['OG', COLORS.la]]) + `<div class="note">PCG : ${sg.pcg_features.grade ? `souffle ${sg.pcg_features.murmur}, grade ${sg.pcg_features.grade}/6` : 'pas de souffle'}</div>`)
    + `<div class="grid3">${card('Hémodynamique', valuesTable(HEMO, st))}${card('Gaz du sang', valuesTable(GASES, st))}${card('Biochimie', valuesTable(CHEM, st))}</div>`
    + card('Périphérie (thermorégulation, pupille, digestif, SNC)', valuesTable(PERIPH, st))
    + card('Signes émergents', st.emergent_signs.length ? st.emergent_signs.map((s) => `<div class="sign">• ${esc(s.label_fr)}<div class="note">&nbsp;&nbsp;${esc(s.source)}</div></div>`).join('') : '<div class="note">Aucun signe dérivé au-delà des seuils sourcés.</div>');
  const dur = sg.ecg.length / sg.ecg_fs;
  timeChart($('#c-ecg'), { series: [{ y: sg.ecg, dt: 1 / sg.ecg_fs, color: COLORS.ecg }], x0: 0, x1: dur, paper: true, yMin: ECG_MIN, yMax: ECG_MAX });
  // pressions alignées sur l'ECG : même axe de temps [0, durée) ; repli sur deux battements
  const pa = sg.pressures_aligned;
  if (pa) {
    timeChart($('#c-pr'), { series: [pa.ao, pa.lv, pa.la].map((y, i) => ({ y, dt: 1 / pa.fs, color: [COLORS.ao, COLORS.lv, COLORS.la][i] })), x0: 0, x1: dur, yMin: 0 });
  } else {
    const two = (a) => [...a, ...a];
    timeChart($('#c-pr'), { series: [two(pr.ao), two(pr.lv), two(pr.la)].map((y, i) => ({ y, dt: pr.dt, color: [COLORS.ao, COLORS.lv, COLORS.la][i] })), x0: 0, x1: pr.dt * pr.ao.length * 2, yMin: 0 });
  }
}

/** Constantes clés en tête de l'onglet Patient (valeurs du modèle, sans intervalle de référence). */
function vitals(st) {
  const rr = st.rr ?? (st.signals && st.signals.respiration && st.signals.respiration.rr_per_min);
  const tile = (k, v, u) => `<div class="vital"><span class="k">${k}</span><span class="v">${v}<small>${u}</small></span></div>`;
  return `<div class="vitals">${tile('FC', fmt(st.hr, 0), '/min')}${tile('PAS / PAD', `${fmt(st.sap, 0)}/${fmt(st.dap, 0)}`, 'mmHg')}`
    + `${tile('PAM', fmt(st.map, 0), 'mmHg')}${tile('FR', fmt(rr, 0), '/min')}${tile('SaO₂', fmt(st.sao2, 1), '%')}`
    + `${tile('Débit cardiaque', fmt(st.co, 2), 'L/min')}${tile('Lactate', fmt(st.lactate, 2), 'mmol/L')}</div>`;
}

// ------------------------------------------------------------------ Respiration
function renderRespiration(v) {
  if (!S.res.st.signals) { v.innerHTML = deadView('respiration'); return; }
  const rs = S.res.st.signals.respiration, dt = 1 / rs.fs, dur = rs.volume_mL.length * dt;
  const avg = (a) => a.reduce((x, y) => x + y, 0) / a.length;
  v.innerHTML = `<div class="summary">FR ${fmt(rs.rr_per_min, 0)} /min · volume courant alvéolaire ${fmt(rs.vt_alv_mL, 0)} mL (VA/FR ; l'espace mort n'est pas modélisé) · inspiration sur 40 % du cycle</div>`
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
  ['left_heart', 'systemic_circulation', 'éjection aortique', 0, 0.16], ['systemic_circulation', 'kidney', 'PAM, DC → perfusion'], ['autonomic', 'kidney', 'SNS, RAAS → DFG (cardio-rénal)', 0, 0.18], ['oxygen_transport', 'systemic_circulation', 'anémie → viscosité, RVS', 0, -0.18],
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
    return { d: `<path d="M${s[0]},${s[1]} Q${c[0]},${c[1]} ${e[0]},${e[1]}" fill="none" stroke="${color}" stroke-width="1.6" ${dash ? 'stroke-dasharray="7 6"' : ''} marker-end="url(#ah-${k})" ${bidir ? `marker-start="url(#ah-${k})"` : ''}/>`, curve: [s, c, e] };
  };
  const curves = [], toLabel = [];
  for (const [f, t, lab, bidir, bend = 0] of EDGES) {
    const p = path(C(byId[f]), C(byId[t]), bend, '#8a99a6', 'g', false, bidir);
    svg += p.d; curves.push(p.curve); toLabel.push([p.curve, lab, curves.length - 1]);
  }
  for (const b of dBlocks) if (byId[b]) { const p = path(C(byId.disease), C(byId[b]), 0.08, COLORS.ao, 'r', true); svg += p.d; curves.push(p.curve); }
  for (const b of gBlocks) if (byId[b]) { const p = path(C(byId.drugs), C(byId[b]), -0.08, COLORS.lv, 'b', true); svg += p.d; curves.push(p.curve); }
  const boxes = NODES.map((n) => { const [x, y] = C(n); return [x - nw / 2 - 2, y - nh / 2 - 2, x + nw / 2 + 2, y + nh / 2 + 2]; });
  const mctx = document.createElement('canvas').getContext('2d'); mctx.font = '10px system-ui, sans-serif';
  const labels = placeEdgeLabels(toLabel, curves, boxes, (t) => mctx.measureText(t).width, W, H)
    .map(([x, y, lab]) => `<text x="${x}" y="${y + 3.5}" font-size="10" fill="${COLORS.muted}" text-anchor="middle" paint-order="stroke" stroke="white" stroke-width="4">${esc(lab)}</text>`).join('');
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
    const clickable = cat.blocks.some((b) => b.id === id);
    svg += `<g data-node="${id}" style="cursor:${clickable ? 'pointer' : 'default'}"${clickable ? ` tabindex="0" role="button" aria-pressed="${sel}" aria-label="${esc(label(id))}"` : ''}><rect x="${x - nw / 2}" y="${y - nh / 2}" width="${nw}" height="${nh}" rx="10" fill="${fill}" stroke="${sel ? COLORS.ink : stroke}" stroke-width="${sel ? 3 : 1.5}"/>`
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
    ? `<div class="scroll-x"><table class="values params"><tr><th>Paramètre</th><th>Bloc</th><th class="v">Sain → patient</th><th>Source</th></tr>${r.changes.map((c) => `<tr><td>${c.param}</td><td>${esc(c.blocks.map((b) => (cat.blocks.find((x) => x.id === b) || {}).label_fr).join(', '))}</td><td class="v">${num(c.healthy)} → ${num(c.patient)}</td><td class="src">${esc(c.sources.join(', '))}</td></tr>`).join('')}</table></div>`
    : '<div class="note">Aucun : patient sain.</div>');
  v.innerHTML = `<div class="legend"><span><i style="background:${COLORS.ao}"></i>modifié par la maladie</span><span><i style="background:${COLORS.lv}"></i>modifié par un médicament</span><span>Cliquer un bloc pour ses paramètres.</span></div>` + svg + detail + table;
  const pick = (id) => { if (cat.blocks.some((b) => b.id === id)) { S.model.selected = S.model.selected === id ? null : id; render(); const g = v.querySelector(`g[data-node="${id}"]`); if (g) g.focus(); } };
  for (const g of v.querySelectorAll('g[data-node]')) {
    g.onclick = () => pick(g.dataset.node);
    g.onkeydown = (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pick(g.dataset.node); } };
  }
}

/** Point et tangente d'une courbe quadratique [s, c, e] au paramètre t. */
function quad([s, c, e], t) {
  const u = 1 - t;
  return { p: [u * u * s[0] + 2 * u * t * c[0] + t * t * e[0], u * u * s[1] + 2 * u * t * c[1] + t * t * e[1]],
    d: [2 * u * (c[0] - s[0]) + 2 * t * (e[0] - c[0]), 2 * u * (c[1] - s[1]) + 2 * t * (e[1] - c[1])] };
}

/**
 * Place les étiquettes des liens sans chevauchement : pour chaque lien, positions candidates le
 * long de la courbe (milieu d'abord) et de part et d'autre ; une position qui touche un bloc, une
 * étiquette déjà posée ou le bord est exclue ; parmi les autres, la moins traversée par les autres
 * liens et la plus proche du milieu. Sans position libre, l'étiquette est omise (couplage détaillé
 * au clic sur le bloc). Renvoie [[x, y, texte]] (centre de l'étiquette).
 * Même algorithme que placeEdgeLabels dans kmp-demo (Views.kt).
 */
function placeEdgeLabels(items, curves, boxes, measure, W, H) {
  const samples = curves.map((cv) => Array.from({ length: 33 }, (_, i) => quad(cv, i / 32).p));
  const hit = (a, b) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
  const placed = [], out = [];
  const TS = [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74, 0.18, 0.82];
  // positions candidates hors des blocs et du bord, pour chaque étiquette
  const cands = items.map(([cv, lab]) => {
    const w = measure(lab) + 8, h = 13, list = [];
    for (const t of TS) for (const off of [0, 11, -11]) {
      const { p, d } = quad(cv, t), L = Math.hypot(d[0], d[1]) || 1;
      const x = p[0] - (d[1] / L) * off, y = p[1] + (d[0] / L) * off;
      const r = [x - w / 2, y - h / 2, x + w / 2, y + h / 2];
      if (r[0] < 0 || r[1] < 0 || r[2] > W || r[3] > H || boxes.some((b) => hit(r, b))) continue;
      list.push({ t, off, x, y, r });
    }
    return list;
  });
  // les étiquettes les plus contraintes (peu de positions libres) sont posées d'abord
  const order = items.map((_, i) => i).sort((i, j) => cands[i].length - cands[j].length || i - j);
  for (const i of order) {
    const [, lab, own] = items[i];
    let best = null;
    for (const { t, off, x, y, r } of cands[i]) {
      if (placed.some((b) => hit(r, b))) continue;
      let cross = 0;
      samples.forEach((pts, j) => { if (j !== own) for (const q of pts) if (q[0] > r[0] && q[0] < r[2] && q[1] > r[1] && q[1] < r[3]) cross++; });
      const cost = 3 * cross + 20 * Math.abs(t - 0.5) + 0.3 * Math.abs(off);
      if (!best || cost < best.cost) best = { cost, x, y, r };
    }
    if (best) { placed.push(best.r); out.push([best.x, best.y, lab]); }
  }
  return out;
}

function num(x) { const a = Math.abs(x); return fmt(x, a === 0 ? 0 : a >= 100 ? 1 : a >= 10 ? 2 : a >= 1 ? 3 : a >= 0.01 ? 4 : 6); }

// ------------------------------------------------------------------ Auscultation
function renderAuscultation(v) {
  const A = S.ausc;
  if (isDead()) { v.innerHTML = deadView('bruit cardiaque : silence à l\'auscultation'); return; }
  if (!A.r) {
    try {
      A.r = S.ctl.auscultate(S.inputs, A.hr, A.site); A.spec = computeSpectrogram(A.r.samples, A.r.fs); A.cursor = null; A.start = 0; A.window = Math.min(A.window, A.r.duration_s);
      // échelle des pressions fixée une fois (pas de Math.max(...) sur de grands tableaux à chaque image)
      let hi = 0; for (const a of [A.r.pressures.ao, A.r.pressures.lv]) for (const x of a) if (x > hi) hi = x;
      A.prMax = hi * 1.05;
    }
    catch (e) { showError(e); return; }
  }
  const r = A.r, dur = r.duration_s;
  const chip = (lab, on, act) => `<button class="${on ? 'on' : ''}" data-act="${act}">${lab}</button>`;
  v.innerHTML = `<div class="toolbar"><div class="row" style="gap:18px">
      <div class="group"><span class="lab">FC</span>${chip('Patient', A.hr === null, 'hr:')}${chip('Repos 72', A.hr === 72, 'hr:72')}${chip('90', A.hr === 90, 'hr:90')}</div>
      <div class="group"><span class="lab">Site</span>${chip('Base', A.site === 'base', 'site:base')}${chip('Apex', A.site === 'apex', 'site:apex')}</div></div>
    <div class="row" style="gap:18px"><button class="primary" id="play" title="Espace">${A.playing ? '■ Arrêter' : A.played ? '↻ Rejouer' : '▶ Écouter'}</button>
      <label class="inline"><input type="checkbox" id="loop" ${A.loop ? 'checked' : ''}> en boucle</label>
      <label class="inline"><input type="checkbox" id="zoomonly" ${A.zoomOnly ? 'checked' : ''}> fenêtre affichée seulement</label>
      <label class="inline">Volume <input id="vol" type="range" min="1" max="8" step="0.5" value="${A.gain}" style="width:120px"> <b id="voll">×${fmt(A.gain, 1)}</b></label></div>
    <div class="summary">FC ${fmt(r.hr, 0)} /min · ${r.beats.length} battements · ${r.grade ? `souffle ${r.murmur}, grade ${r.grade}/6 (indicatif)` : 'pas de souffle'}${r.s3 ? ' · B3' : ''} · site ${r.site}</div>
    <div class="sliders"><label><span>Fenêtre : <b id="wl">${fmt(A.window, 1)}</b> s</span><input id="win" type="range" min="0.5" max="${dur}" step="0.1" value="${A.window}"></label>
      <label><span>Début : <b id="sl">${fmt(A.start, 2)}</b> s</span><input id="start" type="range" min="0" max="${Math.max(0.001, dur - A.window)}" step="0.01" value="${A.start}"></label></div></div>`
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
  $('#vol').oninput = (e) => {
    A.gain = +e.target.value; $('#voll').textContent = '×' + fmt(A.gain, 1);
    if (A.gainNode) A.gainNode.gain.value = A.gain;
  };
  drawAusc();
}

function drawAusc() {
  const A = S.ausc, r = A.r, x0 = A.start, x1 = A.start + A.window;
  // pendant la lecture, la fenêtre bouge peu : les fonds sont mis en cache, seul le curseur est retracé
  const cacheKey = `${x0}:${x1}`;
  timeChart($('#a-pcg'), { series: [{ y: r.samples, dt: 1 / r.fs, color: COLORS.accent }], x0, x1, yMin: -1, yMax: 1, cursor: A.cursor, cacheKey });
  spectrogram($('#a-spec'), A.spec, x0, x1, A.cursor);
  timeChart($('#a-ecg'), { series: [{ y: r.ecg, dt: 1 / r.ecg_fs, color: COLORS.ecg }], x0, x1, cursor: A.cursor, yMin: ECG_MIN, yMax: ECG_MAX, cacheKey });
  const p = r.pressures;
  timeChart($('#a-pr'), { series: [p.ao, p.lv, p.la].map((y, i) => ({ y, dt: 1 / p.fs, color: [COLORS.ao, COLORS.lv, COLORS.la][i] })), x0, x1, yMin: 0, yMax: A.prMax, cursor: A.cursor, cacheKey });
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
  // amplification réglable ; limiteur de crête seulement (seuil −1 dBFS, ratio élevé, attaque
  // et relâchement rapides) : sous le seuil, le signal n'est pas modifié (timbre conservé)
  const src = ctx.createBufferSource(); src.buffer = buf;
  const g = ctx.createGain(); g.gain.value = A.gain; A.gainNode = g;
  const lim = ctx.createDynamicsCompressor();
  lim.threshold.value = -1; lim.knee.value = 0; lim.ratio.value = 20; lim.attack.value = 0.001; lim.release.value = 0.05;
  src.connect(g); g.connect(lim); lim.connect(ctx.destination);
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

// ------------------------------------------------------------------ Cœur animé
function renderHeart(v) {
  const H = S.heart;
  if (isDead()) { v.innerHTML = deadView('contraction ni d\'activité électrique'); return; }
  if (!H.r) {
    try { H.r = S.ctl.heart(S.inputs); } catch (e) { showError(e); return; }
  }
  mountHeart(v, H.r, H, { card, canvas, legend, fmt, ecgMin: ECG_MIN, ecgMax: ECG_MAX });
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
        ${esc(dname(r.drug))} : <button data-step="${i}:-1" title="−10 %">−</button><input data-mg="${i}" type="number" step="any" min="0" style="width:72px" value="${r.mgPerKg}"><button data-step="${i}:1" title="+10 %">+</button> mg/kg ${r.route === 'po' ? 'per os' : 'IV'}, toutes les ${r.intervalH} h × ${r.count} (de ${dayLabel(r.startH)} à ${dayLabel(r.startH + (r.count - 1) * r.intervalH)})${om ? ` ; ${om} omise(s)` : ''}
        <button data-ed="${i}">Modifier</button><button data-op="${i}">${T.open === i ? 'Masquer les prises' : 'Prises individuelles'}</button><button data-rm="${i}">Retirer</button></div>`;
      if (T.open === i) {
        s += '<div class="note">Dose de chaque prise (mg/kg) ; 0 = prise omise</div><div class="row">'
          + Array.from({ length: r.count }, (_, k) => `<label>${dayLabel(r.startH + k * r.intervalH)}<input data-dose="${i}:${k}" type="number" step="any" min="0" style="width:84px" value="${r.overrides && r.overrides[k] !== undefined ? r.overrides[k] : r.mgPerKg}"></label>`).join('') + '</div>';
      }
      return s;
    }).join('')
    + `<div class="row">Durée simulée : <b id="t-dl">${T.days}</b> j <input id="t-days" type="range" min="1" max="14" step="1" value="${T.days}" style="width:240px">
       <span class="note">${T.regimens.length ? 'simulation mise à jour à chaque changement' : 'ajouter un schéma pour simuler'}</span></div>`
    + (T.pending ? `<div class="sim-status" role="status"><span class="spinner"></span>Simulation en cours (${T.days} j${S.inputs.disease === 'mmvd' ? ', plusieurs secondes en MMVD' : ''})… L'interface reste utilisable ; un nouveau changement relance le calcul.<button id="t-abort">Interrompre</button></div>` : ''));
  const r = T.r;
  if (r) {
    html += `<div class="treat-results${T.pending ? ' stale-res' : ''}">`;
    const h = r.plan.horizonH, i = nearest(r.t, T.cursor);
    html += card(`Instant d'examen : ${dayLabel(T.cursor)} (t = ${fmt(T.cursor, 1)} h)`, `<div class="row">
        ${T.ref ? `<button id="t-trough">◀ creux</button><button id="t-peak">pic ▶</button>
        <select id="t-ref">${Object.keys(r.conc_mg_L).map((d) => `<option value="${d}" ${d === T.ref ? 'selected' : ''}>repère : ${esc(dname(d))}</option>`).join('')}</select>` : ''}
        <button class="primary" id="t-exam" ${T.pending ? 'disabled title="simulation en cours"' : ''}>Examiner le patient à cet instant</button></div>
        <input id="t-cur" type="range" min="0" max="${h}" step="0.05" value="${T.cursor}" aria-label="Instant d'examen (h)">
        <div class="note">Astuce : cliquer ou glisser sur une courbe déplace l'instant d'examen ; ◀ creux / pic ▶ sautent d'un extremum au suivant.</div>
        <div class="note">${Object.keys(r.conc_mg_L).map((d) => `${esc(dname(d))} : ${fmt(r.conc_mg_L[d][i], 4)} mg/L, E = ${fmt(r.effect[d][i], 2)}`).join(' &nbsp; ')}</div>
        ${outcomeNote(r)}`);
    html += card('Un chien suivi dans le temps (variabilité naturelle)', mcPanel(r));
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
      html += card(`${ind[1]}${ind[2] ? ` (${ind[2]})` : ''} : ${fmt(r.states[k][j] * (ind[4] || 1), Math.max(ind[3], 2))} à ${dayLabel(r.state_t[j])}${mcBandNote(k)}`, canvas('ti-' + k, 90));
    }
    html += '</div>';
  }
  const focus = focusKey();
  v.innerHTML = html;
  restoreFocus(v, focus);
  // événements
  const setRegimen = (i, reg) => { T.regimens = T.regimens.map((x, j) => (j === i ? reg : x)); resimulate(); };
  const setDose = (i, val) => {
    if (!(val >= 0)) return;
    const old = T.regimens[i];
    // les prises individuelles qui suivaient la dose nominale la suivent ; les autres restent modifiées
    const overrides = Object.fromEntries(Object.entries(old.overrides || {}).filter(([, x]) => x !== val));
    setRegimen(i, { ...old, mgPerKg: val, overrides });
  };
  $('#t-add').onclick = () => {
    const reg = { drug: $('#t-drug').value, mgPerKg: +$('#t-dose').value, route: $('#t-route').value, intervalH: +$('#t-int').value, count: Math.round(+$('#t-n').value), startH: +$('#t-start').value || 0, overrides: {} };
    if (!(reg.mgPerKg >= 0 && reg.intervalH > 0 && reg.count >= 1 && reg.count <= 200)) return;
    if (T.editing === null) T.regimens = [...T.regimens, reg];
    else { const old = T.regimens[T.editing]; reg.overrides = Object.fromEntries(Object.entries(old.overrides || {}).filter(([k]) => +k < reg.count)); T.regimens = T.regimens.map((x, j) => (j === T.editing ? reg : x)); T.editing = null; }
    resimulate();
  };
  if ($('#t-cancel')) $('#t-cancel').onclick = () => { T.editing = null; render(); };
  for (const b of v.querySelectorAll('[data-ed]')) b.onclick = () => { T.editing = +b.dataset.ed; render(); };
  for (const b of v.querySelectorAll('[data-op]')) b.onclick = () => { T.open = T.open === +b.dataset.op ? null : +b.dataset.op; render(); };
  for (const b of v.querySelectorAll('[data-rm]')) b.onclick = () => { T.regimens = T.regimens.filter((_, j) => j !== +b.dataset.rm); T.editing = null; T.open = null; resimulate(); };
  for (const inp of v.querySelectorAll('[data-mg]')) inp.onchange = () => setDose(+inp.dataset.mg, parseFloat(inp.value));
  for (const b of v.querySelectorAll('[data-step]')) b.onclick = () => {
    const [i, dir] = b.dataset.step.split(':').map(Number), x = T.regimens[i].mgPerKg;
    setDose(i, x > 0 ? +(x * (dir > 0 ? 1.1 : 1 / 1.1)).toPrecision(3) : 0);
  };
  for (const inp of v.querySelectorAll('[data-dose]')) inp.onchange = () => {
    const [i, k] = inp.dataset.dose.split(':').map(Number), val = parseFloat(inp.value);
    if (!(val >= 0)) return;
    const reg = { ...T.regimens[i], overrides: { ...(T.regimens[i].overrides || {}) } };
    if (val === reg.mgPerKg) delete reg.overrides[k]; else reg.overrides[k] = val;
    setRegimen(i, reg);
  };
  $('#t-days').oninput = (e) => { T.days = +e.target.value; $('#t-dl').textContent = T.days; };
  $('#t-days').onchange = () => resimulate();
  if ($('#t-abort')) $('#t-abort').onclick = () => { S.bg.cancel(); };
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
  bindMc(v);
  for (const b of v.querySelectorAll('[data-ind]')) b.onclick = () => { const k = b.dataset.ind; T.shown = T.shown.includes(k) ? T.shown.filter((x) => x !== k) : [...T.shown, k]; render(); };
  // cliquer ou glisser sur une courbe : instant d'examen (tracé du curseur seul pendant le glissé)
  for (const c of v.querySelectorAll('canvas')) {
    const at = (e) => { const rect = c.getBoundingClientRect(), left = 44; return Math.max(0, Math.min(r.plan.horizonH, ((e.clientX - rect.left - left) / (rect.width - left)) * r.plan.horizonH)); };
    c.style.cursor = 'col-resize'; c.style.touchAction = 'pan-y';
    c.onpointerdown = (e) => {
      if (e.button !== 0) return;
      c.setPointerCapture(e.pointerId);
      T.cursor = at(e); $('#t-cur').value = T.cursor; drawTreatment();
      c.onpointermove = (m) => { T.cursor = at(m); $('#t-cur').value = T.cursor; drawTreatment(); };
      c.onpointerup = c.onpointercancel = () => { c.onpointermove = null; c.onpointerup = c.onpointercancel = null; render(); };
    };
  }
  drawTreatment();
}

/**
 * Resimule le traitement après tout changement de schéma ou de durée (plus de bouton « Simuler »).
 * Si le patient est déjà examiné sous ce traitement, l'examen suit le nouveau schéma au même instant.
 * Différé d'une tâche : sur Tab, « change » part avant que le focus n'atteigne le champ suivant,
 * qu'on retrouve ainsi après le re-rendu.
 */
function resimulate() {
  clearTimeout(S.treat.timer);
  S.treat.timer = setTimeout(resimulateNow, 0);
}
/**
 * Le calcul tourne dans un Web Worker (plusieurs secondes en MMVD) : l'interface reste utilisable,
 * les anciens résultats restent affichés (grisés) jusqu'au nouveau, et un changement pendant le
 * calcul annule celui-ci au profit du plus récent.
 */
async function resimulateNow() {
  const T = S.treat;
  if (!T.regimens.length) { S.bg.cancel(); T.pending = false; T.r = null; render(); return; }
  T.pending = true;
  render();
  try {
    // instantané des entrées : le calcul porte sur le patient et le schéma de cet instant
    const r = await S.bg.run('treatment', { ...S.inputs }, { regimens: T.regimens, horizonH: T.days * 24 });
    T.pending = false;
    T.r = r;
    T.cursor = Math.min(T.cursor, T.days * 24);
    if (!T.ref || !(T.ref in T.r.conc_mg_L)) T.ref = Object.keys(T.r.conc_mg_L)[0];
  } catch (e) {
    if (e instanceof Superseded) { if (!S.bg.busy) { T.pending = false; render(); } return; }
    T.pending = false; showError(e); render(); return;
  }
  $('#error').hidden = true;
  if (S.inputs.plan && S.inputs.atH !== null) {
    const r = T.r, dead = r.death_t_h !== null && r.death_t_h !== undefined;
    const cause = dead ? r.outcomes.find((x) => x.state === 'dead').cause : null;
    S.inputs.plan = r.plan; S.inputs.atH = Math.min(S.inputs.atH, r.plan.horizonH);
    S.inputs.death = dead ? { t_h: r.death_t_h, cause, label_fr: DEATH_FR[cause] || 'décès' } : null;
    evaluate();
  } else render();
}

/** Champ qui a le focus (identifié par id ou data-*), pour le retrouver après un re-rendu. */
function focusKey() {
  const f = document.activeElement;
  if (!f || f === document.body) return null;
  for (const a of ['id', 'data-mg', 'data-dose', 'data-step']) if (f.hasAttribute(a)) return `[${a}="${f.getAttribute(a)}"]`;
  return null;
}
function restoreFocus(v, key) {
  const el = key && v.querySelector(key);
  if (el) { el.focus(); if (el.select && el.type === 'number') el.select(); }
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
  const T = S.treat, r = T.r;
  if (!r) return;
  const h = r.plan.horizonH;
  const marks = r.plan.regimens.flatMap((reg, i) => planDoses({ regimens: [reg] }).map((d) => ({ t: d.t_h, color: COLORS.drugs[i % 5] })));
  const col = (d) => COLORS.drugs[Math.max(0, r.plan.regimens.findIndex((x) => x.drug === d)) % 5];
  // fonds en cache : déplacer le curseur ne retrace que le curseur (les canvas sont recréés à chaque rendu)
  const base = { x0: 0, x1: h, cursor: T.cursor, marks, ticks: 'treatment', bands: outcomeBands(r), cacheKey: 'treatment' };
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
    if (!c || !r.states[k]) continue;
    const f = ind[4] || 1, mc = mcFresh() ? S.mc.res : null, b = mc && mc.n > 1 && mc.bands[k];
    const series = [{ y: r.states[k].map((x) => x * f), t: r.state_t, color: COLORS.ink }];
    const fills = [];
    if (b) {
      const sc = (a) => a.map((x) => x * f);
      fills.push({ t: mc.t, lo: sc(b.p05), hi: sc(b.p95), color: 'rgba(11,94,122,0.13)' });
      fills.push({ t: mc.t, lo: sc(b.p25), hi: sc(b.p75), color: 'rgba(11,94,122,0.22)' });
      for (const tr of mc.examples) series.push({ y: sc(tr[k]), t: mc.t, color: 'rgba(11,94,122,0.55)', width: 0.9 });
      series.push({ y: sc(b.p50), t: mc.t, color: COLORS.accent, width: 1.8, dash: [5, 3] });
    }
    if (mc && mc.n === 1 && mc.examples[0][k]) {
      series[0].color = 'rgba(27,42,53,0.45)';
      series.push({ y: mc.examples[0][k].map((x) => x * f), t: mc.t, color: COLORS.accent, width: 1.8 });
    }
    timeChart(c, { ...base, series, fills, points: !mc });
  }
}

// ---------------------------------------------------------------- Monte-Carlo (variabilité)
// Un animal est généré (variabilité interindividuelle, graine du cas) puis suivi dans le temps :
// N trajectoires avec fluctuation naturelle, fluctuation/progression de la maladie, variabilité
// PK/PD et bruit de mesure (modules/variation.json, placeholders D). Calcul dans un Web Worker.

// drug : tiré une fois par chien ; les trois autres retirent à chaque instant (options, coupées par défaut)
const MC_SRC = [['drug', 'PK/PD propre au chien (tirée une fois)'], ['natural', '+ fluctuation au fil des heures'], ['disease', '+ jours bons/mauvais'], ['measurement', '+ bruit de mesure']];

/** Signature du cas simulé : les résultats Monte-Carlo ne valent que pour elle. */
function mcSignature() {
  const M = S.mc, i = S.inputs;
  return JSON.stringify([i.bw, i.disease, i.stage, M.dog, S.treat.regimens, S.treat.days, M.n, M.individual, M.src, M.progression]);
}
function mcFresh() { return S.mc.res && S.mc.sig === mcSignature(); }

function mcPanel(r) {
  const M = S.mc;
  const opt = (x) => `<option value="${x}" ${M.n === x ? 'selected' : ''}>${x === 1 ? '1 (ce chien)' : x + ' réalisations'}</option>`;
  let s = `<div class="row">
      <label><input type="checkbox" id="mc-ind" ${M.individual ? 'checked' : ''}> chien tiré (lois normales sur ln des paramètres) n° <b>${M.dog}</b></label>
      <label>suivi <select id="mc-n">${[1, 10, 20].map(opt).join('')}</select></label>
      ${MC_SRC.map(([k, l]) => `<label><input type="checkbox" data-mcsrc="${k}" ${M.src[k] ? 'checked' : ''}> ${l}</label>`).join('')}
      ${S.inputs.disease ? `<label>progression (sévérité/jour)<input id="mc-prog" type="number" step="0.001" min="-0.1" max="0.1" style="width:80px" value="${M.progression}"></label>` : ''}
      ${M.running ? `<button id="mc-stop">Arrêter</button> <progress max="${M.n}" value="${M.progress}"></progress> <span id="mc-pt">${M.progress}/${M.n}</span>`
        : `<button class="primary" id="mc-run">${M.res ? 'Relancer' : 'Générer ce chien'}</button> <button id="mc-next">Autre chien</button>${M.res ? ' <button id="mc-clear">Effacer</button>' : ''}`}
    </div>`;
  if (M.error) s += `<div class="warn">${esc(M.error)}</div>`;
  if (M.res && !mcFresh() && !M.running) s += '<div class="warn">Résultats d\'un autre réglage : relancer pour mettre à jour.</div>';
  if (M.res && mcFresh()) {
    const dead = M.res.alive.map((a) => 1 - a), last = dead[dead.length - 1];
    s += M.res.n === 1
      ? `<div class="note">Courbe bleue : le chien n° ${M.dog}, paramètres tirés une fois (lois normales sur ln), suivi dans le temps${['natural', 'disease', 'measurement'].some((k) => M.src[k]) ? ' avec les fluctuations cochées' : ' sans nouveau tirage : la courbe ne bouge que par la maladie et le médicament'} ; trait gris : chien moyen.${last > 0 ? ` Ce chien décède à ${dayLabel(M.res.t[M.res.alive.findIndex((a) => a < 1)])}.` : ''}</div>`
      : `<div class="note">${M.res.n} réalisations du chien n° ${M.dog}. Bande claire = 5ᵉ–95ᵉ percentile, bande foncée = 25ᵉ–75ᵉ, tirets = médiane, traits fins = 3 réalisations ; trait noir = chien moyen sans aléa.${last > 0 ? ` Décès dans ${fmt(100 * last, 0)} % des réalisations à ${dayLabel(M.res.t[M.res.t.length - 1])}.` : ''}</div>`;
  }
  s += `<div class="note">Amplitudes de variabilité (fluctuation des paramètres, de la sévérité, PK/PD, imprécision analytique) : placeholders à caler (evidence D). Calcul ${S.inputs.disease === 'mmvd' ? 'long en MMVD (≈ 0,2 s par point)' : 'de quelques secondes'} ; grille de ${mcTimes(r.plan.horizonH).length} instants.</div>`;
  return s;
}

function mcBandNote(k) {
  if (!mcFresh() || !S.mc.res.bands[k]) return '';
  if (S.mc.res.n === 1) {
    const r1 = S.mc.res, j1 = nearest(r1.t, S.treat.cursor), ind1 = INDICATORS.find((x) => x[0] === k);
    const x = r1.examples[0][k][j1];
    return Number.isFinite(x) ? ` ; chien n° ${S.mc.dog} : ${fmt(x * (ind1[4] || 1), Math.max(ind1[3], 2))}` : '';
  }
  const b = S.mc.res.bands[k], j = nearest(S.mc.res.t, S.treat.cursor), ind = INDICATORS.find((x) => x[0] === k), f = ind[4] || 1;
  if (!Number.isFinite(b.p05[j])) return '';
  const d = Math.max(ind[3], 2);
  return ` ; Monte-Carlo : ${fmt(b.p50[j] * f, d)} [${fmt(b.p05[j] * f, d)} – ${fmt(b.p95[j] * f, d)}]`;
}

/** Grille d'instants : ~48 points pour un chien, ~24 pour plusieurs réalisations (coût du moteur). */
function mcTimes(h) {
  const step = Math.max(1, Math.ceil(h / (S.mc.n === 1 ? 48 : 24)));
  return Array.from({ length: Math.floor(h / step) + 1 }, (_, i) => i * step);
}

function bindMc(v) {
  const M = S.mc;
  if ($('#mc-n')) $('#mc-n').onchange = (e) => { M.n = +e.target.value; render(); };
  if ($('#mc-ind')) $('#mc-ind').onchange = (e) => { M.individual = e.target.checked; render(); };
  if ($('#mc-prog')) $('#mc-prog').onchange = (e) => { const x = parseFloat(e.target.value); M.progression = Number.isFinite(x) ? x : 0; render(); };
  for (const c of v.querySelectorAll('[data-mcsrc]')) c.onchange = () => { M.src = { ...M.src, [c.dataset.mcsrc]: c.checked }; render(); };
  if ($('#mc-run')) $('#mc-run').onclick = runMc;
  if ($('#mc-next')) $('#mc-next').onclick = () => { M.dog += 1; runMc(); };
  if ($('#mc-stop')) $('#mc-stop').onclick = () => { if (M.worker) M.worker.terminate(); M.worker = null; M.running = false; render(); };
  if ($('#mc-clear')) $('#mc-clear').onclick = () => { M.res = null; M.error = null; render(); };
}

function runMc() {
  const M = S.mc, T = S.treat, i = S.inputs;
  if (M.worker) M.worker.terminate();
  const keys = [...new Set([...INDICATORS.map((x) => x[0])])];
  const times = mcTimes(T.days * 24);
  const sig = mcSignature();
  M.running = true; M.progress = 0; M.error = null;
  const w = new Worker('./mc-worker.js', { type: 'module' });
  M.worker = w;
  w.onmessage = (ev) => {
    const d = ev.data;
    if (d.error) { M.error = 'Monte-Carlo : ' + d.error; M.running = false; w.terminate(); M.worker = null; render(); return; }
    if (d.progress !== undefined) { M.progress = d.progress; const p = document.querySelector('#view progress'), pt = $('#mc-pt'); if (p) { p.value = d.progress; if (pt) pt.textContent = `${d.progress}/${M.n}`; } else render(); return; }
    if (d.done) { M.res = mcSummary(d.t, d.trajectories, keys); M.sig = sig; M.running = false; w.terminate(); M.worker = null; render(); }
  };
  w.postMessage({
    bw: i.bw, seed: M.dog, individual: M.individual, disease: i.disease, stage: i.stage,
    doses: planDoses({ regimens: T.regimens }), times, n: M.n, keys,
    scale: Object.fromEntries(MC_SRC.map(([k]) => [k, M.src[k] ? 1 : 0])),
    progression: i.disease ? { [i.disease]: M.progression } : {}, chunk: i.disease === 'mmvd' ? 1 : 4,
  });
  render();
}

/** Percentiles par instant sur les trajectoires vivantes. */
function mcSummary(t, trajs, keys) {
  const q = (a, p) => { if (!a.length) return NaN; const x = p * (a.length - 1), lo = Math.floor(x), hi = Math.min(a.length - 1, lo + 1); return a[lo] + (x - lo) * (a[hi] - a[lo]); };
  const bands = {};
  for (const k of keys) {
    if (!trajs.some((tr) => tr.values[k])) continue;
    const b = { p05: [], p25: [], p50: [], p75: [], p95: [] };
    for (let j = 0; j < t.length; j++) {
      const a = trajs.map((tr) => tr.values[k] && tr.values[k][j]).filter((x) => typeof x === 'number').sort((x, y) => x - y);
      b.p05.push(q(a, 0.05)); b.p25.push(q(a, 0.25)); b.p50.push(q(a, 0.5)); b.p75.push(q(a, 0.75)); b.p95.push(q(a, 0.95));
    }
    bands[k] = b;
  }
  const examples = trajs.slice(0, 3).map((tr) => Object.fromEntries(Object.entries(tr.values).map(([k, v]) => [k, v.map((x) => (typeof x === 'number' ? x : NaN))])));
  const alive = t.map((_, j) => trajs.filter((tr) => tr.outcome[j] !== 'dead').length / trajs.length);
  return { t, n: trajs.length, bands, examples, alive };
}

/** Indice du point de la grille (croissante) le plus proche de x (le premier en cas d'égalité). */
function nearest(t, x) {
  let lo = 0, hi = t.length - 1;
  if (hi <= 0) return 0;
  while (hi - lo > 1) { const m = (lo + hi) >> 1; if (t[m] <= x) lo = m; else hi = m; }
  return Math.abs(t[hi] - x) < Math.abs(t[lo] - x) ? hi : lo;
}

/** Prochain pic (maximum local) après x, ou dernier creux (minimum local) avant x. */
function extremum(t, c, x, forward, max) {
  const ok = (i) => (max ? c[i] >= c[i - 1] && c[i] > c[i + 1] : c[i] <= c[i - 1] && c[i] < c[i + 1]);
  if (forward) { for (let i = 1; i < c.length - 1; i++) if (t[i] > x + 1e-9 && ok(i)) return t[i]; }
  else for (let i = c.length - 2; i >= 1; i--) if (t[i] < x - 1e-9 && ok(i)) return t[i];
  return x;
}

main();
