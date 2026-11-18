"""Fixture de la décompensation (couplages mécanistiques, termes toxiques, viabilité) : référence Python.
Scénarios choisis loin des seuils de classement. Sortie : crates/vetsim-core/tests/fixtures/python_reference_tox.json
Usage : python tests/make_tox_fixture.py   (depuis la racine du kit)"""
import json, sys, warnings
sys.path.insert(0, 'reference/python')
warnings.filterwarnings('ignore')
import diseases, physio, pkpd
from patient import Patient

class Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

SCENARIOS = [
    dict(id='sain', bw=20.0),
    dict(id='opioide_0.5', bw=20.0, conc={'opioide': 0.5}),
    dict(id='opioide_2', bw=20.0, conc={'opioide': 2.0}),
    dict(id='betabloquant_3', bw=20.0, conc={'betabloquant': 3.0}),
    dict(id='betabloquant_10', bw=20.0, conc={'betabloquant': 10.0}),
    dict(id='pimobendan_3', bw=20.0, conc={'pimobendan': 3.0}),
    dict(id='furosemide_U50', bw=20.0, U=50.0),
    dict(id='mmvd_0.9', bw=20.0, disease=('MMVD', 'mmvd', 0.9)),
    dict(id='addison_1.0', bw=20.0, disease=('Addison', 'addison', 1.0)),
    dict(id='mmvd_3kg_0.9', bw=3.0, disease=('MMVD', 'mmvd', 0.9)),
    dict(id='mmvd_5kg_0.7', bw=5.0, disease=('MMVD', 'mmvd', 0.7)),
]
KEYS = ['map', 'co', 'hr', 'ph', 'pco2', 'pao2', 'po2_v', 'lactate', 'k']

def main():
    diseases.KM['zsd'] = 0.0
    out = {'note': __doc__.splitlines()[0], 'cases': []}
    for sc in SCENARIOS:
        pt = Patient(sc['bw'], seed=0, variability=False); pt.rng = Stub()
        if 'disease' in sc:
            pt.add_disease(sc['disease'][0], severity=sc['disease'][2])
        q = pkpd.apply_drug_effects(pt.p, sc.get('conc', {}), 0.0, sc.get('U', 0.0))
        case = dict(id=sc['id'], bw=sc['bw'], conc=sc.get('conc', {}), U=sc.get('U', 0.0),
                    disease=[sc['disease'][1], sc['disease'][2]] if 'disease' in sc else None)
        try:
            r = physio.evaluate(q)
            v = dict(map=r['hemo']['map'], co=r['hemo']['co'], hr=r['p']['hr'], ph=r['art']['ph'], pco2=r['art']['pco2'],
                     pao2=r['art']['pao2'], po2_v=r['ven']['po2'], lactate=r['art']['lactate'], k=r['chem']['k'])
            case.update(state=r['outcome']['state'], mechanisms=[m['id'] for m in r['outcome']['mechanisms']],
                        f_contract=r['decomp']['f_contract'], f_rsys=r['decomp']['f_rsys'], f_vent=r['decomp']['f_vent'],
                        values={k: float(v[k]) for k in KEYS})
        except physio.NonViable as e:
            case.update(state='dead', cause=e.cause)
        out['cases'].append(case)
        print(case['id'], case['state'], case.get('mechanisms', case.get('cause')))
    dst = 'crates/vetsim-core/tests/fixtures/python_reference_tox.json'
    with open(dst, 'w') as f:
        json.dump(out, f, indent=1, ensure_ascii=False); f.write('\n')

if __name__ == '__main__':
    main()
