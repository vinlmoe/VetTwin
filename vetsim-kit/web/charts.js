// Tracés sur canvas pour VetTwins (sans dépendance). Toutes les fonctions prennent un
// <canvas> et dessinent à sa taille CSS (net sur les écrans haute densité).

export const COLORS = {
  ink: '#1b2a35', muted: '#5b6b77', grid: '#e6eaee', paper: '#fbf7f2', paperGrid: '#f0d9cf',
  ecg: '#0b7a55', ao: '#c0392b', lv: '#2563a8', la: '#b7791f', pa: '#7a3e9d', accent: '#0b5e7a', cursor: '#e0465a',
  drugs: ['#2563a8', '#c0392b', '#0b7a55', '#7a3e9d', '#b7791f'],
};

/** Prépare le contexte 2D à la taille affichée du canvas. */
export function setup(canvas) {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth, h = canvas.clientHeight;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr);
  }
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  return { ctx, w, h };
}

const fmt = (v, d = 2) => (Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(d)).replace('.', ',');

/** Libellé d'instant de traitement : « J2 14 h ». */
export function dayLabel(h) {
  const d = Math.floor(h / 24), r = h - 24 * d, hh = Math.floor(r), mm = Math.round((r - hh) * 60);
  return `J${d} ${hh} h` + (mm > 0 && mm < 60 ? ` ${String(mm).padStart(2, '0')}` : '');
}

/**
 * Courbes sur un axe de temps.
 * opts : series [{y: Float32Array|number[], color, t?: number[] (sinon dt/t0), dt, t0}],
 *        x0, x1 (fenêtre), yMin, yMax, cursor, marks [{t, color}], refLines [y], paper (ECG),
 *        ticks ('s' | 'treatment'), points (bool), left (marge),
 *        fills [{t, lo, hi, color}] (bandes d'incertitude sous les courbes) ;
 *        série : dash ([motif]) pour un tracé pointillé
 */
export function timeChart(canvas, opts) {
  // fond inchangé (même clé, même taille) : on recolle l'image mise en cache et on ne trace que le curseur
  if (opts.cacheKey !== undefined && reuseBase(canvas, opts.cacheKey)) {
    const left = opts.left ?? 44, x0 = opts.x0, x1 = opts.x1;
    drawCursor(canvas, opts.cursor, left, x0, x1, canvas.clientHeight - 16);
    return { left, W: canvas.clientWidth - left, x0, x1 };
  }
  const out = drawTimeChart(canvas, { ...opts, cursor: undefined });
  if (opts.cacheKey !== undefined) storeBase(canvas, opts.cacheKey);
  if (out) drawCursor(canvas, opts.cursor, out.left, opts.x0, opts.x1, canvas.clientHeight - 16);
  return out;
}

/** Clé de cache d'un fond : clé fournie + taille en pixels physiques. */
function baseKey(canvas, key) { return `${key}|${canvas.width}x${canvas.height}`; }

/** Recolle le fond mis en cache si la clé et la taille n'ont pas changé. */
function reuseBase(canvas, key) {
  const c = canvas._base;
  const dpr = window.devicePixelRatio || 1;
  if (!c || canvas.width !== Math.round(canvas.clientWidth * dpr) || canvas.height !== Math.round(canvas.clientHeight * dpr)) return false;
  if (c.key !== baseKey(canvas, key)) return false;
  const ctx = canvas.getContext('2d');
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(c.img, 0, 0);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return true;
}

function storeBase(canvas, key) {
  const c = canvas._base || (canvas._base = { img: document.createElement('canvas') });
  c.img.width = canvas.width; c.img.height = canvas.height;
  const g = c.img.getContext('2d');
  g.clearRect(0, 0, canvas.width, canvas.height);
  g.drawImage(canvas, 0, 0);
  c.key = baseKey(canvas, key);
}

function drawCursor(canvas, cursor, left, x0, x1, H) {
  if (cursor === undefined || cursor === null || cursor < x0 || cursor > x1) return;
  const ctx = canvas.getContext('2d'), W = canvas.clientWidth - left, x = left + ((cursor - x0) / (x1 - x0 || 1)) * W;
  ctx.strokeStyle = COLORS.cursor; ctx.lineWidth = 2.5;
  ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, H); ctx.stroke();
}

function drawTimeChart(canvas, opts) {
  const { ctx, w, h } = setup(canvas);
  const left = opts.left ?? 44, axis = 16, W = w - left, H = h - axis;
  const x0 = opts.x0, x1 = opts.x1, span = x1 - x0 || 1;
  const X = (t) => left + ((t - x0) / span) * W;
  if (opts.paper) {
    ctx.fillStyle = COLORS.paper; ctx.fillRect(left, 0, W, H);
    ctx.strokeStyle = COLORS.paperGrid; ctx.lineWidth = 1;
    const step = W / 40;
    for (let x = left; x < left + W; x += step) { ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, H); ctx.stroke(); }
    for (let y = 0; y < H; y += step) { ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(left + W, y); ctx.stroke(); }
  }
  // échelle verticale commune
  let lo = opts.yMin, hi = opts.yMax;
  if (lo === undefined || hi === undefined) {
    let a = Infinity, b = -Infinity;
    for (const s of opts.series) forVisible(s, x0, x1, (t, v) => { if (v < a) a = v; if (v > b) b = v; });
    for (const r of opts.refLines || []) { a = Math.min(a, r); b = Math.max(b, r); }
    for (const f of opts.fills || []) for (let i = 0; i < f.t.length; i++) {
      if (f.t[i] < x0 || f.t[i] > x1) continue;
      if (isFinite(f.lo[i])) a = Math.min(a, f.lo[i]);
      if (isFinite(f.hi[i])) b = Math.max(b, f.hi[i]);
    }
    if (lo === undefined) lo = a; if (hi === undefined) hi = b;
  }
  if (!isFinite(lo) || !isFinite(hi)) return;
  const ys = hi - lo > 1e-9 ? hi - lo : 1;
  // bandes d'état (décompensation, décès) sous les courbes
  for (const b of opts.bands || []) {
    ctx.fillStyle = b.color;
    ctx.fillRect(X(Math.max(x0, b.t0)), 0, Math.max(1, X(Math.min(x1, b.t1)) - X(Math.max(x0, b.t0))), H);
  }
  const Y = (v) => 4 + (1 - (v - lo) / ys) * (H - 8);
  // graduations
  ctx.font = '10px system-ui, sans-serif'; ctx.fillStyle = COLORS.muted; ctx.strokeStyle = COLORS.grid; ctx.lineWidth = 1;
  const ticks = opts.ticks === 'treatment' ? treatmentTicks(span) : opts.ticks === 'min' ? minuteTicks(span) : secondTicks(span);
  for (let t = Math.ceil(x0 / ticks) * ticks; t <= x1 + 1e-9; t += ticks) {
    const x = X(t);
    ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, H); ctx.stroke();
    const lab = opts.ticks === 'treatment' ? (ticks >= 24 ? `J${Math.round(t / 24)}` : dayLabel(t)) : opts.ticks === 'min' ? `${Math.round(t)} min` : `${fmt(t, ticks < 0.5 ? 2 : 1)} s`;
    if (x + 40 < w) ctx.fillText(lab, x + 2, h - 3);
  }
  // graduations verticales « rondes » (1, 2, 5 × 10ⁿ) dans [lo, hi], avec lignes de grille
  const yt = niceTicks(lo, hi, H < 100 ? 3 : H < 140 ? 4 : 5);
  for (const v of yt.values) {
    const y = Y(v);
    if (!opts.paper) { ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(left + W, y); ctx.stroke(); }
    ctx.fillText(yt.format(v), 2, Math.min(H - 1, Math.max(9, y + 3.5)));
  }
  for (const r of opts.refLines || []) {
    ctx.save(); ctx.setLineDash([6, 6]); ctx.strokeStyle = COLORS.muted;
    ctx.beginPath(); ctx.moveTo(left, Y(r)); ctx.lineTo(left + W, Y(r)); ctx.stroke(); ctx.restore();
  }
  for (const m of opts.marks || []) {
    if (m.t < x0 || m.t > x1) continue;
    ctx.strokeStyle = m.color + '66'; ctx.lineWidth = 3;
    ctx.beginPath(); ctx.moveTo(X(m.t), H - 10); ctx.lineTo(X(m.t), H); ctx.stroke();
  }
  // bandes d'incertitude (Monte-Carlo) : polygone lo → hi, segments interrompus par les trous
  for (const f of opts.fills || []) {
    ctx.fillStyle = f.color;
    let seg = [];
    const flush = () => {
      if (seg.length > 1) {
        ctx.beginPath();
        seg.forEach((i, k) => (k ? ctx.lineTo(X(f.t[i]), Y(f.hi[i])) : ctx.moveTo(X(f.t[i]), Y(f.hi[i]))));
        for (let k = seg.length - 1; k >= 0; k--) ctx.lineTo(X(f.t[seg[k]]), Y(f.lo[seg[k]]));
        ctx.closePath(); ctx.fill();
      }
      seg = [];
    };
    for (let i = 0; i < f.t.length; i++) { if (isFinite(f.lo[i]) && isFinite(f.hi[i])) seg.push(i); else flush(); }
    flush();
  }
  // séries : min/max par colonne si la série est dense
  for (const s of opts.series) {
    ctx.strokeStyle = s.color; ctx.fillStyle = s.color; ctx.lineWidth = s.width ?? 1.6;
    ctx.setLineDash(s.dash || []);
    const n = countVisible(s, x0, x1);
    if (n > 4 * W && !s.t) {
      const cols = Math.max(1, Math.floor(W));
      const mins = new Float32Array(cols).fill(Infinity), maxs = new Float32Array(cols).fill(-Infinity);
      forVisible(s, x0, x1, (t, v) => {
        const c = Math.min(cols - 1, Math.max(0, Math.floor(((t - x0) / span) * cols)));
        if (v < mins[c]) mins[c] = v; if (v > maxs[c]) maxs[c] = v;
      });
      ctx.beginPath();
      for (let c = 0; c < cols; c++) if (isFinite(mins[c])) { ctx.moveTo(left + c + 0.5, Y(maxs[c])); ctx.lineTo(left + c + 0.5, Y(mins[c]) + 0.5); }
      ctx.lineWidth = 1.2; ctx.stroke();
    } else {
      ctx.beginPath(); let first = true;
      forVisible(s, x0, x1, (t, v) => { if (first) { ctx.moveTo(X(t), Y(v)); first = false; } else ctx.lineTo(X(t), Y(v)); });
      ctx.stroke();
      if (opts.points) forVisible(s, x0, x1, (t, v) => { ctx.beginPath(); ctx.arc(X(t), Y(v), 2, 0, 2 * Math.PI); ctx.fill(); });
    }
  }
  ctx.setLineDash([]);
  return { left, W, x0, x1 };
}

/** Graduations de pas 1, 2 ou 5 × 10ⁿ comprises dans [lo, hi] (au moins les bornes si l'écart est nul). */
function niceTicks(lo, hi, n) {
  const span = hi - lo;
  if (!(span > 1e-12)) return { values: [lo], format: (v) => fmt(v) };
  const raw = span / n, p = 10 ** Math.floor(Math.log10(raw)), m = raw / p;
  const step = (m < 1.5 ? 1 : m < 3 ? 2 : m < 7 ? 5 : 10) * p; // pas rond le plus proche du pas brut
  const values = [];
  for (let v = Math.ceil(lo / step - 1e-9) * step; v <= hi + step * 1e-9; v += step) values.push(Math.abs(v) < step * 1e-9 ? 0 : v);
  const d = Math.max(0, -Math.floor(Math.log10(step) + 1e-9));
  return { values, format: (v) => v.toFixed(d).replace('.', ',').replace('-', '−') };
}

function secondTicks(span) { return span <= 1 ? 0.1 : span <= 3 ? 0.25 : span <= 6 ? 0.5 : 1; }
function minuteTicks(span) { return span <= 12 ? 1 : span <= 30 ? 5 : span <= 90 ? 10 : 30; }
function treatmentTicks(span) { return span <= 48 ? 6 : 24; }

function countVisible(s, x0, x1) {
  if (s.t) return s.y.length;
  const dt = s.dt, t0 = s.t0 || 0;
  return Math.max(0, Math.min(s.y.length, Math.floor((x1 - t0) / dt) + 1) - Math.max(0, Math.floor((x0 - t0) / dt)));
}

function forVisible(s, x0, x1, f) {
  // valeurs non finies (patient décédé à cet instant) : ignorées
  if (s.t) { for (let i = 0; i < s.y.length; i++) if (s.t[i] >= x0 && s.t[i] <= x1 && Number.isFinite(s.y[i])) f(s.t[i], s.y[i]); return; }
  const dt = s.dt, t0 = s.t0 || 0;
  const i0 = Math.max(0, Math.floor((x0 - t0) / dt)), i1 = Math.min(s.y.length - 1, Math.ceil((x1 - t0) / dt));
  for (let i = i0; i <= i1; i++) if (Number.isFinite(s.y[i])) f(t0 + i * dt, s.y[i]);
}

/** Spectrogramme : s = {frames, bins, db (Float32Array), hopS, minDb, maxDb}. */
export function spectrogram(canvas, s, x0, x1, cursor) {
  const left = 44, H = canvas.clientHeight - 16;
  // l'image ne dépend que du spectrogramme et de la fenêtre : recalculée seulement s'ils changent
  if (!s.id) s.id = ++spectroIds;
  const key = `spec${s.id}:${x0}:${x1}`;
  if (!reuseBase(canvas, key)) { drawSpectrogram(canvas, s, x0, x1); storeBase(canvas, key); }
  drawCursor(canvas, cursor, left, x0, x1, H);
}
let spectroIds = 0;

function drawSpectrogram(canvas, s, x0, x1) {
  const { ctx, w, h } = setup(canvas);
  const left = 44, axis = 16, W = w - left, H = h - axis;
  if (!s.frames) return;
  const f0 = Math.max(0, Math.floor(x0 / s.hopS)), f1 = Math.min(s.frames, Math.ceil(x1 / s.hopS));
  // putImageData travaille en pixels physiques (sans la transformation d'échelle)
  const dpr = window.devicePixelRatio || 1;
  const img = ctx.createImageData(Math.max(1, Math.floor(W * dpr)), Math.max(1, Math.floor(H * dpr)));
  // palette précalculée (256 niveaux) et bande de fréquence de chaque ligne de pixels
  const lut = spectroLut(), data = new Uint32Array(img.data.buffer), iw = img.width, ih = img.height;
  const rowBin = new Int32Array(ih);
  for (let py = 0; py < ih; py++) rowBin[py] = Math.min(s.bins - 1, Math.floor(((ih - 1 - py) / ih) * s.bins));
  const scale = 255 / (s.maxDb - s.minDb);
  for (let px = 0; px < iw; px++) {
    const base = Math.min(s.frames - 1, f0 + Math.floor((px / iw) * (f1 - f0))) * s.bins;
    for (let py = 0; py < ih; py++) {
      const q = (s.db[base + rowBin[py]] - s.minDb) * scale;
      data[py * iw + px] = lut[q <= 0 ? 0 : q >= 255 ? 255 : q | 0];
    }
  }
  ctx.putImageData(img, Math.round(left * dpr), 0);
  ctx.font = '10px system-ui, sans-serif'; ctx.fillStyle = COLORS.muted;
  ctx.fillText('500 Hz', 2, 10); ctx.fillText('0', 2, H);
  const span = x1 - x0, ticks = secondTicks(span);
  for (let t = Math.ceil(x0 / ticks) * ticks; t <= x1 + 1e-9; t += ticks) {
    const x = left + ((t - x0) / span) * W;
    if (x + 40 < w) ctx.fillText(`${fmt(t, ticks < 0.5 ? 2 : 1)} s`, x + 2, h - 3);
  }
}

/** Palette du spectrogramme en mots RGBA (ordre des octets de la plateforme). */
let lutCache = null;
function spectroLut() {
  if (lutCache) return lutCache;
  const bytes = new Uint8ClampedArray(256 * 4);
  for (let i = 0; i < 256; i++) {
    const v = i / 255;
    bytes[4 * i] = 255 * (0.05 + 0.95 * v); bytes[4 * i + 1] = 255 * 0.9 * v * v;
    bytes[4 * i + 2] = 255 * (0.25 + 0.5 * (1 - v)) * (1 - 0.6 * v); bytes[4 * i + 3] = 255;
  }
  return (lutCache = new Uint32Array(bytes.buffer));
}

/** Spectrogramme par TFCT (Hann, FFT radix 2). */
export function computeSpectrogram(x, fs, nfft = 512, hop = 64, maxHz = 500) {
  const binHz = fs / nfft, bins = Math.min(nfft / 2, Math.floor(maxHz / binHz) + 1);
  const frames = Math.max(0, Math.floor((x.length - nfft) / hop) + 1);
  const win = new Float64Array(nfft).map((_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / nfft));
  const db = new Float32Array(frames * bins), re = new Float64Array(nfft), im = new Float64Array(nfft);
  for (let f = 0; f < frames; f++) {
    for (let i = 0; i < nfft; i++) { re[i] = x[f * hop + i] * win[i]; im[i] = 0; }
    fft(re, im);
    for (let b = 0; b < bins; b++) db[f * bins + b] = 10 * Math.log10(re[b] * re[b] + im[b] * im[b] + 1e-12);
  }
  let maxDb = -Infinity; for (const v of db) if (v > maxDb) maxDb = v;
  return { frames, bins, db, hopS: hop / fs, minDb: maxDb - 60, maxDb };
}

/** Facteurs de rotation e^(−2iπk/n) de la FFT, calculés une fois par taille. */
const twiddles = new Map();
function twiddle(n) {
  if (!twiddles.has(n)) {
    const c = new Float64Array(n / 2), s = new Float64Array(n / 2);
    for (let k = 0; k < n / 2; k++) { c[k] = Math.cos((-2 * Math.PI * k) / n); s[k] = Math.sin((-2 * Math.PI * k) / n); }
    twiddles.set(n, { c, s });
  }
  return twiddles.get(n);
}

function fft(re, im) {
  const n = re.length, tw = twiddle(n);
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1; for (; j & bit; bit >>= 1) j ^= bit; j ^= bit;
    if (i < j) { let t = re[i]; re[i] = re[j]; re[j] = t; t = im[i]; im[i] = im[j]; im[j] = t; }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const half = len >> 1, stride = n / len;
    for (let i = 0; i < n; i += len) for (let k = 0; k < half; k++) {
      const c = tw.c[k * stride], s = tw.s[k * stride], a = i + k, b = a + half;
      const vr = re[b] * c - im[b] * s, vi = re[b] * s + im[b] * c;
      re[b] = re[a] - vr; im[b] = im[a] - vi; re[a] += vr; im[a] += vi;
    }
  }
}
