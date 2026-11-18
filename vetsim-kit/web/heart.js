// Cœur animé : contraction (volumes simulés battement par battement) et influx électrique
// (séquence de conduction du moteur, la même que celle de l'ECG et du son).
// Schéma en vue frontale : OD et VD à gauche de l'écran (côté droit du chien).
// Usage pédagogique uniquement.
import { COLORS, timeChart } from './charts.js';

const P_DUR = 0.05; // durée de dépolarisation atriale (onde P) affichée, s
const A_REPOL = 0.12; // repolarisation atriale (onde Ta) après la dépolarisation, s
const WIDE_QRS = 2.2; // QRS large d'un battement ventriculaire (même facteur que le moteur)
const ACTIVE = '#ff7a2f', REST_A = '#f3d9d4', REST_V = '#efc9c3', WALL = '#9b4a43', CONDUCT = '#d4a017';

// Cavités : contour et point fixe de l'échelle (plan valvulaire : la base descend vers l'apex).
const SHAPES = {
  ra: { d: 'M80,72 C78,36 176,30 186,70 L192,150 C150,166 96,160 78,140 Z', c: [136, 160] },
  la: { d: 'M232,72 C240,40 336,42 340,82 L338,146 C300,160 250,158 232,148 Z', c: [285, 160] },
  rv: { d: 'M80,172 L196,172 L206,300 C170,322 120,292 95,252 C80,226 76,196 80,172 Z', c: [140, 166] },
  lv: { d: 'M216,172 L346,172 C356,232 330,302 256,346 C230,322 216,282 213,240 Z', c: [281, 166] },
};
// Voies de conduction (coordonnées du schéma).
const PATHS = {
  internodal: 'M170,60 C184,100 190,130 200,160',
  bachmann: 'M170,60 C200,52 242,58 272,82',
  his: 'M200,160 L206,192',
  rbb: 'M206,192 C201,240 192,280 176,302',
  lbb: 'M206,192 C216,240 226,292 246,330',
  purk_r: 'M176,302 C120,292 86,232 86,182',
  purk_l: 'M246,330 C302,300 346,240 340,182',
};
// Cavité qui porte chaque voie (la voie suit sa contraction) ; le His reste sur le plan valvulaire.
const PATH_OF = { internodal: 'ra', bachmann: 'la', rbb: 'rv', purk_r: 'rv', lbb: 'lv', purk_l: 'lv' };
const SA = [170, 60], AVN = [200, 160];
// Origine de l'activation ventriculaire selon le battement.
const V_ORIGIN = { sinus: [208, 250], junctional: [208, 250], ventricular_escape: [206, 296], ectopic: [322, 236] };
const ORIGIN_FR = {
  sinus: 'battement sinusal conduit', junctional: 'échappement jonctionnel (QRS fin)',
  ventricular_escape: 'échappement ventriculaire (QRS large)', ectopic: 'extrasystole ventriculaire (QRS large)',
};

const lerp = (a, b, x) => a + (b - a) * x;
const clamp01 = (x) => Math.max(0, Math.min(1, x));

/** Valeur d'une série échantillonnée à fs (interpolation linéaire). */
function sample(y, fs, t) {
  const x = Math.max(0, Math.min(y.length - 1, t * fs)), i = Math.floor(x), j = Math.min(y.length - 1, i + 1);
  return y[i] + (y[j] - y[i]) * (x - i);
}

/** Chronologie électrique précalculée depuis les événements de conduction. */
export function heartTimeline(r) {
  const iv = r.intervals, beats = [], tracks = [], blocks = [], avGlow = [];
  let prev = null;
  for (const b of r.ventricular) {
    const w = (b.wide ? WIDE_QRS : 1) * iv.qrs;
    const q0 = b.t - w / 2;
    const rr = prev === null ? 60 / r.hr : b.t - prev;
    prev = b.t;
    const qt = iv.qt * Math.cbrt(95 / Math.max(60 / rr, 30)) + (b.wide ? 0.5 * (w - iv.qrs) : 0);
    const supra = !b.wide && (b.origin === 'sinus' || b.origin === 'junctional');
    const ev = { ...b, w, q0, tEnd: q0 + qt, supra };
    beats.push(ev);
    if (supra) {
      const his0 = q0 - iv.hv;
      avGlow.push({ t0: b.t_a !== null && b.t_a !== undefined ? b.t_a + 0.035 : his0 - 0.03, t1: his0 });
      tracks.push({ p: 'his', t0: his0, t1: q0 });
      tracks.push({ p: 'rbb', t0: q0, t1: q0 + 0.35 * w }, { p: 'lbb', t0: q0, t1: q0 + 0.35 * w });
      tracks.push({ p: 'purk_r', t0: q0 + 0.35 * w, t1: q0 + 0.75 * w }, { p: 'purk_l', t0: q0 + 0.35 * w, t1: q0 + 0.8 * w });
    }
  }
  const conducted = new Set(r.ventricular.filter((b) => b.t_a !== null && b.t_a !== undefined).map((b) => b.t_a.toFixed(6)));
  const atria = r.atrial.map((a, i) => ({ ...a, i, conducted: conducted.has(a.t.toFixed(6)) }));
  for (const a of atria) {
    if (!a.organized) continue;
    tracks.push({ p: 'internodal', t0: a.t, t1: a.t + 0.035 }, { p: 'bachmann', t0: a.t, t1: a.t + 0.04 });
    if (!a.conducted) blocks.push({ t0: a.t + 0.035, t1: a.t + 0.3 });
  }
  return { beats, atria, tracks, blocks, avGlow };
}

const last = (arr, t, key) => { let x = null; for (const e of arr) { if (e[key] <= t) x = e; else break; } return x; };

/** Monte la vue dans `v` et lance l'animation ; `H` garde l'état (instant, vitesse, lecture). */
export function mountHeart(v, r, H, ui) {
  const tl = heartTimeline(r);
  const dur = r.duration_s;
  const vmax = { lv: Math.max(...r.volumes.lv), rv: Math.max(...r.volumes.rv) };
  const arange = {};
  for (const k of ['la', 'ra']) arange[k] = [Math.min(...r.volumes[k]), Math.max(...r.volumes[k])];
  const chamber = (k) => `<g id="h-${k}"><clipPath id="h-cp-${k}"><path d="${SHAPES[k].d}"/></clipPath>
      <path d="${SHAPES[k].d}" fill="${k[1] === 'a' ? REST_A : REST_V}" stroke="${WALL}" stroke-width="${k === 'lv' ? 7 : 4}" stroke-linejoin="round"/>
      <g clip-path="url(#h-cp-${k})"><circle id="h-w-${k}" r="0" fill="${ACTIVE}" opacity="0"/><g id="h-f-${k}"></g></g>
      ${Object.keys(PATHS).filter((p) => PATH_OF[p] === k).map(pathEl).join('')}</g>`;
  const pathEl = (k) => `<path id="h-p-${k}" d="${PATHS[k]}" fill="none" stroke="${CONDUCT}" stroke-width="2.2" stroke-linecap="round" opacity="0.55"/>`;
  const sw = (lab, val) => `<button class="${H.speed === val ? 'on' : ''}" data-speed="${val}">${lab}</button>`;
  v.innerHTML = `<div class="toolbar"><div class="row" style="gap:18px">
      <button class="primary" id="h-play">${H.playing ? '❚❚ Pause' : '▶ Lecture'}</button>
      <div class="group"><span class="lab">Vitesse</span>${sw('×1', 1)}${sw('×0,5', 0.5)}${sw('×0,25', 0.25)}${sw('×0,1', 0.1)}</div></div>
    <div class="sliders"><label><span>Instant : <b id="h-tl">${ui.fmt(H.t, 2)}</b> s / ${ui.fmt(dur, 0)} s</span>
      <input id="h-t" type="range" min="0" max="${dur}" step="0.005" value="${H.t}"></label></div>
    <div class="summary" id="h-status">&nbsp;</div></div>
    <div class="heart-grid">
      <div class="card"><h3>Contraction et influx électrique</h3>
        <svg id="h-svg" viewBox="40 20 340 340" role="img" aria-label="Schéma animé du cœur : quatre cavités et tissu de conduction">
          <text x="112" y="32" class="h-lab">OD</text><text x="300" y="32" class="h-lab">OG</text>
          <text x="62" y="320" class="h-lab">VD</text><text x="362" y="320" class="h-lab">VG</text>
          ${chamber('ra')}${chamber('la')}${chamber('rv')}${chamber('lv')}
          ${pathEl('his')}
          <circle id="h-sa" cx="${SA[0]}" cy="${SA[1]}" r="6" fill="${CONDUCT}" stroke="#7a5c00"/>
          <circle id="h-avn" cx="${AVN[0]}" cy="${AVN[1]}" r="6" fill="${CONDUCT}" stroke="#7a5c00"/>
          <g id="h-block" opacity="0"><path d="M${AVN[0] - 9},${AVN[1] - 9} l18,18 m0,-18 l-18,18" stroke="${COLORS.cursor}" stroke-width="4" stroke-linecap="round"/></g>
          <g id="h-dots"></g>
          <circle id="h-focus" r="7" fill="none" stroke="${COLORS.cursor}" stroke-width="3" opacity="0"/>
          <text x="${SA[0] + 10}" y="${SA[1] - 8}" class="h-small">nœud sinusal</text>
          <text x="${AVN[0] + 10}" y="${AVN[1] + 4}" class="h-small">nœud AV</text>
        </svg>
        <div class="legend"><span><i style="background:${ACTIVE}"></i>myocarde dépolarisé</span><span><i style="background:${CONDUCT}"></i>tissu de conduction et influx</span><span><i style="background:${COLORS.cursor}"></i>blocage, foyer ectopique</span></div>
      </div>
      <div class="col">
        ${ui.card('ECG, dérivation II (fenêtre de 3 s)', ui.canvas('h-ecg', 120))}
        ${ui.card('Volumes ventriculaires (mL)', ui.canvas('h-vol', 120) + ui.legend([['VG', COLORS.lv], ['VD', COLORS.pa]]))}
        ${ui.card('Pressions (mmHg)', ui.canvas('h-pr', 120) + ui.legend([['Aorte', COLORS.ao], ['VG', COLORS.lv], ['OG', COLORS.la]]))}
        <div class="note">Volumes et pressions : modèle à six compartiments simulé battement par battement sur la séquence de conduction de l'ECG. La taille des ventricules suit la racine cubique du volume ; celle des oreillettes est amplifiée (leur volume inclut les veines, dont la variation relative est faible). Les trajets de l'influx sont schématiques ; leurs instants (PR, HV, QRS, QT corrigé) sont ceux du patient. Usage pédagogique uniquement.</div>
      </div></div>`;
  const svg = v.querySelector('#h-svg');
  const el = (id) => svg.querySelector('#' + id);
  const pathLen = {};
  for (const k of Object.keys(PATHS)) pathLen[k] = el('h-p-' + k).getTotalLength();
  const NS = 'http://www.w3.org/2000/svg';
  // Point du schéma au repos → position à l'écran, la cavité étant contractée d'un facteur s.
  const moved = (k, x, y, sc) => {
    if (!k) return [x, y];
    const [cx, cy] = SHAPES[k].c, s = sc[k];
    return [cx + s * (x - cx), cy + s * (y - cy)];
  };

  const draw = () => {
    const t = H.t;
    // --- contraction
    const scale = {};
    for (const k of ['lv', 'rv', 'la', 'ra']) {
      const vol = sample(r.volumes[k], r.fs, t);
      const s = k[1] === 'v' ? Math.cbrt(vol / vmax[k]) : 0.84 + 0.16 * clamp01((vol - arange[k][0]) / (arange[k][1] - arange[k][0] || 1));
      scale[k] = s;
      const [cx, cy] = SHAPES[k].c;
      el('h-' + k).setAttribute('transform', `translate(${cx} ${cy}) scale(${s.toFixed(4)}) translate(${-cx} ${-cy})`);
    }
    // --- oreillettes : onde depuis le nœud sinusal, ou ondelettes chaotiques en FA
    const a = last(tl.atria, t, 't');
    for (const k of ['ra', 'la']) {
      const wv = el('h-w-' + k), f = el('h-f-' + k);
      f.replaceChildren();
      wv.setAttribute('opacity', '0');
      if (!a) continue;
      const dt = t - a.t;
      if (a.organized) {
        const op = dt < P_DUR ? 0.9 : 0.9 * (1 - clamp01((dt - P_DUR) / A_REPOL));
        if (op <= 0) continue;
        wv.setAttribute('cx', SA[0]); wv.setAttribute('cy', SA[1]);
        wv.setAttribute('r', (200 * clamp01(dt / P_DUR)).toFixed(1));
        wv.setAttribute('opacity', op.toFixed(3));
      } else {
        // FA : ondelettes désordonnées, renouvelées à chaque activation (positions pseudo-aléatoires)
        const [cx, cy] = SHAPES[k].c;
        for (let j = 0; j < 4; j++) {
          const h = Math.sin((a.i + 1) * 12.9898 + j * 78.233 + (k === 'la' ? 3.1 : 0)) * 43758.5453;
          const u = h - Math.floor(h), w2 = (h * 7.13) - Math.floor(h * 7.13);
          const c = document.createElementNS(NS, 'circle');
          c.setAttribute('cx', cx - 45 + 90 * u); c.setAttribute('cy', cy - 40 + 80 * w2);
          c.setAttribute('r', (6 + 22 * clamp01(dt / 0.06)).toFixed(1));
          c.setAttribute('fill', ACTIVE); c.setAttribute('opacity', (0.75 * (1 - clamp01(dt / 0.12))).toFixed(3));
          f.appendChild(c);
        }
      }
    }
    // --- ventricules : onde depuis l'origine du battement, puis repolarisation jusqu'à la fin de T
    const b = last(tl.beats, t, 'q0');
    const focus = el('h-focus');
    focus.setAttribute('opacity', '0');
    for (const k of ['rv', 'lv']) {
      const wv = el('h-w-' + k);
      wv.setAttribute('opacity', '0');
      if (!b || t > b.tEnd) continue;
      const [ox, oy] = V_ORIGIN[b.origin] || V_ORIGIN.sinus;
      const dt = t - b.q0;
      wv.setAttribute('cx', ox); wv.setAttribute('cy', oy);
      wv.setAttribute('r', (260 * clamp01(dt / b.w)).toFixed(1));
      const op = 0.9 * (1 - clamp01((t - (b.tEnd - 0.1)) / 0.1));
      wv.setAttribute('opacity', op.toFixed(3));
      if (!b.supra && dt < b.w) {
        const [fx, fy] = moved(ox > 210 ? 'lv' : 'rv', ox, oy, scale);
        focus.setAttribute('cx', fx); focus.setAttribute('cy', fy); focus.setAttribute('opacity', '1');
      }
    }
    // --- tissu de conduction : influx le long des voies, nœud AV, blocage
    const dots = el('h-dots');
    dots.replaceChildren();
    for (const k of Object.keys(PATHS)) { const p = el('h-p-' + k); p.setAttribute('opacity', '0.55'); p.setAttribute('stroke-width', '2.2'); }
    for (const tr of tl.tracks) {
      if (t < tr.t0 || t > tr.t1 + 0.04) continue;
      const p = el('h-p-' + tr.p);
      p.setAttribute('opacity', '1'); p.setAttribute('stroke-width', '4');
      if (t > tr.t1) continue;
      const pt = p.getPointAtLength(pathLen[tr.p] * clamp01((t - tr.t0) / (tr.t1 - tr.t0 || 1)));
      const [px, py] = moved(PATH_OF[tr.p], pt.x, pt.y, scale);
      const c = document.createElementNS(NS, 'circle');
      c.setAttribute('cx', px); c.setAttribute('cy', py); c.setAttribute('r', '5.5');
      c.setAttribute('fill', '#fff7c2'); c.setAttribute('stroke', '#b8860b'); c.setAttribute('stroke-width', '2');
      dots.appendChild(c);
    }
    const sa = a && a.organized && t - a.t < 0.03;
    el('h-sa').setAttribute('r', sa ? '9' : '6');
    const [sx, sy] = moved('ra', SA[0], SA[1], scale);
    el('h-sa').setAttribute('cx', sx); el('h-sa').setAttribute('cy', sy);
    const glow = tl.avGlow.some((g) => t >= g.t0 && t <= g.t1);
    el('h-avn').setAttribute('r', glow ? '9' : '6');
    el('h-avn').setAttribute('fill', glow ? '#ffd84a' : CONDUCT);
    el('h-block').setAttribute('opacity', tl.blocks.some((x) => t >= x.t0 && t <= x.t1) ? '1' : '0');
    // --- état courant
    const blocked = tl.blocks.some((x) => t >= x.t0 && t <= x.t1);
    const vol = sample(r.volumes.lv, r.fs, t), plv = sample(r.pressures.p_lv, r.fs, t);
    const parts = [];
    const fa = a && !a.organized;
    if (b) {
      const lab = fa && b.origin === 'sinus' ? 'battement conduit depuis les oreillettes fibrillantes' : ORIGIN_FR[b.origin] || b.origin;
      parts.push(`Dernier battement : ${lab}${b.pr && !fa && b.supra ? `, PR ${ui.fmt(b.pr * 1000, 0)} ms` : ''}`);
    }
    if (blocked) parts.push('onde P bloquée au nœud AV');
    if (fa) parts.push('oreillettes en fibrillation');
    if (!tl.atria.length) parts.push('aucune activation atriale (arrêt atrial)');
    parts.push(`VG ${ui.fmt(vol, 0)} mL, ${ui.fmt(plv, 0)} mmHg`);
    v.querySelector('#h-status').textContent = parts.join(' · ');
    v.querySelector('#h-tl').textContent = ui.fmt(t, 2);
    v.querySelector('#h-t').value = t;
    // --- tracés synchronisés
    const x0 = Math.max(0, Math.min(dur - 3, t - 1.5)), x1 = x0 + Math.min(3, dur);
    timeChart(v.querySelector('#h-ecg'), { series: [{ y: r.ecg, dt: 1 / r.ecg_fs, color: COLORS.ecg }], x0, x1, cursor: t, yMin: ui.ecgMin, yMax: ui.ecgMax });
    timeChart(v.querySelector('#h-vol'), { series: [{ y: r.volumes.lv, dt: 1 / r.fs, color: COLORS.lv }, { y: r.volumes.rv, dt: 1 / r.fs, color: COLORS.pa }], x0, x1, cursor: t, yMin: 0 });
    const pr = r.pressures;
    timeChart(v.querySelector('#h-pr'), { series: [pr.p_ao, pr.p_lv, pr.p_la].map((y, i) => ({ y, dt: 1 / r.fs, color: [COLORS.ao, COLORS.lv, COLORS.la][i] })), x0, x1, cursor: t, yMin: 0 });
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
  v.querySelector('#h-play').onclick = (e) => {
    H.playing = !H.playing;
    e.target.textContent = H.playing ? '❚❚ Pause' : '▶ Lecture';
    if (H.playing) start();
  };
  for (const btn of v.querySelectorAll('button[data-speed]')) btn.onclick = () => {
    H.speed = +btn.dataset.speed;
    for (const o of v.querySelectorAll('button[data-speed]')) o.classList.toggle('on', o === btn);
  };
  v.querySelector('#h-t').oninput = (e) => { H.t = +e.target.value; if (!H.playing) draw(); };
  draw();
  if (H.playing) start();
}
