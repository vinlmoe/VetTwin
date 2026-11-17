"""Génère la fixture de référence du jalon M4 (PK/PD) depuis le Python normatif.
Exécuter depuis la racine du kit :  python tests/make_m4_fixture.py
Sortie : crates/vetsim-core/tests/fixtures/python_reference_m4.json
 - effects : pkpd.apply_drug_effects(p, conc, D, U) (algébrique) pour des concentrations autour de l'EC50,
   négatives, nulles, combinées, avec ou sans maladie ;
 - timecourses : pkpd.timecourse (LSODA rtol 1e-8) hors golden : doses répétées, associations, MRC, poids extrêmes,
   avec D et U (diurétique).
Ne modifie aucun module ni aucune valeur de référence."""
import json, sys
sys.path.insert(0, 'reference/python')
import numpy as np, params as PY, diseases as D, pkpd

class Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

NAMES = {'mmvd': 'MMVD', 'ckd': 'CKD', 'addison': 'Addison', 'pneumonia': 'Pneumonie'}
D.KM['zsd'] = 0.0

def patient(bw, combo):
    p = PY.healthy_params(bw)
    for n, s in combo:
        p = D.apply(p, D.REGISTRY[NAMES[n]].sample(Stub(), severity=s))
    return p

out = {'note': __doc__.splitlines()[0], 'effects': [], 'timecourses': []}
EC50 = {k: v['ec50'] for k, v in pkpd.DRUGS.items()}
for bw, combo in [(20, []), (20, [('mmvd', 0.8)]), (5, [('ckd', 0.6)])]:
    for drug, ec in EC50.items():
        for c in (0.0, -0.01, ec / 4, ec, 10 * ec, 1000 * ec):
            conc = {drug: c}
            out['effects'].append(dict(bw=bw, diseases=combo, conc=conc, D=0.0, U=0.0,
                                       params={k: float(v) for k, v in pkpd.apply_drug_effects(patient(bw, combo), conc, 0.0, 0.0).items()}))
    for conc, Dv, Uv in [({}, 150.0, 2.0), ({}, -30.0, 0.0), ({'furosemide': 3.0}, 80.0, 1.5),
                         ({k: 2 * v for k, v in EC50.items()}, 120.0, 3.0)]:
        out['effects'].append(dict(bw=bw, diseases=combo, conc=conc, D=Dv, U=Uv,
                                   params={k: float(v) for k, v in pkpd.apply_drug_effects(patient(bw, combo), conc, Dv, Uv).items()}))

T36 = [float(t) for t in np.arange(0, 36.5, 0.5)]
TC = [
    (20, [], [(0.0, 'furosemide', 2.0, 'po'), (12.0, 'furosemide', 2.0, 'po'), (24.0, 'furosemide', 2.0, 'po')], 36.0, T36),
    (20, [('mmvd', 0.8)], [(0.0, 'pimobendan', 0.25, 'po'), (0.0, 'furosemide', 2.0, 'iv'), (12.0, 'pimobendan', 0.25, 'po')], 24.0, T36[:49]),
    (20, [('ckd', 0.8)], [(0.0, 'atropine', 0.04, 'iv'), (1.5, 'atropine', 0.02, 'iv')], 8.0, [0, 0.1, 0.25, 0.5, 1, 1.5, 2, 3, 4, 6, 8]),
    (5, [], [(0.0, 'betabloquant', 1.0, 'po'), (8.0, 'betabloquant', 1.0, 'po')], 16.0, [float(t) for t in range(17)]),
    (40, [], [(0.0, 'opioide', 0.3, 'iv'), (0.0, 'furosemide', 4.0, 'iv')], 12.0, [0, 0.05, 0.25, 0.5, 1, 2, 4, 6, 8, 12]),
    (20, [('ckd', 0.6), ('mmvd', 0.5)], [(0.0, 'furosemide', 5.0, 'po'), (6.0, 'furosemide', 2.0, 'iv')], 24.0, [0, 1, 2, 4, 6, 6.5, 7, 8, 12, 18, 24]),
]
for bw, combo, doses, t_end, t_eval in TC:
    tc = pkpd.timecourse(patient(bw, combo), doses, t_end, t_eval)
    out['timecourses'].append(dict(bw=bw, diseases=combo, doses=[list(d) for d in doses], t_end=t_end, t_eval=t_eval,
                                   expected={k: [float(x) for x in v] for k, v in tc.items()}))

dst = 'crates/vetsim-core/tests/fixtures/python_reference_m4.json'
with open(dst, 'w') as f:
    json.dump(out, f, indent=1); f.write('\n')
print(dst, len(out['effects']), 'jeux d\'effets,', len(out['timecourses']), 'cinétiques')
