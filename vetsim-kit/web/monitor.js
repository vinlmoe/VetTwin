// Moniteur de salle (monitor.html) : affiche la session de la régie reçue par le relais.
// N'exécute pas le moteur ; masque les voies dont le capteur n'est pas posé.
// Usage pédagogique uniquement.
import { Room, roomConfig } from './room.js';
// La cause du décès (modèle) n'est pas montrée aux étudiants : le moniteur n'affiche que ce qu'il mesure.
import { MonitorView, has, visibleAlarms, ALARM_FR } from './monitor-view.js';

const $ = (id) => document.getElementById(id);
const fmt = (v, d = 0) => (v === undefined || v === null || Number.isNaN(v) ? '—' : Number(v).toFixed(d).replace('.', ','));
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const clock = (t) => { const m = Math.floor(t / 60), s = Math.floor(t % 60); return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}:${String(s).padStart(2, '0')}`; };

const S = { snap: null, probes: null, patient: null, link: { connected: false, regie: false }, view: new MonitorView(), lastSnapAt: 0 };

function header() {
  const l = $('link'), st = S.link;
  const fresh = performance.now() - S.lastSnapAt < 6000;
  l.className = 'link ' + (st.connected && st.regie && fresh ? 'ok' : 'bad');
  l.textContent = !st.connected ? 'relais déconnecté' : !st.regie ? 'régie absente' : fresh ? 'en ligne' : 'en attente de la régie';
  const p = S.patient;
  $('who').textContent = p ? `chien ${fmt(p.bw, 1)} kg${p.disease ? ` · ${p.disease}${p.stage ? ' ' + p.stage : ''}` : ''}` : '—';
}

function values() {
  const s = S.snap; if (!s) return;
  const pr = S.probes, vi = s.vitals || {};
  $('clock').textContent = clock(s.t_s);
  const alarms = visibleAlarms(s.alarms, pr);
  const deadSeen = s.dead && (has(pr, 'ecg') || has(pr, 'spo2') || has(pr, 'art'));
  $('alarms').innerHTML = (deadSeen ? '<span class="dead">Aucun pouls détecté</span>' : '')
    + alarms.filter((a) => !(s.dead && a === 'cardiac_arrest')).map((a) => `<span>${esc(ALARM_FR[a] || a)}</span>`).join('');
  const gas = s.inhalants.find((i) => i.vaporizer > 0) || s.inhalants.reduce((a, b) => (b.fe > (a ? a.fe : -1) ? b : a), null);
  const dead = !!s.dead;
  const box = (lab, val, unit, cls, on, extra = '') => `<div class="mv ${cls} ${extra} ${on ? '' : 'off'}"><span>${lab}</span><b>${on ? val : '—'}</b><i>${unit}</i></div>`;
  const bp = has(pr, 'art') || has(pr, 'nibp');
  const hrOn = has(pr, 'ecg') || has(pr, 'spo2') || has(pr, 'art');
  $('vals').innerHTML = [
    box(has(pr, 'ecg') ? 'FC (ECG)' : 'FC (pouls)', dead ? '0' : fmt(vi.hr), '/min', 'g', hrOn, 'wide'),
    box(has(pr, 'art') ? 'PA invasive' : 'PNI', dead ? '0/0' : `${fmt(vi.sap)}/${fmt(vi.dap)}`, dead ? '(0) mmHg' : `(${fmt(vi.map)}) mmHg`, 'r', bp, 'wide'),
    box('SpO₂', dead ? '—' : fmt(vi.spo2), '%', 'b', has(pr, 'spo2')),
    box('EtCO₂', dead ? '0' : fmt(s.etco2), 'mmHg', 'y', has(pr, 'capno')),
    box('FR', dead ? '0' : fmt(s.rr), s.controlled ? '/min (VC)' : '/min', 'y', has(pr, 'capno')),
    box('T°', fmt(s.temp_c, 1), '°C', 'w', has(pr, 'temp')),
    gas ? box(`Fi/Fe ${esc(gas.id)}`, `${fmt(gas.fi, 1)}/${fmt(gas.fe, 1)}`, '%', 'w small', has(pr, 'capno')) : box('Agent', '—', '', 'w small', false),
    box('FiO₂', fmt(100 * s.fio2), '%', 'w small', has(pr, 'capno')),
  ].join('');
}

function frame(now) {
  requestAnimationFrame(frame);
  if (S.snap) S.view.draw(now, S.snap, { ecg: $('ecg'), art: $('art'), cap: $('cap') }, S.probes);
}

function start() {
  $('start').style.display = 'none';
  S.view.setSound(true);
  const cfg = roomConfig() || { url: `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/ws` };
  new Room(cfg, 'monitor', {
    onMessage: (m) => {
      if (m.kind === 'snapshot') {
        S.snap = m.snap; S.probes = m.probes ?? S.probes; S.patient = m.patient || S.patient; S.lastSnapAt = performance.now();
        values(); header();
      }
      if (m.kind === 'waves') S.view.feed(m);
      if (m.kind === 'probes') { S.probes = m.probes; values(); }
      if (m.kind === 'session') { S.view.reset(); S.patient = m.patient; header(); }
    },
    onStatus: (st) => { S.link = st; header(); },
  });
  setInterval(header, 1000);
  requestAnimationFrame(frame);
}

$('go').onclick = start;
if (new URLSearchParams(location.search).get('autostart') === '1') start();
