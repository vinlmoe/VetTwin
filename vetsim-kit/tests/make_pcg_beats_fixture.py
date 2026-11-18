"""Fixture de l'auscultation battement par battement : reference/python/pcg_beats.make_pcg_beats
sur l'hémodynamique de chaque battement (enregistrée) et les tirages NumPy (enregistrés) ; le Rust
rejoue les mêmes tirages sur les mêmes battements et doit produire le même signal.
Exécuter depuis la racine du kit : python tests/make_pcg_beats_fixture.py
Sortie : crates/vetsim-core/tests/fixtures/python_reference_pcg_beats.json"""
import base64, json, sys, warnings
sys.path.insert(0, 'reference/python')
warnings.filterwarnings('ignore')
import numpy as np
import params as PY, diseases as D, physio, rhythm, pcg_beats

class Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

class Recorder:
    def __init__(self, seed):
        self.g = np.random.Generator(np.random.PCG64(seed)); self.log = []
    def _rec(self, x):
        self.log.extend(np.atleast_1d(x).astype(float).tolist()); return x
    def standard_normal(self, size=None): return self._rec(self.g.standard_normal(size))
    def uniform(self, low=0.0, high=1.0, size=None): return self._rec(self.g.uniform(low, high, size))
    def lognormal(self, mean=0.0, sigma=1.0, size=None): return self._rec(self.g.lognormal(mean, sigma, size))

def b64(x):
    return base64.b64encode(np.asarray(x, '<f8').tobytes()).decode()

D.KM['zsd'] = 0.0
DUR, FS = 4.0, 4000

def case(label, disease, sev, seed):
    p = PY.healthy_params(20.0)
    if disease:
        p = D.apply(p, D.REGISTRY[disease].sample(Stub(), severity=sev))
    r = physio.evaluate(p)
    q = r['p']
    cond = rhythm.simulate_conduction(q, r['autonomic']['v_eff'], 60.0 / q['hr_sinus'], pcg_beats.SIGNAL_WARM_S + DUR + 2.0,
                                      rhythm.SplitMix64(seed).random)
    beats, la = pcg_beats.signal_beats(q, cond, 60.0 / q['hr'], pcg_beats.SIGNAL_WARM_S, DUR, pcg_beats.CFG['defaults']['end_margin_s'])
    rec = Recorder(seed)
    t, sig, feats = pcg_beats.make_pcg_beats(beats, la, rec, fs=FS, dur=DUR, resp_hz=0.35)
    keys = ('t', 'Q_ao', 'Q_mi', 'P_lv', 'P_ao', 'P_pv')
    return dict(label=label, la_mean=float(la), resp_hz=0.35, fs=FS, dur=DUR,
                beats=[dict(start=float(b['t']), T=float(b['h']['T']), ts=float(b['h']['ts']),
                            **{k: np.asarray(b['h'][k]).tolist() for k in keys}) for b in beats],
                noise_b64=b64(rec.log), n_noise=len(rec.log),
                features={k: (v.item() if hasattr(v, 'item') else v) for k, v in feats.items()},
                sig_b64=b64(sig))

out = dict(note=__doc__.splitlines()[0], cases=[
    case('sain', None, None, 1),
    case('FA réponse modérée', 'FA', 0.5, 2),
    case('ESV très fréquentes', 'ESV', 0.9, 3),
    case('MMVD 0,6', 'MMVD', 0.6, 4),
])
with open('crates/vetsim-core/tests/fixtures/python_reference_pcg_beats.json', 'w') as f:
    json.dump(out, f); f.write('\n')
for c in out['cases']:
    s1 = [round(x, 3) for x in [c['features']['s1']]]
    print(c['label'], len(c['beats']), 'battements', c['n_noise'], 'tirages', c['features'])
