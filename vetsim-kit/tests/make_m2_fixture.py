"""Génère la fixture de référence du jalon M2 (gaz, acide-base, rein, lactate, en algébrique).
Exécuter depuis la racine du kit :  python tests/make_m2_fixture.py
Sortie : crates/vetsim-core/tests/fixtures/python_reference_m2.json

Méthode : physio.simulate_hemo est remplacé par un bouchon qui renvoie une hémodynamique
imposée (co, map, la_mean) ; physio._evaluate_once (une passe de la référence, sans les couplages de décompensation) calcule tout l'aval.
L'aval ne dépend que de ces trois grandeurs et des paramètres non touchés par le baroréflexe.
Ne modifie aucun module ni aucune valeur de référence."""
import json, os, sys
sys.path.insert(0, 'reference/python')
import params as PY, diseases as D, physio

class Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

NAMES = {'mmvd': 'MMVD', 'ckd': 'CKD', 'addison': 'Addison', 'pneumonia': 'Pneumonie'}
D.KM['zsd'] = 0.0
KEYS = ['ph', 'pco2', 'hco3', 'be', 'pao2', 'sao2', 'p50', 'rr', 'lactate', 'ph_v', 'pco2_v', 'po2_v', 'hco3_v',
        'na', 'k', 'cl', 'glucose', 'creat', 'urea', 'phos', 'alb', 'hct', 'gfr', 'anion_gap', 'do2_kg', 'er']

def flat(r):
    a, v, c = r['art'], r['ven'], r['chem']
    d = dict(ph=a['ph'], pco2=a['pco2'], hco3=a['hco3'], be=a['be'], pao2=a['pao2'], sao2=a['sao2'], p50=a['p50'], rr=a['rr'],
             lactate=a['lactate'], ph_v=v['ph'], pco2_v=v['pco2'], po2_v=v['po2'], hco3_v=v['hco3'],
             na=c['na'], k=c['k'], cl=c['cl'], glucose=c['glucose'], creat=c['creat'], urea=c['urea'], phos=c['phos'],
             alb=c['alb'], hct=c['hct'], gfr=c['gfr'], anion_gap=c['anion_gap'], do2_kg=r['do2_kg'], er=r['er'])
    return {k: float(d[k]) for k in KEYS}

def scale(bw, co20):          # débit à l'échelle allométrique du poids
    return co20 * (bw / 20.0) ** 0.75

# Profils hémodynamiques : normal, bas débit, congestion, choc, hyperdébit
HEMO = {'normal': (3.3, 103.0, 6.0), 'low_output': (1.2, 70.0, 9.0), 'congestion': (2.4, 92.0, 28.0),
        'shock': (0.35, 38.0, 4.0), 'high_output': (6.0, 120.0, 5.0)}
PATIENTS = [(bw, [], {}) for bw in (5, 20, 40)] + \
           [(20, [(n, s)], {}) for n in NAMES for s in (0.2, 0.5, 0.8, 0.95)] + \
           [(20, [('mmvd', 0.8), ('ckd', 0.6)], {}), (40, [('addison', 0.9), ('pneumonia', 0.4)], {}),
            (20, [], {'vent_drive': 0.4}), (20, [], {'fio2': 1.0}), (20, [], {'hb': 6.0}), (20, [('pneumonia', 1.0)], {'vent_drive': 0.45})]

out = {'note': __doc__.splitlines()[0], 'keys': KEYS, 'cases': []}
for bw, combo, over in PATIENTS:
    p0 = PY.healthy_params(bw)
    for n, s in combo:
        p0 = D.apply(p0, D.REGISTRY[NAMES[n]].sample(Stub(), severity=s))
    p0.update(over)
    for label, (co20, mp, la) in HEMO.items():
        hemo = dict(co=scale(bw, co20), map=mp, la_mean=la)
        physio.simulate_hemo = lambda q, n_beats=11, h=hemo: dict(h)
        entry = dict(bw=float(bw), diseases=[[n, s] for n, s in combo], overrides=over, hemo_profile=label, hemo=hemo)
        try:
            entry["expected"] = flat(physio._evaluate_once(dict(p0)))   # une passe, sans couplage de décompensation
        except Exception as exc:
            entry['error'] = f'{type(exc).__name__}: {exc}'
        out['cases'].append(entry)

dst = 'crates/vetsim-core/tests/fixtures/python_reference_m2.json'
with open(dst, 'w') as f:
    json.dump(out, f, indent=1, ensure_ascii=False); f.write('\n')
print(dst, len(out['cases']), 'cas,', sum('error' in c for c in out['cases']), 'erreurs Python')
