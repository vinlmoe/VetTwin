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

const ACTIVE = '#ff7a2f', REST_A = '#f3d9d4', REST_V = '#efc9c3', WALL = '#9b4a43', WALL_ACT = '#5c1a14';
const CONDUCT = '#d4a017', JET = '#c0392b', FLOW = '#2563a8';

// Cavités : contour et point fixe de l'échelle (plan valvulaire : la base descend vers l'apex).
const SHAPES = {
  ra: { d: 'M66,72 C64,36 160,30 170,70 L172,150 C134,166 84,160 66,140 Z', c: [118, 160] },
  la: { d: 'M252,72 C258,40 350,42 352,82 L350,146 C316,160 268,158 252,148 Z', c: [301, 160] },
  rv: { d: 'M70,174 L198,174 L206,300 C170,322 120,292 90,252 C74,226 68,196 70,174 Z', c: [134, 168] },
  lv: { d: 'M214,174 L352,174 C362,234 334,302 256,346 C230,322 214,282 212,240 Z', c: [283, 168] },
};
// Gros vaisseaux (tronçons) au-dessus des valves sigmoïdes.
const VESSELS = { pa: 'M178,168 L178,112 L198,112 L198,168', ao: 'M224,168 L224,104 L246,104 L246,168' };
// Valves : extrémités de l'anneau, sens antérograde (unitaire), débit du modèle.
const VALVES = {
  tr: { a: [88, 168], b: [140, 168], d: [0, 1], q: 'q_tr' },
  pu: { a: [178, 168], b: [198, 168], d: [0, -1], q: 'q_pu' },
  ao: { a: [224, 168], b: [246, 168], d: [0, -1], q: 'q_ao' },
  mi: { a: [272, 168], b: [330, 168], d: [0, 1], q: 'q_mi' },
};
// Tissu de conduction (coordonnées du schéma) et cavité qui le porte (il suit sa contraction).
const PATHS = {
  internodal: 'M150,54 C156,100 170,140 209,166',
  bachmann: 'M150,54 C200,40 250,52 286,78',
  his: 'M209,166 L208,196',
  rbb: 'M208,196 C203,240 194,280 178,302',
  lbb: 'M208,196 C218,240 228,292 248,330',
  purk_r: 'M178,302 C120,292 80,232 80,184',
  purk_l: 'M248,330 C306,300 352,240 344,184',
};
const PATH_OF = { internodal: 'ra', bachmann: 'la', rbb: 'rv', purk_r: 'rv', lbb: 'lv', purk_l: 'lv' };
const SA = [150, 54], AVN = [209, 166], HISB = [208, 196];
// Point de départ de la dépolarisation ventriculaire selon l'origine du battement (schéma).
const V_ORIGIN = { sinus: [210, 250], junctional: [210, 250], ventricular_escape: [208, 296], ectopic: [326, 236] };
const ORIGIN_FR = {
  sinus: 'battement sinusal conduit', junctional: 'échappement jonctionnel (QRS fin)',
  ventricular_escape: 'échappement ventriculaire (QRS large)', ectopic: 'extrasystole ventriculaire (QRS large)',
};
const FATE_FR = {
  conducted: 'conduit', nodal_refractory: 'bloqué au nœud AV (période réfractaire)',
  concealed: 'bloqué dans le nœud AV en réfractarité relative (conduction cachée : allonge la réfractarité)', collision: 'bloqué : influx précédent encore dans le His-Purkinje',
  infranodal: 'bloqué sous le His (bloc infranodal)', ventricular_refractory: 'arrivé aux ventricules réfractaires',
};

const clamp01 = (x) => Math.max(0, Math.min(1, x));
const within = (t, a, b) => t >= a && t <= b;
/** Valeur d'une série échantillonnée à fs (interpolation linéaire). */
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
 * Chronologie électrique : trajets de l'influx (voie, début, fin) et marques de blocage, tirés
 * des sorties du moteur. Conventions du modèle : l'influx atteint le nœud AV au début de l'onde P
 * (pas de délai intra-atrial), sort du nœud vers le His à `t_his`, et l'instant ventriculaire
 * est le pic R (QRS centré dessus).
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

/** Phase mécanique du cœur gauche, lue sur les débits et l'activation du modèle. */
export function leftPhase(r, t, qmax) {
  const s = r.series, fs = r.fs;
  const qa = sample(s.q_ao, fs, t), qm = sample(s.q_mi, fs, t), av = sample(s.act_v, fs, t), aa = sample(s.act_a, fs, t);
  const dav = sample(s.act_v, fs, t + 0.005) - sample(s.act_v, fs, t - 0.005);
  const eps = 0.02;
  const reg = qm < -eps * qmax.mi ? ' + régurgitation mitrale' : '';
  if (qa > eps * qmax.ao) return 'éjection' + reg;
  if (qm > eps * qmax.mi) return aa > 0.05 ? 'remplissage actif (systole atriale, onde A)' : 'remplissage passif (onde E, diastasis)';
  if (av > 0.02) return (dav >= 0 ? 'contraction isovolumique' : 'relaxation isovolumique') + reg;
  return 'diastole, valves fermées' + reg;
}

/** Monte la vue dans `v` et lance l'animation ; `H` garde l'état (instant, vitesse, lecture). */
export function mountHeart(v, r, H, ui) {
  const tl = heartTimeline(r), s = r.series, fs = r.fs, dur = r.duration_s;
  const range = (y) => [Math.min(...y), Math.max(...y)];
  const vmax = { lv: Math.max(...s.v_lv), rv: Math.max(...s.v_rv) };
  const arange = { la: range(s.v_la), ra: range(s.v_ra) };
  const qmax = {};
  for (const k of Object.keys(VALVES)) qmax[k] = Math.max(1e-9, ...s[VALVES[k].q]);
  const NS = 'http://www.w3.org/2000/svg';
  const pathEl = (k) => `<path id="h-p-${k}" d="${PATHS[k]}" fill="none" stroke="${CONDUCT}" stroke-width="2.2" stroke-linecap="round" opacity="0.55"/>`;
  const chamber = (k) => `<g id="h-${k}"><clipPath id="h-cp-${k}"><path d="${SHAPES[k].d}"/></clipPath>
      <path id="h-wall-${k}" d="${SHAPES[k].d}" fill="${k[1] === 'a' ? REST_A : REST_V}" stroke="${WALL}" stroke-width="4" stroke-linejoin="round"/>
      <g clip-path="url(#h-cp-${k})"><circle id="h-w-${k}" r="0" fill="${ACTIVE}" opacity="0"/><g id="h-f-${k}"></g></g>
      ${Object.keys(PATHS).filter((p) => PATH_OF[p] === k).map(pathEl).join('')}</g>`;
  const sw = (lab, val) => `<button class="${H.speed === val ? 'on' : ''}" data-speed="${val}">${lab}</button>`;
  v.innerHTML = `<div class="toolbar"><div class="row" style="gap:18px">
      <button class="primary" id="h-play">${H.playing ? '❚❚ Pause' : '▶ Lecture'}</button>
      <div class="group"><span class="lab">Vitesse</span>${sw('×1', 1)}${sw('×0,5', 0.5)}${sw('×0,25', 0.25)}${sw('×0,1', 0.1)}</div>
      <div class="group"><button id="h-prev" title="Battement précédent">◀ battement</button><button id="h-next" title="Battement suivant">battement ▶</button></div></div>
    <div class="sliders"><label><span>Instant : <b id="h-tl">${ui.fmt(H.t, 3)}</b> s / ${ui.fmt(dur, 0)} s</span>
      <input id="h-t" type="range" min="0" max="${dur}" step="0.002" value="${H.t}"></label></div>
    <div class="summary" id="h-elec">&nbsp;</div><div class="summary" id="h-mech">&nbsp;</div></div>
    <div class="heart-grid">
      <div class="card"><h3>Contraction et influx électrique</h3>
        <svg id="h-svg" viewBox="30 10 360 350" role="img" aria-label="Schéma animé du cœur : cavités, valves et tissu de conduction">
          ${Object.values(VESSELS).map((d) => `<path d="${d}" fill="#f6e3df" stroke="${WALL}" stroke-width="3"/>`).join('')}
          <text x="188" y="106" class="h-small" text-anchor="middle">AP</text><text x="235" y="98" class="h-small" text-anchor="middle">Ao</text>
          <text x="112" y="28" class="h-lab">OD</text><text x="302" y="28" class="h-lab">OG</text>
          <text x="52" y="320" class="h-lab">VD</text><text x="366" y="320" class="h-lab">VG</text>
          ${chamber('ra')}${chamber('la')}${chamber('rv')}${chamber('lv')}
          ${Object.keys(VALVES).map((k) => `<g><path id="h-vl-${k}" fill="none" stroke="#3b2b2b" stroke-width="3.5" stroke-linecap="round"/><path id="h-vj-${k}" fill="none" stroke-linecap="round"/><path id="h-vh-${k}" fill="none" stroke-linecap="round" stroke-linejoin="round"/></g>`).join('')}
          ${pathEl('his')}
          <circle id="h-sa" cx="${SA[0]}" cy="${SA[1]}" r="6" fill="${CONDUCT}" stroke="#7a5c00"/>
          <circle id="h-avn" cx="${AVN[0]}" cy="${AVN[1]}" r="6" fill="${CONDUCT}" stroke="#7a5c00"/>
          <circle id="h-avn-p" cx="${AVN[0]}" cy="${AVN[1]}" r="10" fill="none" stroke="#ffd84a" stroke-width="3" opacity="0" transform="rotate(-90 ${AVN[0]} ${AVN[1]})"/>
          <g id="h-marks"></g><g id="h-dots"></g>
          <circle id="h-focus" r="7" fill="none" stroke="${COLORS.cursor}" stroke-width="3" opacity="0"/>
          <text x="${SA[0] - 6}" y="${SA[1] - 10}" class="h-small" text-anchor="end">nœud sinusal</text>
          <text x="${AVN[0] + 8}" y="${AVN[1] + 22}" class="h-small">nœud AV</text>
        </svg>
        <div class="legend"><span><i style="background:${ACTIVE}"></i>myocarde dépolarisé (fenêtres de l'ECG)</span><span><i style="background:${WALL_ACT}"></i>paroi épaissie = activation mécanique</span><span><i style="background:${CONDUCT}"></i>tissu de conduction, influx</span><span><i style="background:${FLOW}"></i>débit antérograde</span><span><i style="background:${JET}"></i>régurgitation, blocage, foyer</span></div>
      </div>
      <div class="col">
        ${ui.card('ECG, dérivation II (fenêtre de 3 s)', ui.canvas('h-ecg', 100))}
        ${ui.card('Activation mécanique (0–1)', ui.canvas('h-act', 80) + ui.legend([['Ventricules (élastance active × amplitude)', COLORS.lv], ['Oreillettes', COLORS.la]]))}
        ${ui.card('Pressions (mmHg)', ui.canvas('h-pr', 100) + ui.legend([['Aorte', COLORS.ao], ['VG', COLORS.lv], ['OG', COLORS.la]]))}
        ${ui.card('Débits valvulaires (mL/s ; négatif = régurgitation)', ui.canvas('h-q', 100) + ui.legend([['Mitral', COLORS.la], ['Aortique', COLORS.ao]]))}
        ${ui.card('Volumes ventriculaires (mL)', ui.canvas('h-vol', 80) + ui.legend([['VG', COLORS.lv], ['VD', COLORS.pa]]))}
        <div class="note">Tout ce qui est animé vient du moteur : séquence de conduction et devenir de chaque influx au nœud AV, bornes des ondes P, QRS et T de l'ECG, activation mécanique, volumes, pressions et débits simulés battement par battement. Conventions du modèle : l'influx atteint le nœud AV au début de l'onde P, la mécanique ventriculaire démarre au pic R, l'oreillette n'a pas de repolarisation tracée (pas d'onde Ta) : elle est affichée repolarisée à la fin de sa systole. Géométrie et trajets schématiques ; ventricules à l'échelle de la racine cubique du volume ; oreillettes amplifiées (leur volume inclut les veines). Usage pédagogique uniquement.</div>
      </div></div>`;
  const svg = v.querySelector('#h-svg');
  const el = (id) => svg.querySelector('#' + id);
  const pathLen = {};
  for (const k of Object.keys(PATHS)) pathLen[k] = el('h-p-' + k).getTotalLength();
  const moved = (k, x, y, sc) => {
    if (!k) return [x, y];
    const [cx, cy] = SHAPES[k].c, f = sc[k];
    return [cx + f * (x - cx), cy + f * (y - cy)];
  };
  const dot = (layer, x, y) => {
    const c = document.createElementNS(NS, 'circle');
    c.setAttribute('cx', x); c.setAttribute('cy', y); c.setAttribute('r', '5.5');
    c.setAttribute('fill', '#fff7c2'); c.setAttribute('stroke', '#b8860b'); c.setAttribute('stroke-width', '2');
    layer.appendChild(c);
  };
  const arrow = (jet, head, mx, my, dx, dy, len, color, width) => {
    jet.setAttribute('d', `M${mx},${my} l${(dx * len).toFixed(1)},${(dy * len).toFixed(1)}`);
    jet.setAttribute('stroke', color); jet.setAttribute('stroke-width', width); jet.setAttribute('opacity', '0.8');
    const ex = mx + dx * len, ey = my + dy * len, nx = -dy, ny = dx;
    head.setAttribute('d', `M${(ex - 7 * dx + 6 * nx).toFixed(1)},${(ey - 7 * dy + 6 * ny).toFixed(1)} L${(ex + 3 * dx).toFixed(1)},${(ey + 3 * dy).toFixed(1)} L${(ex - 7 * dx - 6 * nx).toFixed(1)},${(ey - 7 * dy - 6 * ny).toFixed(1)}`);
    head.setAttribute('stroke', color); head.setAttribute('stroke-width', '3.5'); head.setAttribute('opacity', '0.8');
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
    if (q > 0.02 * qmax[k]) arrow(jet, head, mx, my, dx, dy, 10 + 30 * clamp01(q / qmax[k]), FLOW, 5);
    else if (q < -0.02 * qmax[k]) arrow(jet, head, mx, my, -dx, -dy, 10 + 45 * clamp01(-q / qmax[k]), JET, 4);
    else { jet.setAttribute('opacity', '0'); head.setAttribute('opacity', '0'); }
  };

  const draw = () => {
    const t = H.t;
    // --- mécanique : taille (volumes) et paroi (activation)
    const scale = {};
    const av = sample(s.act_v, fs, t), aa = sample(s.act_a, fs, t);
    for (const k of ['lv', 'rv', 'la', 'ra']) {
      const vol = sample(s['v_' + k], fs, t);
      const f = k[1] === 'v' ? Math.cbrt(vol / vmax[k]) : 0.84 + 0.16 * clamp01((vol - arange[k][0]) / (arange[k][1] - arange[k][0] || 1));
      scale[k] = f;
      const [cx, cy] = SHAPES[k].c;
      el('h-' + k).setAttribute('transform', `translate(${cx} ${cy}) scale(${f.toFixed(4)}) translate(${-cx} ${-cy})`);
      const act = k[1] === 'v' ? av : aa, base = k === 'lv' ? 6 : k === 'rv' ? 4 : 3;
      el('h-wall-' + k).setAttribute('stroke-width', (base + (k[1] === 'v' ? 7 : 4) * clamp01(act)).toFixed(2));
      el('h-wall-' + k).setAttribute('stroke', mix(WALL, WALL_ACT, act));
    }
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
        wv.setAttribute('r', (230 * clamp01((t - a.on) / (a.off - a.on))).toFixed(1));
        wv.setAttribute('opacity', (0.9 * (1 - clamp01((t - (end - 0.04)) / 0.04))).toFixed(3));
      } else {
        // FA : ondelettes à chaque onde f (positions pseudo-aléatoires, durée = onde f de l'ECG)
        const idx = tl.atria.indexOf(a), [cx, cy] = SHAPES[k].c;
        for (let j = 0; j < 4; j++) {
          const h = Math.sin((idx + 1) * 12.9898 + j * 78.233 + (k === 'la' ? 3.1 : 0)) * 43758.5453;
          const u = h - Math.floor(h), w2 = h * 7.13 - Math.floor(h * 7.13);
          const c = document.createElementNS(NS, 'circle');
          c.setAttribute('cx', cx - 45 + 90 * u); c.setAttribute('cy', cy - 100 + 80 * w2);
          c.setAttribute('r', (6 + 22 * clamp01((t - a.on) / (a.off - a.on || 1))).toFixed(1));
          c.setAttribute('fill', ACTIVE); c.setAttribute('opacity', (0.75 * (1 - clamp01((t - a.off) / 0.06))).toFixed(3));
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
      wv.setAttribute('r', (270 * clamp01((t - b.qrs_on) / b.w)).toFixed(1));
      wv.setAttribute('opacity', (0.9 * (1 - clamp01((t - b.t_peak) / (b.t_end - b.t_peak)))).toFixed(3));
      if (b.t_his === null && t < b.qrs_off) {
        const [fx, fy] = moved(ox > 212 ? 'lv' : 'rv', ox, oy, scale);
        focus.setAttribute('cx', fx); focus.setAttribute('cy', fy); focus.setAttribute('opacity', '1');
      }
    }
    // --- tissu de conduction
    const dots = el('h-dots'), marks = el('h-marks');
    dots.replaceChildren(); marks.replaceChildren();
    for (const k of Object.keys(PATHS)) { const p = el('h-p-' + k); p.setAttribute('opacity', '0.55'); p.setAttribute('stroke-width', '2.2'); }
    for (const tr of tl.tracks) {
      if (t < tr.t0 || t > tr.t1 + 0.03) continue;
      const p = el('h-p-' + tr.p);
      p.setAttribute('opacity', '1'); p.setAttribute('stroke-width', '4');
      if (t > tr.t1) continue;
      const pt = p.getPointAtLength(pathLen[tr.p] * clamp01((t - tr.t0) / (tr.t1 - tr.t0 || 1)));
      const [px, py] = moved(PATH_OF[tr.p], pt.x, pt.y, scale);
      dot(dots, px, py);
    }
    const sa = (a && a.organized && within(t, a.on, a.off)) || tl.saBlocked.some((x) => within(t, x, x + 0.03));
    const [sx, sy] = moved('ra', SA[0], SA[1], scale);
    el('h-sa').setAttribute('cx', sx); el('h-sa').setAttribute('cy', sy); el('h-sa').setAttribute('r', sa ? '9' : '6');
    // nœud AV : anneau de progression de la conduction nodale (AH) jusqu'au His
    const nd = tl.nodal.find((x) => within(t, x.t0, x.t1));
    const ring = el('h-avn-p');
    if (nd) {
      const x = clamp01((t - nd.t0) / (nd.t1 - nd.t0 || 1)), C = 2 * Math.PI * 10;
      ring.setAttribute('opacity', '1'); ring.setAttribute('stroke-dasharray', `${(C * x).toFixed(1)} ${C.toFixed(1)}`);
      ring.setAttribute('stroke', nd.ok ? '#ffd84a' : JET);
    } else ring.setAttribute('opacity', '0');
    el('h-avn').setAttribute('fill', nd ? '#ffd84a' : CONDUCT);
    for (const m of tl.marks) {
      if (!within(t, m.t0, m.t1)) continue;
      const g = document.createElementNS(NS, 'path'), [x, y] = m.at;
      g.setAttribute('d', `M${x - 9},${y - 9} l18,18 m0,-18 l-18,18`);
      g.setAttribute('stroke', m.fate === 'concealed' ? '#e08a00' : JET); g.setAttribute('stroke-width', '4'); g.setAttribute('stroke-linecap', 'round');
      marks.appendChild(g);
    }
    // --- états courants
    const e = [];
    const im = lastBefore(tl.impulses, t, 't');
    if (b) e.push(`dernier battement : ${a && !a.organized && b.origin === 'sinus' ? 'conduit depuis les oreillettes fibrillantes' : ORIGIN_FR[b.origin] || b.origin}`);
    if (im && t - im.t < 0.6) e.push(`dernier influx au nœud AV : ${FATE_FR[im.fate] || im.fate}${im.t_his !== null ? `, AH ${ui.fmt((im.t_his - im.t) * 1000, 0)} ms` : ''}`);
    if (tl.saBlocked.some((x) => within(t, x, x + 0.4))) e.push('décharge sinusale bloquée à la sortie (bloc sino-atrial)');
    if (a && !a.organized) e.push('oreillettes en fibrillation');
    if (!tl.atria.length) e.push('aucune activation atriale (arrêt atrial)');
    v.querySelector('#h-elec').textContent = 'Électrique : ' + (e.join(' · ') || '—');
    const vol = sample(s.v_lv, fs, t), plv = sample(s.p_lv, fs, t), pla = sample(s.p_la, fs, t), pao = sample(s.p_ao, fs, t);
    v.querySelector('#h-mech').textContent = `Mécanique (cœur gauche) : ${leftPhase(r, t, qmax)} · VG ${ui.fmt(vol, 0)} mL · PVG ${ui.fmt(plv, 0)}, POG ${ui.fmt(pla, 0)}, PAo ${ui.fmt(pao, 0)} mmHg · activation ${ui.fmt(av, 2)}`;
    v.querySelector('#h-tl').textContent = ui.fmt(t, 3);
    v.querySelector('#h-t').value = t;
    // --- tracés synchronisés
    const x0 = Math.max(0, Math.min(dur - 3, t - 1.5)), x1 = x0 + Math.min(3, dur), dt = 1 / fs;
    const ser = (y, color) => ({ y, dt, color });
    timeChart(v.querySelector('#h-ecg'), { series: [{ y: r.ecg, dt: 1 / r.ecg_fs, color: COLORS.ecg }], x0, x1, cursor: t, yMin: ui.ecgMin, yMax: ui.ecgMax });
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
  const setPlaying = (on) => { H.playing = on; v.querySelector('#h-play').textContent = on ? '❚❚ Pause' : '▶ Lecture'; if (on) start(); };
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
  draw();
  if (H.playing) start();
}
