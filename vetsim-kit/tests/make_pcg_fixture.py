"""Fixture de rejeu du module auscultation : signals_pcg_v5.make_pcg (implémentation secondaire,
inchangée) avec un mandataire qui ENREGISTRE chaque tirage de NumPy ; le Rust les rejoue.
Exécuter depuis la racine du kit :  python tests/make_pcg_fixture.py
Sortie : crates/vetsim-core/tests/fixtures/python_reference_pcg.json
Les longs vecteurs de tirages sont stockés en float64 little-endian encodés en base64."""
import base64, json, sys
sys.path.insert(0, 'reference/python')
import numpy as np
import params as PY, diseases as D, physio, autonomic

class Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

class Recorder:
    def __init__(self, seed):
        self.g = np.random.Generator(np.random.PCG64(seed)); self.log = []
    def _rec(self, x):
        self.log.extend(np.atleast_1d(x).astype(float).tolist()); return x
    def random(self, size=None): return self._rec(self.g.random(size))
    def standard_normal(self, size=None): return self._rec(self.g.standard_normal(size))
    def uniform(self, low=0.0, high=1.0, size=None): return self._rec(self.g.uniform(low, high, size))
    def lognormal(self, mean=0.0, sigma=1.0, size=None): return self._rec(self.g.lognormal(mean, sigma, size))

recorders = []
real_default_rng = np.random.default_rng
def fake_default_rng(seed=None):
    r = Recorder(seed); recorders.append(r); return r
import signals_pcg_v5 as S
S.np.random.default_rng = fake_default_rng   # même objet numpy : la référence n'est pas modifiée

D.KM['zsd'] = 0.0
STRIDE = 7

def b64(x):
    return base64.b64encode(np.asarray(x, '<f8').tobytes()).decode()

def series(x):
    x = np.asarray(x, float)
    return dict(n=len(x), every=x[::STRIDE].tolist(), sum=float(x.sum()), sum_abs=float(np.abs(x).sum()))

def case(label, bw, disease, dur, fs, seed, use_beats, resp=None):
    p = PY.healthy_params(bw)
    if disease:
        p = D.apply(p, D.REGISTRY['MMVD'].sample(Stub(), severity=disease))
    np.random.default_rng = real_default_rng
    r = physio.evaluate(p)
    au = autonomic.simulate_autonomic(r, dur=120.0, seed=seed)
    np.random.default_rng = fake_default_rng
    h = r['hemo']
    beats = au['beats'][au['beats'] < dur] if use_beats else None
    resp_hz = resp if resp is not None else r['art']['rr'] / 60.0
    recorders.clear()
    t, sig, feats = S.make_pcg(h, fs=fs, seed=seed, beat_times=beats, resp_hz=resp_hz, dur=dur)
    hemo = {k: np.asarray(h[k]).tolist() for k in ('t', 'Q_ao', 'Q_mi', 'P_lv', 'P_ao', 'P_pv')}
    hemo.update(T=float(h['T']), ts=float(h['ts']), la_mean=float(h['la_mean']))
    return dict(label=label, fs=fs, dur=dur, resp_hz=resp_hz, hemo=hemo,
                beats=None if beats is None else beats.tolist(),
                noise_b64=b64(recorders[0].log), n_noise=len(recorders[0].log),
                features={k: (v.item() if hasattr(v, 'item') else v) for k, v in feats.items()}, sig=series(sig))

out = dict(note=__doc__.splitlines()[0], stride=STRIDE, cases=[
    case('sain 20 kg', 20, None, 3.0, 4000, 1, True),
    case('MMVD 0,3', 20, 0.3, 3.0, 4000, 2, True),
    case('MMVD 0,85, battements réguliers', 20, 0.85, 3.0, 4000, 3, False, resp=0.35),
    case('sain 5 kg, 8000 Hz', 5, None, 2.0, 8000, 4, True),
])
from scipy.signal import butter, sosfilt
imp = np.zeros(800); imp[0] = 1.0
out['impulse'] = [dict(order=o, band=[lo, hi], fs=fs, h=sosfilt(butter(o, [lo, hi], btype='band', fs=fs, output='sos'), imp).tolist())
                  for o, lo, hi, fs in [(3, 60, 250, 4000), (2, 35, 220, 4000), (3, 60, 250, 8000), (2, 35, 220, 8000)]]
dst = 'crates/vetsim-core/tests/fixtures/python_reference_pcg.json'
with open(dst, 'w') as f:
    json.dump(out, f); f.write('\n')
import os
print(dst, f'{os.path.getsize(dst) / 1e6:.2f} Mo', [ (c['label'], c['n_noise'], c['features']['grade'], c['features']['s3']) for c in out['cases']])
