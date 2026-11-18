"""Génère la fixture de référence du jalon M6 (autonome, VFC, ECG, PCG) depuis le Python normatif.
Exécuter depuis la racine du kit :  python tests/make_m6_fixture.py
Sortie : crates/vetsim-core/tests/fixtures/python_reference_m6.json

Méthode : np.random.default_rng est remplacé par un mandataire qui ENREGISTRE chaque nombre tiré
par le code de référence (inchangé). Le Rust rejoue ces nombres (source « Replay ») : la comparaison
est alors déterministe, valeur par valeur. Les entrées scalaires de l'autonome (FC, tonus, PAM, FR, VA)
et les séries hémodynamiques du PCG sont prises dans la référence pour isoler le module testé.
Les séries longues sont sous-échantillonnées (1 valeur sur STRIDE) avec somme et somme des |x|.
Ne modifie aucun module ni aucune valeur de référence."""
import json, sys
sys.path.insert(0, 'reference/python')
import numpy as np
import params as PY, diseases as D, physio, autonomic, signals

class Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

class Recorder:
    """Mandataire de numpy.random.Generator qui enregistre les tirages, dans l'ordre."""
    def __init__(self, seed):
        self.g = np.random.Generator(np.random.PCG64(seed)); self.log = []
    def random(self, size=None):
        x = self.g.random(size); self.log.extend(np.atleast_1d(x).tolist()); return x
    def standard_normal(self, size=None):
        x = self.g.standard_normal(size); self.log.extend(np.atleast_1d(x).tolist()); return x

recorders = []
def fake_default_rng(seed=None):
    r = Recorder(seed); recorders.append(r); return r
np.random.default_rng = fake_default_rng

NAMES = {'mmvd': 'MMVD', 'ckd': 'CKD', 'addison': 'Addison', 'pneumonia': 'Pneumonie'}
D.KM['zsd'] = 0.0
STRIDE = 7

def patient(bw, combo):
    p = PY.healthy_params(bw)
    for n, s in combo:
        p = D.apply(p, D.REGISTRY[NAMES[n]].sample(Stub(), severity=s))
    return p

def series(x):
    x = np.asarray(x, float)
    return dict(n=len(x), every=x[::STRIDE].tolist(), sum=float(x.sum()), sum_abs=float(np.abs(x).sum()))

PNAMES = ['bw', 'va0', 'map_set', 'baro_sens', 'rsa_depth', 'vag_noise', 'sym_noise', 'a_s', 'a_v', 'k', 'pr', 'qrs', 'qt',
          'p_amp', 't_amp', 'r_amp']
out = {'note': __doc__.splitlines()[0], 'stride': STRIDE, 'autonomic': [], 'ecg': [], 'pcg': [], 'butter_impulse': None}

for label, bw, combo, dur, seed in [('sain 20 kg', 20, [], 120.0, 3), ('MMVD 0.8', 20, [('mmvd', 0.8)], 60.0, 5),
                                    ('Addison 0.8', 20, [('addison', 0.8)], 60.0, 7), ('sain 5 kg', 5, [], 60.0, 11)]:
    r = physio.evaluate(patient(bw, combo))
    recorders.clear()
    au = autonomic.simulate_autonomic(r, dur=dur, seed=seed)
    inputs = dict(hr=r['p']['hr'], s_eff=r['autonomic']['s_eff'], v_eff=r['autonomic']['v_eff'], map=r['hemo']['map'],
                  rr=r['art']['rr'], va=r['art']['va'], params={k: float(r['p'][k]) for k in PNAMES})
    out['autonomic'].append(dict(label=label, dur=dur, inputs=inputs, noise=recorders[0].log, fr=au['fr'],
                                 beats=au['beats'].tolist(), hr=series(au['hr']), s=series(au['s']), v=series(au['v']),
                                 hrv={k: float(v) for k, v in au['hrv'].items()}))

    # ECG avec les instants R de cette simulation
    recorders.clear()
    rt = au['beats'][au['beats'] < 8.0]
    t, sig, f = signals.make_ecg(r['p'], r['p']['hr'], r['art']['rr'], r_times=rt)
    out['ecg'].append(dict(label=label, hr=r['p']['hr'], rr=r['art']['rr'], params={k: float(r['p'][k]) for k in PNAMES},
                           r_times=rt.tolist(), noise=recorders[0].log, features={k: float(v) for k, v in f.items()}, sig=series(sig)))

    # PCG à partir de l'hémodynamique du dernier battement de la référence
    recorders.clear()
    h = physio.simulate_hemo(r['p'])
    t2, pcg, feats = signals.make_pcg(h)
    hemo = {k: np.asarray(h[k]).tolist() for k in ('t', 'Q_ao', 'Q_mi', 'P_lv', 'P_ao', 'P_pv')}
    hemo.update(T=float(h['T']), la_mean=float(h['la_mean']))
    out['pcg'].append(dict(label=label, hemo=hemo, noise=recorders[0].log, features={k: (v.item() if hasattr(v, 'item') else v) for k, v in feats.items()}, sig=series(pcg)))

# ECG pour une gamme de kaliémies, sans instants R imposés (repli sinusoïdal)
for k in (2.6, 4.1, 5.8, 6.6, 7.2, 8.3):
    p = PY.healthy_params(20); p['k'] = k
    recorders.clear()
    t, sig, f = signals.make_ecg(p, 90.0, 22.0)
    out['ecg'].append(dict(label=f'K {k}', hr=90.0, rr=22.0, params={kk: float(p[kk]) for kk in PNAMES}, r_times=None,
                           noise=recorders[0].log, features={kk: float(v) for kk, v in f.items()}, sig=series(sig)))

from scipy.signal import sosfilt, butter
imp = np.zeros(400); imp[0] = 1.0
out['butter_impulse'] = sosfilt(butter(3, [90, 450], btype='band', fs=4000, output='sos'), imp).tolist()

dst = 'crates/vetsim-core/tests/fixtures/python_reference_m6.json'
with open(dst, 'w') as fh:
    json.dump(out, fh); fh.write('\n')
import os
print(dst, f'{os.path.getsize(dst) / 1e6:.1f} Mo', len(out['autonomic']), 'autonome,', len(out['ecg']), 'ECG,', len(out['pcg']), 'PCG')
