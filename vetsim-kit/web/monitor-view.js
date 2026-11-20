// Moniteur d'anesthésie défilant, partagé par l'onglet Anesthésie (régie) et la page
// monitor.html (écran de la salle de simulation). Usage pédagogique uniquement.
//
// Les tracés (ECG D II, pression artérielle) viennent du moteur par segments de quelques
// secondes ; ils sont découpés aux ondes R puis mis bout à bout et défilent à vitesse réelle
// (balayage de 6 s), indépendamment de l'accélération de la simulation, comme un moniteur.
//
// Capteurs posés (`probes`, boîtier des capteurs de la salle) : une voie dont le capteur n'est
// pas posé est affichée sans signal ni valeur. `probes = null` : tout est affiché.
import { setup } from './charts.js';

export const SWEEP_S = 6, GAP_S = 0.25;

export const ALARM_FR = {
  hypotension: 'Hypotension (PAM < 60)', hypertension: 'Hypertension', bradycardia: 'Bradycardie', tachycardia: 'Tachycardie',
  desaturation: 'Désaturation (SpO₂ < 90 %)', apnea: 'Apnée', hypercapnia: 'Hypercapnie (PaCO₂ > 60)', hypothermia: 'Hypothermie (< 36 °C)',
  movement: 'Mouvement', cardiac_arrest: 'Arrêt cardiorespiratoire', tissue_hypoxia: 'Hypoxie tissulaire (dette d\u2019O₂)',
};
export const DEATH_FR = {
  cardiac_arrest: 'arrêt cardiaque', circulatory_collapse: 'effondrement circulatoire', irreversible_shock: 'choc irréversible',
  oxygen_debt: "dette d'oxygène", hypoxic_arrest: 'arrêt cardiaque hypoxique', no_steady_state: 'aucun état viable', vicious_circle: 'spirale de décompensation',
};


/** Voies du moniteur et capteur requis. */
export const PROBES = [
  ['ecg', 'ECG (électrodes)'], ['spo2', 'Oxymètre (SpO₂)'], ['nibp', 'Brassard (PNI)'],
  ['art', 'Cathéter artériel (PA invasive)'], ['capno', 'Capnographe (EtCO₂, gaz)'], ['temp', 'Sonde thermique'],
];

/** Capteurs nécessaires à chaque alarme ; une alarme sans capteur posé n'est pas affichée. */
const ALARM_NEEDS = {
  hypotension: ['nibp', 'art'], hypertension: ['nibp', 'art'], bradycardia: ['ecg', 'spo2', 'art'], tachycardia: ['ecg', 'spo2', 'art'],
  desaturation: ['spo2'], apnea: ['capno'], hypercapnia: ['capno'], hypothermia: ['temp'], cardiac_arrest: ['ecg', 'spo2', 'art'],
  tissue_hypoxia: [], movement: null,
};

export const has = (probes, k) => !probes || probes[k] !== false;

/** Alarmes du moteur visibles avec les capteurs posés (`movement` : constat visuel, toujours). */
export function visibleAlarms(alarms, probes) {
  if (!probes) return alarms;
  return alarms.filter((a) => {
    const need = ALARM_NEEDS[a];
    if (need === undefined || need === null) return true;
    return need.some((k) => has(probes, k));
  });
}

function rPeaks(ecg, fs) {
  let mx = -Infinity; for (const x of ecg) if (x > mx) mx = x;
  const thr = 0.5 * mx, out = [], refr = Math.round(0.2 * fs);
  for (let i = 1; i < ecg.length - 1; i++) {
    if (ecg[i] > thr && ecg[i] >= ecg[i - 1] && ecg[i] > ecg[i + 1] && (!out.length || i - out[out.length - 1] > refr)) out.push(i);
  }
  return out;
}

export class MonitorView {
  constructor() { this.M = null; this.lastAlarm = 0; this.audio = null; this.sound = false; }

  reset() { this.M = null; }

  /** Nombre de segments en attente (la régie ne redemande des tracés que s'il n'y en a plus). */
  get pending() { return this.M ? this.M.pending.length : 0; }

  setSound(on) {
    this.sound = on;
    if (on && !this.audio) { try { this.audio = new (window.AudioContext || window.webkitAudioContext)(); } catch (e) { this.sound = false; } }
    if (this.audio && this.audio.state === 'suspended') this.audio.resume();
    return this.sound;
  }

  /** Segment reçu du moteur (`ecg`, `ecg_fs`, `pressures`) : coupé aux ondes R (battements entiers). */
  feed(r) {
    const fs = r.ecg_fs, ecg = r.ecg;
    const pr = r.pressures, k = pr ? Math.max(1, Math.round(fs / pr.fs)) : 1;
    const art = ecg.map((_, i) => (pr ? pr.ao[Math.min(pr.ao.length - 1, Math.floor(i / k))] : NaN));
    const pk = rPeaks(ecg, fs);
    let seg;
    if (pk.length >= 2) {
      const a = pk[0], b = pk[pk.length - 1];
      seg = { ecg: ecg.slice(a, b), art: art.slice(a, b), peaks: pk.slice(0, -1).map((i) => i - a) };
    } else seg = { ecg, art, peaks: pk };
    if (!this.M) this.M = { fs, ecg: [], art: [], peaks: [], base: 0, pos: 0, pending: [], last: null, t: performance.now() };
    this.M.pending.push(seg);
    if (this.M.pending.length > 2) this.M.pending.shift();
  }

  appendSeg() {
    const M = this.M;
    const seg = M.pending.length ? M.pending.shift() : M.last;
    if (!seg) return false;
    M.last = seg;
    const off = M.base + M.ecg.length;
    for (let i = 0; i < seg.ecg.length; i++) { M.ecg.push(seg.ecg[i]); M.art.push(seg.art[i]); }
    for (const p of seg.peaks) M.peaks.push(off + p);
    return seg.ecg.length > 0;
  }

  beep(freq, dur, type = 'sine', vol = 0.15, delay = 0) {
    const ac = this.audio; if (!ac || !this.sound) return;
    const t0 = ac.currentTime + delay, o = ac.createOscillator(), g = ac.createGain();
    o.type = type; o.frequency.value = freq;
    g.gain.setValueAtTime(0, t0); g.gain.linearRampToValueAtTime(vol, t0 + 0.005); g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
    o.connect(g); g.connect(ac.destination); o.start(t0); o.stop(t0 + dur + 0.02);
  }

  /** Une image : `c = {ecg, art, cap}` (canvas), `s` instantané du moteur, `probes` capteurs posés. */
  draw(now, s, c, probes = null) {
    if (!s || !c.ecg) return;
    const M = this.M;
    const fs = M ? M.fs : 250;
    // tête de lecture à vitesse réelle
    let prev = 0, P = 0;
    if (M) {
      const dt = Math.min(0.25, (now - M.t) / 1000); M.t = now;
      prev = M.base + M.pos; M.pos += dt * fs;
      while (M.pos >= M.ecg.length && this.appendSeg()) { /* rallonge par battements entiers */ }
      if (M.pos >= M.ecg.length) M.pos = M.ecg.length - 1;
      const keep = Math.ceil(SWEEP_S * fs) + 10, drop = Math.floor(M.pos) - keep;
      if (drop > 0) { M.ecg.splice(0, drop); M.art.splice(0, drop); M.base += drop; M.pos -= drop; M.peaks = M.peaks.filter((p) => p >= M.base); }
      P = M.base + M.pos;
      // bip à chaque QRS franchi par la tête de lecture ; tonalité selon la SpO2 (fixe sans oxymètre)
      const pulseSrc = has(probes, 'ecg') || has(probes, 'spo2') || has(probes, 'art');
      if (!s.dead && pulseSrc) for (const pk of M.peaks) if (pk > prev && pk <= P) {
        const spo2 = has(probes, 'spo2') && s.vitals ? s.vitals.spo2 : 90;
        this.beep(Math.max(250, Math.min(900, 300 + 6 * (spo2 - 60))), 0.06);
      }
    } else P = (now / 1000) * fs;
    // alarmes sonores (toutes les 4 s) ; arrêt : tonalité continue répétée
    const alarms = visibleAlarms(s.alarms, probes);
    const deadSeen = s.dead && (has(probes, 'ecg') || has(probes, 'spo2') || has(probes, 'art'));
    if (this.sound && (alarms.length || deadSeen) && now - this.lastAlarm > (deadSeen ? 1000 : 4000)) {
      this.lastAlarm = now;
      if (deadSeen) this.beep(960, 0.9, 'square', 0.08);
      else [0, 0.18, 0.36].forEach((d, i) => this.beep(i === 1 ? 740 : 880, 0.12, 'square', 0.07, d));
    }
    const sweep = (cv, yOf, lo, hi, color, left, off) => {
      const { ctx, w, h } = setup(cv);
      if (off) {
        ctx.fillStyle = 'rgba(255,255,255,0.45)'; ctx.font = '12px system-ui';
        ctx.fillText(off, left + 8, h / 2 + 4); return;
      }
      const W = w - left, n = Math.floor((SWEEP_S - GAP_S) * fs), Y = (v) => 4 + (1 - (v - lo) / (hi - lo)) * (h - 8);
      ctx.fillStyle = 'rgba(255,255,255,0.35)'; ctx.font = '10px system-ui';
      if (left) { ctx.fillText(String(hi), 2, 10); ctx.fillText(String(lo), 2, h - 3); }
      ctx.strokeStyle = color; ctx.lineWidth = 1.6; ctx.beginPath();
      let lastX = null;
      for (let k = n; k >= 0; k--) {
        const ia = P - k, v = yOf(ia);
        if (!Number.isFinite(v)) { lastX = null; continue; }
        const x = left + (((ia / fs) % SWEEP_S) / SWEEP_S) * W;
        if (lastX === null || x < lastX) ctx.moveTo(x, Y(v)); else ctx.lineTo(x, Y(v));
        lastX = x;
      }
      ctx.stroke();
    };
    const at = (arr, ia) => { if (!M) return NaN; const i = Math.floor(ia - M.base); return i >= 0 && i < arr.length ? arr[i] : NaN; };
    const flat = s.dead || !M;
    sweep(c.ecg, (ia) => (flat ? 0 : at(M.ecg, ia)), -1.5, 2.5, '#35e08a', 0, has(probes, 'ecg') ? null : 'ECG : électrodes non posées');
    sweep(c.art, (ia) => (s.dead ? 0 : at(M ? M.art : [], ia)), 0, 200, '#ff6b6b', 26, has(probes, 'art') ? null : 'PA invasive : pas de cathéter artériel');
    // capnogramme : plateau à l'EtCO2, au rythme respiratoire, sur le même balayage
    const per = s.rr > 0 ? 60 / s.rr : Infinity;
    sweep(c.cap, (ia) => {
      if (!Number.isFinite(per) || s.dead) return 0;
      const ph = ((ia / fs) % per) / per, e = s.etco2;
      return ph < 0.4 ? 0 : ph < 0.45 ? e * (ph - 0.4) / 0.05 : ph < 0.9 ? e * (0.92 + 0.08 * (ph - 0.45) / 0.45) : e * (1 - (ph - 0.9) / 0.1);
    }, 0, 60, '#f2d45c', 26, has(probes, 'capno') ? null : 'Capnographe non branché');
  }
}
