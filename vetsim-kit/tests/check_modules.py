"""Vérifie que les modules JSON reproduisent le code Python de référence (zéro bruit) :
 - patient sain à 5, 20, 40 kg
 - modificateurs maladie à 3 sévérités
Usage : python tests/check_modules.py   (depuis la racine du kit)"""
import json, sys, os, math
sys.path.insert(0, 'tests'); sys.path.insert(0, 'reference/python')
from dsl_ref import ev, disease_mods
import params as PY, diseases as D

class Stub:                       # rng sans bruit : lognormal -> 1, random -> 0.99 (Bernoulli faux)
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

def main():
    H = json.load(open('modules/healthy_dog.json')); bad = 0
    for bw in (5, 20, 40):
        env = {'s': bw / 20.0, 'bw': float(bw)}; ref = PY.healthy_params(bw)
        for k, ex in H['params'].items():
            v = ev(ex, env)
            if not math.isclose(v, ref[k], rel_tol=1e-9, abs_tol=1e-12):
                print('ECART sain', bw, k, v, ref[k]); bad += 1
    for name, cls in [('mmvd', 'MMVD'), ('ckd', 'CKD'), ('addison', 'Addison'), ('pneumonia', 'Pneumonie'),
                      ('atrial_fibrillation', 'FA'), ('av_block', 'BAV'), ('ventricular_premature', 'ESV'), ('sick_sinus', 'SSS')]:
        mod = json.load(open(f'modules/diseases/{name}.json')); D.KM['zsd'] = 0.0
        for s in (0.2, 0.5, 0.8, 0.95):
            case = D.REGISTRY[cls].sample(Stub(), severity=s)
            ref = dict((k, (op, v)) for k, op, v in case.mods)
            got = dict((k, (op, v)) for k, op, v in disease_mods(mod, s))
            for k in ref:
                if k not in got or got[k][0] != ref[k][0] or not math.isclose(got[k][1], ref[k][1], rel_tol=1e-9, abs_tol=1e-12):
                    print('ECART', name, s, k, got.get(k), ref[k]); bad += 1
    print('OK' if not bad else f'{bad} écarts'); return bad
if __name__ == '__main__':
    sys.exit(main())
