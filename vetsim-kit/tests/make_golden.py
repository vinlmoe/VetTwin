"""Génère tests/golden/cases.json depuis l'implémentation Python de référence (exécuter depuis la racine du kit)."""
import json, subprocess, sys
CLI = [sys.executable, 'reference/python/cli.py']
cases = []
def run(args):
    return json.loads(subprocess.check_output(CLI + args, text=True))
def add(cid, args):
    cases.append(dict(id=cid, args=args, expected=run(args)))
for bw in (5, 20, 40): add(f'healthy_{bw}kg', ['eval', '--bw', str(bw)])
for d in ('mmvd', 'ckd', 'addison', 'pneumonia'):
    for s in (0.2, 0.5, 0.8): add(f'{d}_s{s}', ['eval', '--bw', '20', '--disease', f'{d}:{s}'])
add('mmvd_s0.8_pimobendan', ['eval', '--disease', 'mmvd:0.8', '--conc', 'pimobendan=0.03'])
for dr, c in [('pimobendan', 0.03), ('atropine', 0.004), ('betabloquant', 0.15), ('opioide', 0.015)]:
    add(f'healthy_{dr}', ['eval', '--conc', f'{dr}={c}'])
add('healthy_furosemide_volume', ['eval', '--D', '150', '--U', '2.0'])
T = '0,0.25,0.5,1,2,4,8,12'
for dr, dose, route in [('pimobendan', 0.5, 'po'), ('furosemide', 2.0, 'iv'), ('furosemide', 5.0, 'po'), ('atropine', 0.04, 'iv')]:
    add(f'pk_{dr}_{route}_{dose}', ['pk', '--drug', dr, '--dose', str(dose), '--route', route, '--times', T])
add('pk_furosemide_iv_ckd', ['pk', '--drug', 'furosemide', '--dose', '2.0', '--route', 'iv', '--times', T, '--disease', 'ckd:0.6'])
tol = dict(default_rtol=0.03, default_atol=0.02,
           per_key={'ph': dict(atol=0.005), 'ph_v': dict(atol=0.005), 'reg_fraction': dict(atol=0.02), 'ef': dict(atol=0.02),
                    's_tone': dict(atol=0.03), 'v_tone': dict(atol=0.03), 'be': dict(atol=0.6), 'lactate': dict(atol=0.1),
                    'k': dict(rtol=0.001, atol=0.001), 'na': dict(rtol=0.001, atol=0.01), 'cl': dict(rtol=0.001, atol=0.01),
                    'conc_mg_L': dict(rtol=0.02, atol=1e-5)},
           note="Hémodynamique issue d'un solveur ODE : tolérances de 3 %. Les valeurs purement algébriques (Na, K, Cl, glucose…) doivent être quasi exactes.")
json.dump(dict(tolerances=tol, cases=cases), open('tests/golden/cases.json', 'w'), indent=1)
print(len(cases), 'cas')
