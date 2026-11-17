"""Fixture de l'ECG construit depuis la conduction (signals.make_ecg_conducted) : séquence de
conduction (rhythm.py, décalée de SIGNAL_WARM_S), paramètres, tirages du bruit enregistrés et
tracé produit. Le test Rust (tests/ecg_conducted.rs) reconstruit la même séquence, rejoue le
bruit et compare le tracé à 1e-9.
Sortie : crates/vetsim-core/tests/fixtures/python_reference_ecg_conducted.json
Usage : python tests/make_ecg_conducted_fixture.py   (depuis la racine du kit)"""
import json, sys
sys.path.insert(0, 'reference/python')
import numpy as np
import params, rhythm, signals

SCEN = [
    dict(id='sain', hr=88.0, over={}),
    dict(id='fa', hr=88.0, over=dict(af=1.0)),
    dict(id='bav3', hr=88.0, over=dict(hp_conduction=0.0)),
    dict(id='esv', hr=88.0, over=dict(vpc_rate=12.0)),
    dict(id='k6_7', hr=140.0, over=dict(k=6.7)),
    dict(id='k7_35', hr=163.0, over=dict(k=7.35)),
    dict(id='k9', hr=88.0, over=dict(k=9.0)),
    dict(id='hypok', hr=88.0, over=dict(k=2.5)),
]
STRIDE = 3


class Recorder:
    def __init__(self, seed):
        self.g = np.random.Generator(np.random.PCG64(seed)); self.log = []
    def standard_normal(self, size=None):
        x = self.g.standard_normal(size); self.log.extend(np.atleast_1d(x).tolist()); return x


def series(x):
    x = np.asarray(x, float)
    return dict(n=len(x), every=x[::STRIDE].tolist(), sum=float(x.sum()), sum_abs=float(np.abs(x).sum()))


def main():
    out = {'note': __doc__.splitlines()[0], 'stride': STRIDE, 'cases': []}
    real_rng = np.random.default_rng
    for i, sc in enumerate(SCEN):
        p = params.healthy_params(20.0); p.update(sc['over'])
        g = real_rng(2000 + i)
        cond = rhythm.simulate_conduction(p, 0.6, 60.0 / sc['hr'], 20.0, lambda: float(g.random()))
        cond = signals.shift_conduction(cond, signals.SIGNAL_WARM_S)
        rec = []
        np.random.default_rng = lambda seed=None: rec.append(Recorder(seed)) or rec[-1]
        try:
            t, v, f = signals.make_ecg_conducted(p, cond, sc['hr'])
        finally:
            np.random.default_rng = real_rng
        out['cases'].append(dict(
            id=sc['id'], hr=sc['hr'], over=sc['over'],
            atrial=[[a, o] for a, o in cond['atrial']],
            ventricular=[[b['t'], b['origin'], b['t_a'], b['pr'], b['wide']] for b in cond['ventricular']],
            noise=rec[0].log, features={k: float(x) for k, x in f.items()}, sig=series(v)))
        print(sc['id'], len(cond['atrial']), 'A,', len(cond['ventricular']), 'V')
    dst = 'crates/vetsim-core/tests/fixtures/python_reference_ecg_conducted.json'
    json.dump(out, open(dst, 'w'))
    print(dst)


main()
