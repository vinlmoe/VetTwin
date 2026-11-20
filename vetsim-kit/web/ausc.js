// Lecteur d'auscultation de la salle (ausc.html) : joue en boucle, sur la sortie audio reliée au
// haut-parleur du mannequin, les bruits du cœur du patient de la régie, au site où le
// stéthoscope est posé (boîtier stéthoscope) et à la FC courante de la session d'anesthésie.
// Ce poste exécute son propre moteur (API pcg) ; la régie ne lui transmet que le patient, le
// site et la FC. Usage pédagogique uniquement.
import { Controller } from './engine.js';
import { Room, roomConfig } from './room.js';

const $ = (id) => document.getElementById(id);
const fmt = (v, d = 0) => (v === undefined || v === null || Number.isNaN(v) ? '—' : Number(v).toFixed(d).replace('.', ','));
const HR_STEP = 3;      // nouvelle synthèse quand la FC change d'au moins 3 /min (réglage d'usage)
const LOOP_S = 8;       // durée de la boucle synthétisée (s)
const FADE_S = 0.4;     // fondu entre deux boucles (s)

const S = {
  ctl: null, ac: null, gain: null, src: null, srcGain: null,
  patient: null, site: null, manual: false, hr: null, dead: false, valid: false,
  key: null, busy: false, link: { connected: false, regie: false }, info: '',
};

function status(text, detail = '') { $('state').textContent = text; $('detail').textContent = detail; }

function header() {
  const l = $('link'), st = S.link;
  l.className = 'link ' + (st.connected && st.regie ? 'ok' : 'bad');
  l.textContent = !st.connected ? 'relais déconnecté' : !st.regie ? 'régie absente' : 'en ligne';
}

function stopSound() {
  if (!S.src) return;
  const t = S.ac.currentTime, g = S.srcGain, src = S.src;
  g.gain.setValueAtTime(g.gain.value, t); g.gain.linearRampToValueAtTime(0, t + FADE_S);
  src.stop(t + FADE_S + 0.05);
  S.src = null; S.srcGain = null;
}

function play(samples, fs) {
  const buf = S.ac.createBuffer(1, samples.length, fs);
  buf.copyToChannel(samples, 0);
  const src = S.ac.createBufferSource(); src.buffer = buf; src.loop = true;
  const g = S.ac.createGain(); g.gain.value = 0;
  src.connect(g); g.connect(S.gain);
  const t = S.ac.currentTime;
  stopSound();
  src.start(t); g.gain.linearRampToValueAtTime(1, t + FADE_S);
  S.src = src; S.srcGain = g;
}

/** Recalcule le son si le patient, le site ou la FC ont changé. */
function update() {
  if (!S.ctl || !S.ac) return;
  const site = S.site;
  if (!site) { stopSound(); S.key = null; status('Stéthoscope non posé', S.info); return; }
  if (!S.patient) { stopSound(); S.key = null; status('En attente du patient de la régie'); return; }
  if (S.dead) { stopSound(); S.key = null; status(`${site} : aucun bruit cardiaque`, 'patient décédé'); return; }
  if (!S.valid && !S.manual) { stopSound(); S.key = null; status('En attente de la session'); return; }
  const hr = S.hr;
  if (!(hr > 20 && hr < 300)) { stopSound(); S.key = null; status(`${site} : FC ${fmt(hr)} /min`, 'hors du domaine du synthétiseur (20 à 300 /min) : silence'); return; }
  const hrq = Math.round(hr / HR_STEP) * HR_STEP;
  const key = JSON.stringify([S.patient, site, hrq]);
  if (key === S.key || S.busy) return;
  S.busy = true;
  setTimeout(() => {
    try {
      const inp = { bw: S.patient.bw, disease: S.patient.disease, stage: S.patient.stage, seed: S.patient.seed || 1 };
      const r = S.ctl.auscultate(inp, hrq, site, LOOP_S);
      play(r.samples, r.fs);
      S.key = key;
      status(`${site === 'apex' ? 'Apex' : 'Base'} · FC ${fmt(hrq)} /min`, r.murmur && r.murmur !== 'aucun' ? `souffle ${r.murmur}` : 'sans souffle');
    } catch (e) { status('Erreur du moteur', e.message); }
    S.busy = false;
    update();
  }, 0);
}

async function listSinks() {
  if (!navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices || !(S.ac && S.ac.setSinkId)) {
    $('sink').disabled = true; $('sink').title = 'choix de la sortie non pris en charge par ce navigateur'; return;
  }
  const devs = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'audiooutput');
  $('sink').innerHTML = '<option value="">sortie par défaut</option>' + devs.filter((d) => d.deviceId !== 'default')
    .map((d, i) => `<option value="${d.deviceId}">${d.label || `sortie ${i + 1}`}</option>`).join('');
  const want = new URLSearchParams(location.search).get('sink');
  if (want) { const o = [...$('sink').options].find((x) => x.textContent.includes(want)); if (o) { $('sink').value = o.value; await S.ac.setSinkId(o.value); } }
}

async function start() {
  $('go').style.display = 'none';
  S.ac = new (window.AudioContext || window.webkitAudioContext)();
  S.gain = S.ac.createGain(); S.gain.gain.value = +$('gain').value; S.gain.connect(S.ac.destination);
  $('gain').oninput = () => { S.gain.gain.value = +$('gain').value; };
  $('sink').onchange = async () => { try { await S.ac.setSinkId($('sink').value); } catch (e) { status('Sortie audio refusée', e.message); } };
  listSinks().catch(() => {});
  status('Chargement du moteur…');
  try { S.ctl = await Controller.create(); } catch (e) { status('Moteur indisponible', e.message); return; }
  for (const b of document.querySelectorAll('[data-site]')) b.onclick = () => {
    S.manual = !!b.dataset.site; S.site = b.dataset.site || null;
    if (S.manual && S.hr === null) S.hr = 90; // essai sans régie : FC d'usage
    update();
  };
  const cfg = roomConfig() || { url: `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/ws` };
  new Room(cfg, 'ausc', {
    onMessage: (m) => {
      if (m.patient) S.patient = m.patient;
      if (m.kind === 'steth') { S.site = m.site; S.manual = false; }
      if (m.kind === 'snapshot') {
        const vi = m.snap && m.snap.vitals;
        S.dead = !!(m.snap && m.snap.dead); S.valid = true;
        if (vi && Number.isFinite(vi.hr)) S.hr = vi.hr;
      }
      update();
    },
    onStatus: (st) => { S.link = st; if (!st.regie) S.valid = false; header(); update(); },
  });
  status('Stéthoscope non posé');
}

$('go').onclick = start;
if (new URLSearchParams(location.search).get('autostart') === '1') start();
