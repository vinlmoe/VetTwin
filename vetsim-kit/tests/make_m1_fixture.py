"""Génère la fixture de référence du jalon M1 à partir du Python normatif.
Exécuter depuis la racine du kit :  python tests/make_m1_fixture.py
Sortie : crates/vetsim-core/tests/fixtures/python_reference.json (consommée par les tests Rust).

Contenu
 - healthy : params.healthy_params(bw) pour 5, 20, 40 kg (et 2.5, 63 kg hors golden) ;
 - disease_mods : modificateurs de diseases.REGISTRY[...].sample(Stub, severity=s), sans bruit ;
 - applied : diseases.apply(healthy_params(bw), case) pour des combinaisons maladie(s) x poids ;
 - dsl : expressions évaluées par tests/dsl_ref.py (valeur, ou type d'exception levée).
Ne modifie aucun module ni aucune valeur de référence."""
import json, math, os, sys
sys.path.insert(0, 'tests'); sys.path.insert(0, 'reference/python')
from dsl_ref import ev
import params as PY, diseases as D

class Stub:                       # identique à tests/check_modules.py et reference/python/cli.py
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

NAMES = {'mmvd': 'MMVD', 'ckd': 'CKD', 'addison': 'Addison', 'pneumonia': 'Pneumonie'}
D.KM['zsd'] = 0.0

def case(name, s):
    return D.REGISTRY[NAMES[name]].sample(Stub(), severity=s)

out = {'note': __doc__.splitlines()[0], 'healthy': {}, 'disease_mods': [], 'applied': [], 'dsl': []}
for bw in (2.5, 5, 20, 40, 63):
    out['healthy'][repr(float(bw))] = {k: float(v) for k, v in PY.healthy_params(bw).items()}

for name in NAMES:
    for s in (0.0, 0.05, 0.2, 0.5, 0.6, 0.8, 0.85, 0.95, 1.0):
        c = case(name, s)
        out['disease_mods'].append(dict(disease=name, severity=s, stage=c.meta.get('stage'),
                                         flags={k: v for k, v in c.meta.items() if k != 'stage'},
                                         mods=[[k, op, float(v)] for k, op, v in c.mods]))

combos = [[(n, s)] for n in NAMES for s in (0.2, 0.5, 0.8)] + [
    [('mmvd', 0.8), ('ckd', 0.6)], [('ckd', 0.6), ('addison', 0.5)], [('addison', 0.9), ('pneumonia', 0.4), ('mmvd', 0.3)]]
for bw in (5, 20, 40):
    for combo in combos:
        p = PY.healthy_params(bw)
        for n, s in combo:
            p = D.apply(p, case(n, s))
        out['applied'].append(dict(bw=float(bw), diseases=[[n, s] for n, s in combo], params={k: float(v) for k, v in p.items()}))

ENV = {'s': 0.37, 'bw': 12.5, 'z': 0.0, 'one': 1.0, 'neg': -2.0}
EXPRS = [
    2.4, True, False, 's', {'add': [1, 's', 'bw']}, {'add': ['s']}, {'mul': [87.0, {'pow': ['s', -0.25]}]},
    {'sub': ['bw', 's']}, {'div': [1.7572, {'pow': ['s', 0.75]}]}, {'pow': ['neg', 2]}, {'pow': ['neg', 3]},
    {'exp': {'mul': [-2.4, 's']}}, {'exp': [1.5]}, {'log': 'bw'}, {'neg': 's'}, {'abs': 'neg'}, {'abs': ['neg']},
    {'min': [3, 's', 2]}, {'max': [0, {'div': [{'sub': ['s', 0.8]}, 0.2]}]}, {'clip': ['bw', 0, 10]}, {'clip': ['neg', 0, 10]},
    {'clip': ['s', 0, 1]}, {'if': [{'gt': ['s', 0.3]}, 'bw', 's']}, {'if': ['z', {'div': [1, 'z']}, 7]},
    {'lt': ['s', 0.37]}, {'le': ['s', 0.37]}, {'gt': ['bw', 12]}, {'ge': ['neg', -1]}, {'not': 'z'}, {'not': [{'lt': [1, 2]}]},
    {'and': [{'lt': [0, 's']}, {'lt': ['s', 1]}]}, {'and': ['z', {'div': [1, 'z']}]}, {'or': ['one', {'log': 'z'}]}, {'or': ['z', False]},
    {'mul': [{'gt': ['s', 0]}, 5]}, {'add': [{'lt': [1, 2]}, {'lt': [1, 2]}]},
    {'mul': [6.07, {'pow': ['s', 2.617]}, 'one']}, {'max': [0.05, {'exp': {'mul': [-2.4, 0.95, 1.0]}}]},
    {'div': [1, 'z']}, {'log': 'z'}, {'log': 'neg'}, {'pow': ['z', -1]}, 'unknown_var', {'add': [1, 'unknown_var']},
]
for e in EXPRS:
    try:
        v = ev(e, ENV)
        out['dsl'].append(dict(expr=e, value=v))
    except Exception as exc:          # ZeroDivisionError, ValueError (log), KeyError (variable)
        out['dsl'].append(dict(expr=e, error=type(exc).__name__))
out['dsl_env'] = ENV

dst = 'crates/vetsim-core/tests/fixtures/python_reference.json'
os.makedirs(os.path.dirname(dst), exist_ok=True)
with open(dst, 'w') as f:
    json.dump(out, f, indent=1, ensure_ascii=False); f.write('\n')
print(dst, len(out['healthy']), 'poids,', len(out['disease_mods']), 'cas maladie,', len(out['applied']), 'combinaisons,', len(out['dsl']), 'expressions')
