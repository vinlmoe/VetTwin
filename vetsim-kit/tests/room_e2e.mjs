// Essai de bout en bout de la salle de simulation dans Chromium (Playwright) :
// relais (salle/relay.py) + régie (index.html?room=1, onglet Anesthésie) + moniteur de salle
// (monitor.html) + lecteur d'auscultation (ausc.html), boîtiers simulés par HTTP POST.
// Prérequis : sh web/build.sh, Playwright installé (require('playwright')).
//   node tests/room_e2e.mjs [dossier_captures]
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import path from 'node:path';
import fs from 'node:fs';
import net from 'node:net';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(HERE, '..');
const SHOTS = process.argv[2] || null;
const fails = [];
const check = (c, what) => { console.log((c ? 'ok   ' : 'ÉCHEC ') + what); if (!c) fails.push(what); };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const port = await new Promise((res) => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => res(p)); }); });
const relay = spawn('python3', ['-I', path.join(ROOT, 'salle/relay.py'), '--web', path.join(ROOT, 'web'), '--port', String(port)], { stdio: ['ignore', 'pipe', 'pipe'] });
let relayLog = ''; relay.stdout.on('data', (d) => { relayLog += d; }); relay.stderr.on('data', (d) => { relayLog += d; });
const base = `http://127.0.0.1:${port}`;
for (let i = 0; i < 50; i++) { try { await fetch(base + '/api/status'); break; } catch (e) { await sleep(100); } }

const device = async (dev, input) => {
  const r = await fetch(base + '/api/device', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ device: dev, input }) });
  return { code: r.status, ...(await r.json()) };
};

const browser = await chromium.launch({ args: ['--autoplay-policy=no-user-gesture-required'] });
const errors = [];
const page = async (url, w = 1400, h = 900) => {
  const p = await browser.newPage({ viewport: { width: w, height: h } });
  p.on('pageerror', (e) => errors.push(`${url}: ${e.message}`));
  p.on('console', (m) => { if (m.type() === 'error') errors.push(`${url}: ${m.text()}`); });
  await p.goto(base + url);
  return p;
};
try {
  // régie
  const regie = await page('/index.html?room=1');
  await regie.waitForSelector('#api:not(:empty)', { timeout: 30000 });
  await regie.click('#tabs button[data-tab="anesthesia"]');
  await regie.waitForSelector('#an-start');
  const pre = await device('pupitre-1', { type: 'inject', agent: 'propofol', conc_mg_ml: 10, ml: 2 });
  check(pre.code === 200 && !pre.ok && /non démarrée/.test(pre.text), `entrée avant le démarrage refusée : ${pre.text}`);
  await regie.selectOption('#an-proc', 'ovariohysterectomy');
  await regie.click('#an-start');
  await regie.waitForSelector('#an-room .ok', { timeout: 10000 });
  check(true, 'régie reliée au relais');

  const mon = await page('/monitor.html?room=1&autostart=1', 1280, 720);
  const ausc = await page('/ausc.html?room=1&autostart=1', 700, 500);
  await mon.waitForFunction(() => document.getElementById('link').textContent === 'en ligne', null, { timeout: 15000 });
  check(true, 'moniteur de salle en ligne');

  // capteurs : SpO2 et capnographe non posés
  let r = await device('capteurs', { type: 'probes', ecg: true, spo2: false, nibp: true, art: false, capno: false, temp: true });
  check(r.ok, `capteurs : ${r.text}`);
  await sleep(1500);
  const vals = await mon.$$eval('#vals .mv', (els) => els.map((e) => ({ lab: e.querySelector('span').textContent, val: e.querySelector('b').textContent, off: e.classList.contains('off') })));
  const spo2 = vals.find((v) => v.lab === 'SpO₂'), fc = vals.find((v) => v.lab.startsWith('FC'));
  check(spo2 && spo2.off && spo2.val === '—', 'SpO₂ masquée sans oxymètre');
  check(fc && !fc.off && /^\d+$/.test(fc.val), `FC affichée avec l'ECG (${fc && fc.val})`);

  // prémédication, induction (pupitre RFID), machine, temps opératoire
  r = await device('pupitre-1', { type: 'inject', agent: 'dexmedetomidine', conc_mg_ml: 0.5, ml: 0.2, route: 'im' });
  check(r.ok && /dexmedetomidine/.test(r.text), `injection IM : ${r.text}`);
  r = await device('pupitre-1', { type: 'inject', agent: 'propofol', conc_mg_ml: 10, ml: 6, route: 'iv', duration_s: 40 });
  check(r.ok && /mg\/kg/.test(r.text), `injection IV : ${r.text}`);
  r = await device('pupitre-1', { type: 'inject', agent: 'propofol', conc_mg_ml: 10, ml: 1, route: 'im' });
  check(!r.ok, `voie non disponible refusée : ${r.text}`);
  r = await device('pupitre-1', { type: 'inject', agent: 'inconnu', conc_mg_ml: 1, ml: 1 });
  check(!r.ok, `agent inconnu refusé : ${r.text}`);
  // intubation trop précoce : le refus du moteur revient tel quel au boîtier
  r = await device('machine', { type: 'intubate' });
  check(!r.ok && /déglutition/.test(r.text), `intubation trop précoce refusée : ${r.text}`);
  for (let i = 0; i < 30 && !r.ok; i++) { await sleep(1000); r = await device('machine', { type: 'intubate' }); }
  check(r.ok, `intubation après induction : ${r.text}`);
  r = await device('machine', { type: 'vaporizer', agent: 'isoflurane', pct: 9 });
  check(r.ok && /butée/.test(r.text), `vaporisateur au-delà de la butée : ${r.text}`);
  r = await device('machine', { type: 'vaporizer', agent: 'isoflurane', pct: 2 });
  check(r.ok, `vaporisateur 2 % : ${r.text}`);
  r = await device('machine', { type: 'fresh_gas', l_min: 1.5 });
  check(r.ok, `gaz frais : ${r.text}`);
  r = await device('formateur', { type: 'procedure_next' });
  check(r.ok && /Incision/.test(r.text), `temps opératoire suivant : ${r.text}`);
  const st = await regie.$eval('#an-status .last', (e) => e.textContent);
  check(/\[formateur\]/.test(st), `régie : dernière action étiquetée par le boîtier (${st.trim()})`);

  // stéthoscope → lecteur d'auscultation
  r = await device('stethoscope', { type: 'stethoscope', site: 'apex' });
  check(r.ok, `stéthoscope : ${r.text}`);
  await ausc.waitForFunction(() => /^Apex · FC/.test(document.getElementById('state').textContent), null, { timeout: 20000 });
  check(true, `lecteur d'auscultation : ${await ausc.$eval('#state', (e) => e.textContent)}`);

  // pouls (boîtier de sortie)
  const pulse = (await (await fetch(base + '/api/pulse')).text()).trim().split(/\s+/);
  check(pulse.length === 6 && +pulse[0] > 20 && pulse[4] === '1', `/api/pulse : ${pulse.join(' ')}`);

  // la régie avance en ×10 : le moniteur suit
  await sleep(3000);
  const t1 = await mon.$eval('#clock', (e) => e.textContent);
  check(t1 !== '--:--:--' && t1 !== '00:00:00', `horloge du moniteur : ${t1}`);

  if (SHOTS) {
    fs.mkdirSync(SHOTS, { recursive: true });
    await mon.screenshot({ path: path.join(SHOTS, 'moniteur.png') });
    await regie.screenshot({ path: path.join(SHOTS, 'regie.png') });
    await ausc.screenshot({ path: path.join(SHOTS, 'auscultation.png') });
  }
  // tous les capteurs retirés puis remis
  r = await device('capteurs', { type: 'probes', spo2: true, capno: true });
  check(r.ok, 'capteurs remis');
} catch (e) {
  check(false, 'exception : ' + e.message);
} finally {
  check(errors.length === 0, `aucune erreur JavaScript${errors.length ? ' : ' + errors.join(' | ') : ''}`);
  await browser.close();
  relay.kill();
  if (/Traceback/.test(relayLog)) { check(false, 'relais sans exception'); console.log(relayLog); }
  console.log(fails.length ? `\n${fails.length} échec(s)` : '\nsalle : tous les contrôles passent');
  process.exit(fails.length ? 1 : 0);
}
