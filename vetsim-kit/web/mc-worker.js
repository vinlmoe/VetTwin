// Monte-Carlo hors du fil de l'interface : trajectoires d'un même animal, par paquets.
// Message reçu : {bw, seed, individual, disease, stage, doses, times, n, keys, scale,
// progression, chunk}. Messages émis : {progress: k, n}, puis {done: true, trajectories}.
// La trajectoire r utilise la graine seed + r (identique quel que soit le découpage en paquets).
import init, { Engine } from './pkg/vetsim_wasm.js';

let ready = null;
async function engine() {
  if (!ready) {
    ready = (async () => {
      await init();
      const e = new Engine();
      const bundle = await (await fetch('./modules.json')).text();
      check(e.loadModules(bundle));
      return e;
    })();
  }
  return ready;
}

function check(text) {
  const r = JSON.parse(text);
  if (!r.ok) throw new Error((r.errors || []).map((x) => `${x.path} ${x.message}`.trim()).join(' ; '));
  return r;
}

self.onmessage = async (ev) => {
  const q = ev.data;
  try {
    const e = await engine();
    // L'animal : tiré une fois (variabilité interindividuelle, graine q.seed) ou chien moyen.
    const id = check(e.newPatient(JSON.stringify({ bw: q.bw, seed: q.seed, variability: q.individual }))).patient_id;
    if (q.disease) {
      const o = { id: q.disease };
      if (q.stage) o.stage = q.stage;
      check(e.addDisease(id, JSON.stringify(o)));
    }
    const out = [];
    const chunk = q.chunk || 2;
    for (let k = 0; k < q.n; k += chunk) {
      const m = Math.min(chunk, q.n - k);
      const r = check(e.montecarlo(id, JSON.stringify({
        state_times_h: q.times, doses: q.doses, n: m, seed: q.seed * 1000 + k, keys: q.keys,
        scale: q.scale, progression_per_day: q.progression, trajectories: m,
      })));
      out.push(...r.trajectories);
      self.postMessage({ progress: k + m, n: q.n });
    }
    self.postMessage({ done: true, trajectories: out, t: q.times });
  } catch (err) {
    self.postMessage({ error: String(err && err.message ? err.message : err) });
  }
};
