// Cœur animé, piloté par le modèle (API `heart`) : rien n'est réglé dans ce fichier.
// - Électrique : séquence de la couche de conduction (activations atriales, influx au nœud AV et
//   leur devenir, instant du His, origine des battements) et bornes des ondes de l'ECG (P ou f,
//   QRS, T) calculées par le moteur avec les mêmes formules que le tracé.
// - Mécanique : activation ventriculaire (élastance active) et atriale, volumes, pressions et
//   débits valvulaires de la simulation battement par battement ; les valves s'ouvrent quand le
//   débit est antérograde et fuient quand il est rétrograde.
// Seule la géométrie est schématique (vue frontale : OD et VD à gauche de l'écran).
// Usage pédagogique uniquement.
import { COLORS, timeChart } from './charts.js';

// ------------------------------------------------------------------ palette
const C = {
  ink: '#1b2a35', muted: '#5b6b77',
  myo: '#c9786d', myoDark: '#7d2f27', epi: '#f4dcd5', epiLine: '#d9a99f',
  bloodV: '#c3cdee', bloodVd: '#8f9fd6', bloodA: '#f6c1bb', bloodAd: '#e48a80',
  dep: '#ff8a1f', conduct: '#e0a800', conductDim: '#d9c27a', impulse: '#fff6c9',
  fwd: '#0f766e', reg: '#d0211b', block: '#d0211b', conceal: '#e07b00',
};
// Couleurs des phases mécaniques (cœur gauche).
const PHASES = {
  ivc: ['Contraction isovolumique', '#f2a33a'], ej: ['Éjection', '#d64545'],
  ivr: ['Relaxation isovolumique', '#8b5cc7'], fe: ['Remplissage passif (E)', '#5aa9e6'],
  fa: ['Systole atriale (A)', '#2e6fd1'], rest: ['Diastole, valves fermées', '#b8c2cc'],
};

// ------------------------------------------------------------------ géométrie (schéma)
// Cavités : contour, point fixe de l'échelle (plan valvulaire), épaisseur de paroi au repos
// et épaississement maximal sous activation.
const SHAPES = {
  ra: { d: 'M64,96 C58,56 146,40 176,78 L182,174 L70,174 C60,150 62,122 64,96 Z', c: [124, 176], wall: 6, gain: 3, blood: 'V' },
  la: { d: 'M244,84 C256,48 350,54 356,96 L354,174 L240,174 Z', c: [298, 176], wall: 6, gain: 3, blood: 'A' },
  rv: { d: 'M78,198 L196,198 C200,252 196,292 182,320 C138,302 90,262 78,198 Z', c: [137, 198], wall: 8, gain: 5, blood: 'V' },
  lv: { d: 'M224,198 L342,198 C346,252 314,314 254,342 C232,302 218,252 224,198 Z', c: [283, 198], wall: 14, gain: 9, blood: 'A' },
};
// Masse ventriculaire (épicarde), contractée avec les ventricules.
const EPI = { d: 'M60,192 C52,266 128,336 254,374 C336,326 376,262 366,192 Z', c: [213, 196] };
// Gros vaisseaux.
const VESSELS = [
  { d: 'M136,8 L136,62', w: 26, col: 'V', lab: ['VCS', 150, 18] },
  { d: 'M366,104 L398,98 M364,146 L398,152', w: 12, col: 'A', lab: ['VP', 384, 128] },
  { d: 'M233,190 C228,132 232,72 268,44 C302,18 344,26 360,62', w: 22, col: 'A', lab: ['Ao', 316, 22] },
  { d: 'M189,190 C188,150 198,108 228,84', w: 18, col: 'V', lab: ['AP', 212, 156] },
];
// Valves : extrémités de l'anneau, sens antérograde (unitaire), débit du modèle.
const VALVES = {
  tr: { a: [86, 186], b: [150, 186], d: [0, 1], q: 'q_tr', name: 'Tricuspide' },
  pu: { a: [180, 190], b: [198, 190], d: [0, -1], q: 'q_pu', name: 'Pulmonaire' },
  ao: { a: [223, 190], b: [243, 190], d: [0, -1], q: 'q_ao', name: 'Aortique' },
  mi: { a: [264, 186], b: [334, 186], d: [0, 1], q: 'q_mi', name: 'Mitrale' },
};
// Tissu de conduction et cavité qui le porte (il suit sa contraction).
const PATHS = {
  internodal: 'M148,64 C152,110 172,150 207,186',
  bachmann: 'M148,64 C196,52 240,64 270,90',
  his: 'M207,186 L209,214',
  rbb: 'M209,214 C204,256 198,292 184,316',
  lbb: 'M209,214 C218,258 230,300 252,336',
  purk_r: 'M184,316 C128,300 86,252 84,206',
  purk_l: 'M252,336 C310,306 346,256 338,206',
};
const PATH_OF = { internodal: 'ra', bachmann: 'la', rbb: 'rv', purk_r: 'rv', lbb: 'lv', purk_l: 'lv' };
const SA = [148, 64], AVN = [207, 186], HISB = [209, 214];
const V_ORIGIN = { sinus: [210, 262], junctional: [210, 262], ventricular_escape: [209, 304], ectopic: [330, 252] };
const LABELS = { ra: ['OD', 112, 128], la: ['OG', 300, 124], rv: ['VD', 132, 252], lv: ['VG', 282, 262] };

const ORIGIN_FR = {
  sinus: 'Battement sinusal conduit', junctional: 'Échappement jonctionnel',
  ventricular_escape: 'Échappement ventriculaire', ectopic: 'Extrasystole ventriculaire',
};
const FATE_FR = {
  conducted: 'conduit', nodal_refractory: 'bloqué au nœud AV (période réfractaire)',
  concealed: 'bloqué dans le nœud AV (conduction cachée)', collision: 'bloqué : influx précédent encore dans le His-Purkinje',
  infranodal: 'bloqué sous le His (bloc infranodal)', ventricular_refractory: 'arrivé aux ventricules réfractaires',
};

const clamp01 = (x) => Math.max(0, Math.min(1, x));
const within = (t, a, b) => t >= a && t <= b;
function sample(y, fs, t) {
  const x = Math.max(0, Math.min(y.length - 1, t * fs)), i = Math.floor(x), j = Math.min(y.length - 1, i + 1);
  return y[i] + (y[j] - y[i]) * (x - i);
}
const lastBefore = (arr, t, key) => { let x = null; for (const e of arr) { if (e[key] <= t) x = e; else break; } return x; };
const mix = (c1, c2, x) => {
  const h = (c) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
  const [a, b] = [h(c1), h(c2)];
  return '#' + a.map((v, i) => Math.round(v + (b[i] - v) * clamp01(x)).toString(16).padStart(2, '0')).join('');
};

/**
 * Chronologie électrique tirée des sorties du moteur. Conventions du modèle : l'influx atteint le
 * nœud AV au début de l'onde P (pas de délai intra-atrial), sort du nœud vers le His à `t_his`, et
 * l'instant ventriculaire est le pic R (QRS centré dessus).
 */
export function heartTimeline(r) {
  const el = r.electrical, tracks = [], marks = [], nodal = [];
  const hv = r.intervals.hv;
  const vent = el.ventricular.map((b) => ({ ...b, w: b.qrs_off - b.qrs_on }));
  for (const a of el.atrial) {
    if (a.organized) tracks.push({ p: 'internodal', t0: a.on, t1: a.off }, { p: 'bachmann', t0: a.on, t1: a.off });
  }
  for (const im of el.impulses) {
    if (im.t_his !== null) nodal.push({ t0: im.t, t1: im.t_his, ok: true });
    else nodal.push({ t0: im.t, t1: im.t + (im.fate === 'concealed' ? 0.04 : 0.015), ok: false });
    if (im.fate === 'nodal_refractory' || im.fate === 'collision' || im.fate === 'concealed') {
      marks.push({ at: AVN, t0: im.t, t1: im.t + 0.25, fate: im.fate });
    } else if (im.fate === 'infranodal') {
      tracks.push({ p: 'his', t0: im.t_his, t1: im.t_his + 0.4 * hv });
      marks.push({ at: HISB, t0: im.t_his, t1: im.t_his + 0.25, fate: im.fate });
    } else if (im.fate === 'ventricular_refractory') {
      tracks.push({ p: 'his', t0: im.t_his, t1: im.t_his + hv });
      marks.push({ at: HISB, t0: im.t_his + hv, t1: im.t_his + hv + 0.25, fate: im.fate });
    }
  }
  for (const b of vent) {
    if (b.t_his === null) continue; // battement ventriculaire : pas de passage par le His
    if (b.origin === 'junctional') nodal.push({ t0: b.t_his - 0.01, t1: b.t_his, ok: true });
    tracks.push({ p: 'his', t0: b.t_his, t1: Math.max(b.t_his + 0.002, b.qrs_on) });
    const w = b.w;
    tracks.push({ p: 'rbb', t0: b.qrs_on, t1: b.qrs_on + 0.35 * w }, { p: 'lbb', t0: b.qrs_on, t1: b.qrs_on + 0.35 * w });
    tracks.push({ p: 'purk_r', t0: b.qrs_on + 0.35 * w, t1: b.qrs_on + 0.75 * w }, { p: 'purk_l', t0: b.qrs_on + 0.35 * w, t1: b.qrs_on + 0.8 * w });
  }
  return { vent, atria: el.atrial, tracks, marks, nodal, saBlocked: el.sa_exit_blocked, impulses: el.impulses };
}

/** Phase mécanique du cœur gauche (clé de PHASES), lue sur les débits et l'activation du modèle. */
export function leftPhase(r, t, qmax) {
  const s = r.series, fs = r.fs;
  const qa = sample(s.q_ao, fs, t), qm = sample(s.q_mi, fs, t), av = sample(s.act_v, fs, t), aa = sample(s.act_a, fs, t);
  const dav = sample(s.act_v, fs, t + 0.005) - sample(s.act_v, fs, t - 0.005);
  const eps = 0.02, reg = qm < -eps * qmax.mi;
  let k = 'rest';
  if (qa > eps * qmax.ao) k = 'ej';
  else if (qm > eps * qmax.mi) k = aa > 0.05 ? 'fa' : 'fe';
  else if (av > 0.02) k = dav >= 0 ? 'ivc' : 'ivr';
  return { k, reg };
}

/** Monte la vue dans `v` et lance l'animation ; `H` garde l'état (instant, vitesse, lecture). */
export function mountHeart(v, r, H, ui) {
  const tl = heartTimeline(r), s = r.series, fs = r.fs, dur = r.duration_s;
  const range = (y) => [Math.min(...y), Math.max(...y)];
  const vmax = { lv: Math.max(...s.v_lv), rv: Math.max(...s.v_rv) };
  const arange = { la: range(s.v_la), ra: range(s.v_ra) };
  const qmax = {};
  for (const k of Object.keys(VALVES)) qmax[k] = Math.max(1e-9, ...s[VALVES[k].q]);
  // phases précalculées (ruban)
  const nPh = Math.floor(dur * fs), phase = new Array(nPh), regs = new Array(nPh);
  for (let i = 0; i < nPh; i++) { const p = leftPhase(r, i / fs, qmax); phase[i] = p.k; regs[i] = p.reg; }
  const NS = 'http://www.w3.org/2000/svg';
  const blood = (k) => `url(#h-g-${SHAPES[k].blood})`;
  const pathEl = (k) => `<path id="h-p-${k}" d="${PATHS[k]}" class="h-cond"/>`;
  const chamber = (k) => {
    const S = SHAPES[k];
    return `<g id="h-${k}">
      <mask id="h-m-${k}" maskUnits="userSpaceOnUse" x="0" y="0" width="420" height="400"><path id="h-mp-${k}" d="${S.d}" fill="black" stroke="white" stroke-width="${S.wall}" stroke-linejoin="round"/></mask>
      <path id="h-wall-${k}" d="${S.d}" fill="${blood(k)}" stroke="${C.myo}" stroke-width="${S.wall}" stroke-linejoin="round"/>
      <g mask="url(#h-m-${k})"><circle id="h-w-${k}" r="0" fill="${C.dep}" opacity="0"/><g id="h-f-${k}"></g></g>
      <text class="h-cav" x="${LABELS[k][1]}" y="${LABELS[k][2]}">${LABELS[k][0]}</text>
      ${Object.keys(PATHS).filter((p) => PATH_OF[p] === k).map(pathEl).join('')}</g>`;
  };
  const sw = (lab, val) => `<button class="${H.speed === val ? 'on' : ''}" data-speed="${val}">${lab}</button>`;
  const vessel = (x) => `<path d="${x.d}" fill="none" stroke="${x.col === 'A' ? C.bloodAd : C.bloodVd}" stroke-width="${x.w + 5}" stroke-linecap="round"/>
      <path d="${x.d}" fill="none" stroke="${x.col === 'A' ? C.bloodA : C.bloodV}" stroke-width="${x.w}" stroke-linecap="round"/>
      <text class="h-vlab" x="${x.lab[1]}" y="${x.lab[2]}">${x.lab[0]}</text>`;
  v.innerHTML = `<div class="hv">
    <div class="hv-bar">
      <button class="hv-play" id="h-play" aria-label="Lecture ou pause (espace)">${H.playing ? '❚❚' : '▶'}</button>
      <div class="hv-time"><span id="h-tl">${ui.fmt(H.t, 2)}</span><small> s / ${ui.fmt(dur, 0)} s</small></div>
      <input id="h-t" class="hv-scrub" type="range" min="0" max="${dur}" step="0.002" value="${H.t}" aria-label="Instant">
      <div class="hv-seg" role="group" aria-label="Vitesse">${sw('×1', 1)}${sw('×½', 0.5)}${sw('×¼', 0.25)}${sw('×⅒', 0.1)}</div>
      <div class="hv-seg"><button id="h-prev" title="Battement précédent (←)">⟨ battement</button><button id="h-next" title="Battement suivant (→)">battement ⟩</button></div>
    </div>
    <div class="hv-main">
      <div class="hv-stage">
        <div class="hv-chips"><span class="hv-chip" id="h-chip-e"><i></i><b>Électrique</b><span id="h-elec"></span></span>
          <span class="hv-chip" id="h-chip-m"><i></i><b>Mécanique</b><span id="h-mech"></span></span></div>
        <svg id="h-svg" viewBox="30 0 390 390" role="img" aria-label="Schéma animé du cœur : cavités, valves, gros vaisseaux et tissu de conduction">
          <defs>
            <radialGradient id="h-g-V" cx="40%" cy="35%" r="75%"><stop offset="0" stop-color="#dfe5f8"/><stop offset="1" stop-color="${C.bloodV}"/></radialGradient>
            <radialGradient id="h-g-A" cx="40%" cy="35%" r="75%"><stop offset="0" stop-color="#fde1dd"/><stop offset="1" stop-color="${C.bloodA}"/></radialGradient>
            <radialGradient id="h-g-epi" cx="45%" cy="30%" r="80%"><stop offset="0" stop-color="#fbeee9"/><stop offset="1" stop-color="${C.epi}"/></radialGradient>
            <filter id="h-glow" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="2.6" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
            <filter id="h-shadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="3" stdDeviation="5" flood-color="#7d2f27" flood-opacity="0.18"/></filter>
          </defs>
          ${VESSELS.map(vessel).join('')}
          <g id="h-epi"><path d="${EPI.d}" fill="url(#h-g-epi)" stroke="${C.epiLine}" stroke-width="2" filter="url(#h-shadow)"/></g>
          ${chamber('rv')}${chamber('lv')}${chamber('ra')}${chamber('la')}
          ${Object.keys(VALVES).map((k) => `<g><path id="h-vj-${k}" class="h-jet" fill="none"/><path id="h-vh-${k}" class="h-jet" fill="none"/><path id="h-vl-${k}" class="h-leaf" fill="none"/></g>`).join('')}
          ${pathEl('his')}
          <circle id="h-sa" cx="${SA[0]}" cy="${SA[1]}" r="6" class="h-node"/>
          <circle id="h-avn" cx="${AVN[0]}" cy="${AVN[1]}" r="6" class="h-node"/>
          <circle id="h-avn-p" cx="${AVN[0]}" cy="${AVN[1]}" r="10.5" fill="none" stroke-width="3.5" opacity="0" transform="rotate(-90 ${AVN[0]} ${AVN[1]})"/>
          <g id="h-marks"></g><g id="h-dots" filter="url(#h-glow)"></g>
          <circle id="h-focus" r="8" fill="none" stroke="${C.block}" stroke-width="3" opacity="0"/>
          <text class="h-small" x="${SA[0] - 12}" y="${SA[1] + 22}" text-anchor="end">nœud sinusal</text>
          <text class="h-small" x="${AVN[0] + 22}" y="${AVN[1] + 26}">nœud AV</text>
        </svg>
        <canvas id="h-ribbon" class="hv-ribbon" aria-label="Phases du cœur gauche"></canvas>
        <div class="hv-legend">${Object.values(PHASES).map(([l, c]) => `<span><i style="background:${c}"></i>${l}</span>`).join('')}<span><i class="hatch"></i>Régurgitation mitrale</span></div>
        <div class="hv-legend"><span><i style="background:${C.dep}"></i>Myocarde dépolarisé</span><span><i style="background:${C.myoDark}"></i>Paroi active (épaissie)</span><span><i style="background:${C.conduct}"></i>Tissu de conduction</span><span><i style="background:${C.fwd}"></i>Débit antérograde</span><span><i style="background:${C.reg}"></i>Régurgitation, blocage, foyer</span></div>
      </div>
      <div class="hv-side">
        ${ui.card('ECG, dérivation II', ui.canvas('h-ecg', 96))}
        ${ui.card('Activation mécanique', ui.canvas('h-act', 74) + ui.legend([['Ventricules (élastance active)', COLORS.lv], ['Oreillettes', COLORS.la]]))}
        ${ui.card('Pressions (mmHg)', ui.canvas('h-pr', 96) + ui.legend([['Aorte', COLORS.ao], ['VG', COLORS.lv], ['OG', COLORS.la]]))}
        ${ui.card('Débits valvulaires (mL/s)', ui.canvas('h-q', 96) + ui.legend([['Mitral', COLORS.la], ['Aortique', COLORS.ao], ['< 0 : régurgitation', '#9aa5b1']]))}
        ${ui.card('Volumes ventriculaires (mL)', ui.canvas('h-vol', 74) + ui.legend([['VG', COLORS.lv], ['VD', COLORS.pa]]))}
      </div>
    </div>
    <div class="note">Tout ce qui est animé vient du moteur : séquence de conduction et devenir de chaque influx au nœud AV, bornes des ondes P, QRS et T de l'ECG, activation mécanique, volumes, pressions et débits simulés battement par battement. Conventions du modèle : l'influx atteint le nœud AV au début de l'onde P ; la mécanique ventriculaire démarre au pic R ; pas de repolarisation atriale tracée (onde Ta), l'oreillette est affichée repolarisée à la fin de sa systole. Géométrie et trajets schématiques ; ventricules à l'échelle de la racine cubique du volume, oreillettes amplifiées (leur volume inclut les veines). Clavier : espace (lecture), ← → (battement). Usage pédagogique uniquement.</div>
  </div>`;
  const svg = v.querySelector('#h-svg');
  const el = (id) => svg.querySelector('#' + id);
  const pathLen = {};
  for (const k of Object.keys(PATHS)) pathLen[k] = el('h-p-' + k).getTotalLength();
  const moved = (k, x, y, sc) => {
    if (!k) return [x, y];
    const [cx, cy] = SHAPES[k].c, f = sc[k];
    return [cx + f * (x - cx), cy + f * (y - cy)];
  };
  const setT = (node, k, cx, cy, f) => node.setAttribute('transform', `translate(${cx} ${cy}) scale(${f.toFixed(4)}) translate(${-cx} ${-cy})`);
  const arrow = (jet, head, mx, my, dx, dy, len, color) => {
    jet.setAttribute('d', `M${mx},${my} l${(dx * len).toFixed(1)},${(dy * len).toFixed(1)}`);
    const ex = mx + dx * len, ey = my + dy * len, nx = -dy, ny = dx;
    head.setAttribute('d', `M${(ex - 8 * dx + 7 * nx).toFixed(1)},${(ey - 8 * dy + 7 * ny).toFixed(1)} L${(ex + 3 * dx).toFixed(1)},${(ey + 3 * dy).toFixed(1)} L${(ex - 8 * dx - 7 * nx).toFixed(1)},${(ey - 8 * dy - 7 * ny).toFixed(1)}`);
    for (const n of [jet, head]) { n.style.stroke = color; n.style.opacity = '1'; }
  };
  const drawValve = (k, t) => {
    const V = VALVES[k], q = sample(s[V.q], fs, t);
    const [ax, ay] = V.a, [bx, by] = V.b, [dx, dy] = V.d;
    const L = 0.5 * Math.hypot(bx - ax, by - ay), ux = (bx - ax) / (2 * L), uy = (by - ay) / (2 * L);
    const th = clamp01(q / (0.15 * qmax[k])) * 1.25; // feuillets ouverts par le débit antérograde (≤ 72°)
    const pa = [ax + L * (Math.cos(th) * ux + Math.sin(th) * dx), ay + L * (Math.cos(th) * uy + Math.sin(th) * dy)];
    const pb = [bx + L * (-Math.cos(th) * ux + Math.sin(th) * dx), by + L * (-Math.cos(th) * uy + Math.sin(th) * dy)];
    el('h-vl-' + k).setAttribute('d', `M${ax},${ay} L${pa[0].toFixed(1)},${pa[1].toFixed(1)} M${bx},${by} L${pb[0].toFixed(1)},${pb[1].toFixed(1)}`);
    const mx = (ax + bx) / 2, my = (ay + by) / 2, jet = el('h-vj-' + k), head = el('h-vh-' + k);
    if (q > 0.02 * qmax[k]) arrow(jet, head, mx, my, dx, dy, 12 + 30 * clamp01(q / qmax[k]), C.fwd);
    else if (q < -0.02 * qmax[k]) arrow(jet, head, mx, my, -dx, -dy, 12 + 48 * clamp01(-q / qmax[k]), C.reg);
    else { jet.style.opacity = '0'; head.style.opacity = '0'; }
  };

  // ruban des phases (fenêtre de 3 s, comme les tracés)
  const ribbon = v.querySelector('#h-ribbon');
  const drawRibbon = (t, x0, x1) => {
    const dpr = window.devicePixelRatio || 1, w = ribbon.clientWidth, h = ribbon.clientHeight;
    if (ribbon.width !== Math.round(w * dpr)) { ribbon.width = Math.round(w * dpr); ribbon.height = Math.round(h * dpr); }
    const ctx = ribbon.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.clearRect(0, 0, w, h);
    const X = (tt) => ((tt - x0) / (x1 - x0)) * w;
    const i0 = Math.max(0, Math.floor(x0 * fs)), i1 = Math.min(nPh - 1, Math.ceil(x1 * fs));
    let a = i0;
    for (let i = i0 + 1; i <= i1 + 1; i++) {
      if (i > i1 || phase[i] !== phase[a] || regs[i] !== regs[a]) {
        ctx.fillStyle = PHASES[phase[a]][1];
        const xa = X(a / fs), xb = X(i / fs);
        ctx.fillRect(xa, 4, xb - xa + 0.5, h - 8);
        if (regs[a]) { ctx.fillStyle = 'rgba(208,33,27,0.55)'; for (let x = xa; x < xb; x += 5) ctx.fillRect(x, 4, 2, h - 8); }
        a = i;
      }
    }
    ctx.fillStyle = C.ink; ctx.fillRect(X(t) - 1, 0, 2, h);
  };

  const draw = () => {
    const t = H.t;
    // --- mécanique : taille (volumes) et paroi (activation)
    const scale = {};
    const av = sample(s.act_v, fs, t), aa = sample(s.act_a, fs, t);
    for (const k of ['lv', 'rv', 'la', 'ra']) {
      const S = SHAPES[k], vol = sample(s['v_' + k], fs, t);
      const f = k[1] === 'v' ? Math.cbrt(vol / vmax[k]) : 0.84 + 0.16 * clamp01((vol - arange[k][0]) / (arange[k][1] - arange[k][0] || 1));
      scale[k] = f;
      setT(el('h-' + k), k, S.c[0], S.c[1], f);
      const act = clamp01(k[1] === 'v' ? av : aa), w = (S.wall + S.gain * act).toFixed(2);
      el('h-wall-' + k).setAttribute('stroke-width', w);
      el('h-mp-' + k).setAttribute('stroke-width', w);
      el('h-wall-' + k).setAttribute('stroke', mix(C.myo, C.myoDark, act));
    }
    // la masse ventriculaire varie moins que les cavités (épaississement des parois)
    setT(el('h-epi'), 'epi', EPI.c[0], EPI.c[1], 1 - 0.7 * (1 - (0.6 * scale.lv + 0.4 * scale.rv)));
    for (const k of Object.keys(VALVES)) drawValve(k, t);
    // --- oreillettes : dépolarisation pendant l'onde P (ou f), repolarisée en fin de systole atriale
    const a = lastBefore(tl.atria, t, 'on');
    const aRepol = r.mechanics.ta_atr + r.mechanics.tra_atr;
    for (const k of ['ra', 'la']) {
      const wv = el('h-w-' + k), f = el('h-f-' + k);
      f.replaceChildren(); wv.setAttribute('opacity', '0');
      if (!a) continue;
      if (a.organized) {
        const end = a.t + aRepol;
        if (t > end) continue;
        wv.setAttribute('cx', SA[0]); wv.setAttribute('cy', SA[1]);
        wv.setAttribute('r', (240 * clamp01((t - a.on) / (a.off - a.on))).toFixed(1));
        wv.setAttribute('opacity', (1 - clamp01((t - (end - 0.04)) / 0.04)).toFixed(3));
      } else {
        // FA : ondelettes à chaque onde f (positions pseudo-aléatoires, durée = onde f de l'ECG)
        const idx = tl.atria.indexOf(a), [cx, cy] = SHAPES[k].c;
        for (let j = 0; j < 5; j++) {
          const h = Math.sin((idx + 1) * 12.9898 + j * 78.233 + (k === 'la' ? 3.1 : 0)) * 43758.5453;
          const u = h - Math.floor(h), w2 = h * 7.13 - Math.floor(h * 7.13);
          const c = document.createElementNS(NS, 'circle');
          c.setAttribute('cx', cx - 55 + 110 * u); c.setAttribute('cy', cy - 120 + 110 * w2);
          c.setAttribute('r', (8 + 26 * clamp01((t - a.on) / (a.off - a.on || 1))).toFixed(1));
          c.setAttribute('fill', C.dep); c.setAttribute('opacity', (0.9 * (1 - clamp01((t - a.off) / 0.06))).toFixed(3));
          f.appendChild(c);
        }
      }
    }
    // --- ventricules : dépolarisation pendant le QRS, repolarisation pendant l'onde T
    const b = lastBefore(tl.vent, t, 'qrs_on');
    const focus = el('h-focus');
    focus.setAttribute('opacity', '0');
    for (const k of ['rv', 'lv']) {
      const wv = el('h-w-' + k);
      wv.setAttribute('opacity', '0');
      if (!b || t > b.t_end) continue;
      const [ox, oy] = V_ORIGIN[b.origin] || V_ORIGIN.sinus;
      wv.setAttribute('cx', ox); wv.setAttribute('cy', oy);
      wv.setAttribute('r', (280 * clamp01((t - b.qrs_on) / b.w)).toFixed(1));
      wv.setAttribute('opacity', (1 - clamp01((t - b.t_peak) / (b.t_end - b.t_peak))).toFixed(3));
      if (b.t_his === null && t < b.qrs_off) {
        const [fx, fy] = moved(ox > 214 ? 'lv' : 'rv', ox, oy, scale);
        focus.setAttribute('cx', fx); focus.setAttribute('cy', fy); focus.setAttribute('opacity', '1');
      }
    }
    // --- tissu de conduction
    const dots = el('h-dots'), marks = el('h-marks');
    dots.replaceChildren(); marks.replaceChildren();
    for (const k of Object.keys(PATHS)) el('h-p-' + k).classList.remove('on');
    for (const tr of tl.tracks) {
      if (t < tr.t0 || t > tr.t1 + 0.03) continue;
      const p = el('h-p-' + tr.p);
      p.classList.add('on');
      if (t > tr.t1) continue;
      const pt = p.getPointAtLength(pathLen[tr.p] * clamp01((t - tr.t0) / (tr.t1 - tr.t0 || 1)));
      const [px, py] = moved(PATH_OF[tr.p], pt.x, pt.y, scale);
      const c = document.createElementNS(NS, 'circle');
      c.setAttribute('cx', px); c.setAttribute('cy', py); c.setAttribute('r', '5.5'); c.setAttribute('class', 'h-imp');
      dots.appendChild(c);
    }
    const sa = (a && a.organized && within(t, a.on, a.off)) || tl.saBlocked.some((x) => within(t, x, x + 0.03));
    const [sx, sy] = moved('ra', SA[0], SA[1], scale);
    const saN = el('h-sa');
    saN.setAttribute('cx', sx); saN.setAttribute('cy', sy); saN.setAttribute('r', sa ? '9' : '6'); saN.classList.toggle('on', !!sa);
    const nd = tl.nodal.find((x) => within(t, x.t0, x.t1));
    const ring = el('h-avn-p');
    if (nd) {
      const x = clamp01((t - nd.t0) / (nd.t1 - nd.t0 || 1)), Cc = 2 * Math.PI * 10.5;
      ring.setAttribute('opacity', '1'); ring.setAttribute('stroke-dasharray', `${(Cc * x).toFixed(1)} ${Cc.toFixed(1)}`);
      ring.setAttribute('stroke', nd.ok ? C.conduct : C.block);
    } else ring.setAttribute('opacity', '0');
    el('h-avn').classList.toggle('on', !!nd);
    for (const m of tl.marks) {
      if (!within(t, m.t0, m.t1)) continue;
      const g = document.createElementNS(NS, 'path'), [x, y] = m.at;
      g.setAttribute('d', `M${x - 9},${y - 9} l18,18 m0,-18 l-18,18`);
      g.setAttribute('class', 'h-x'); g.style.stroke = m.fate === 'concealed' ? C.conceal : C.block;
      marks.appendChild(g);
    }
    // --- états courants (puces)
    const e = [];
    const im = lastBefore(tl.impulses, t, 't');
    let ecol = C.conduct;
    if (b) e.push(a && !a.organized && b.origin === 'sinus' ? 'Conduit depuis les oreillettes fibrillantes' : ORIGIN_FR[b.origin] || b.origin);
    if (b && b.t_his === null) ecol = C.block;
    if (im && t - im.t < 0.6) {
      e.push(`influx au nœud AV : ${FATE_FR[im.fate] || im.fate}${im.t_his !== null ? ` (AH ${ui.fmt((im.t_his - im.t) * 1000, 0)} ms)` : ''}`);
      if (im.fate !== 'conducted') ecol = C.block;
    }
    if (tl.saBlocked.some((x) => within(t, x, x + 0.4))) { e.push('décharge sinusale bloquée à la sortie'); ecol = C.block; }
    if (a && !a.organized) e.push('oreillettes en fibrillation');
    if (!tl.atria.length) e.push('aucune activation atriale (arrêt atrial)');
    v.querySelector('#h-elec').textContent = e.join(' · ') || '—';
    v.querySelector('#h-chip-e i').style.background = ecol;
    const ph = leftPhase(r, t, qmax);
    const vol = sample(s.v_lv, fs, t), plv = sample(s.p_lv, fs, t), pla = sample(s.p_la, fs, t), pao = sample(s.p_ao, fs, t);
    v.querySelector('#h-mech').textContent = `${PHASES[ph.k][0]}${ph.reg ? ' + régurgitation mitrale' : ''} · VG ${ui.fmt(vol, 0)} mL · PVG ${ui.fmt(plv, 0)} / POG ${ui.fmt(pla, 0)} / PAo ${ui.fmt(pao, 0)} mmHg`;
    v.querySelector('#h-chip-m i').style.background = PHASES[ph.k][1];
    v.querySelector('#h-tl').textContent = ui.fmt(t, 2);
    v.querySelector('#h-t').value = t;
    // --- tracés synchronisés
    const x0 = Math.max(0, Math.min(dur - 3, t - 1.5)), x1 = x0 + Math.min(3, dur), dt = 1 / fs;
    const ser = (y, color) => ({ y, dt, color });
    drawRibbon(t, x0, x1);
    timeChart(v.querySelector('#h-ecg'), { series: [{ y: r.ecg, dt: 1 / r.ecg_fs, color: COLORS.ecg }], x0, x1, cursor: t, yMin: ui.ecgMin, yMax: ui.ecgMax, paper: true });
    timeChart(v.querySelector('#h-act'), { series: [ser(s.act_v, COLORS.lv), ser(s.act_a, COLORS.la)], x0, x1, cursor: t, yMin: 0, yMax: 1.3 });
    timeChart(v.querySelector('#h-pr'), { series: [ser(s.p_ao, COLORS.ao), ser(s.p_lv, COLORS.lv), ser(s.p_la, COLORS.la)], x0, x1, cursor: t, yMin: 0 });
    timeChart(v.querySelector('#h-q'), { series: [ser(s.q_mi, COLORS.la), ser(s.q_ao, COLORS.ao)], x0, x1, cursor: t, refLines: [0] });
    timeChart(v.querySelector('#h-vol'), { series: [ser(s.v_lv, COLORS.lv), ser(s.v_rv, COLORS.pa)], x0, x1, cursor: t, yMin: 0 });
  };

  const loop = (now) => {
    if (!svg.isConnected || !H.playing) { H.raf = null; return; }
    const dt = H.last === null ? 0 : Math.min(0.1, (now - H.last) / 1000);
    H.last = now;
    H.t = (H.t + dt * H.speed) % dur;
    draw();
    H.raf = requestAnimationFrame(loop);
  };
  const start = () => { if (H.raf) cancelAnimationFrame(H.raf); H.last = null; H.raf = requestAnimationFrame(loop); };
  const setPlaying = (on) => { H.playing = on; v.querySelector('#h-play').textContent = on ? '❚❚' : '▶'; if (on) start(); };
  v.querySelector('#h-play').onclick = () => setPlaying(!H.playing);
  // pas à pas : se placer juste avant l'activation du battement précédent / suivant
  const jump = (dir) => {
    setPlaying(false);
    const starts = tl.vent.map((x) => (x.t_his !== null ? x.t_his - 0.12 : x.qrs_on - 0.05)).filter((x) => x >= 0);
    const cand = dir > 0 ? starts.filter((x) => x > H.t + 0.01) : starts.filter((x) => x < H.t - 0.01);
    if (cand.length) H.t = dir > 0 ? cand[0] : cand[cand.length - 1];
    draw();
  };
  v.querySelector('#h-prev').onclick = () => jump(-1);
  v.querySelector('#h-next').onclick = () => jump(1);
  for (const btn of v.querySelectorAll('button[data-speed]')) btn.onclick = () => {
    H.speed = +btn.dataset.speed;
    for (const o of v.querySelectorAll('button[data-speed]')) o.classList.toggle('on', o === btn);
  };
  v.querySelector('#h-t').oninput = (e) => { H.t = +e.target.value; if (!H.playing) draw(); };
  // clavier (une seule écoute, retirée quand la vue disparaît)
  if (H.onKey) window.removeEventListener('keydown', H.onKey);
  H.onKey = (e) => {
    if (!svg.isConnected) { window.removeEventListener('keydown', H.onKey); H.onKey = null; return; }
    if (e.target.closest && e.target.closest('input, select, textarea')) return;
    if (e.key === ' ') { e.preventDefault(); setPlaying(!H.playing); } else if (e.key === 'ArrowRight') jump(1); else if (e.key === 'ArrowLeft') jump(-1);
  };
  window.addEventListener('keydown', H.onKey);
  draw();
  if (H.playing) start();
}
