"""Compare `vetsim-cli params` (Rust) aux paramètres du Python de référence (jalon M1, bout en bout :
lecture des fichiers par le CLI, DSL, effets maladie sans bruit).
  python tests/check_params_cli.py --cmd "target/release/vetsim-cli"
Tolérance : rel 1e-9, abs 1e-12 (celle de check_modules.py). Code de sortie != 0 si un écart."""
import argparse, json, math, shlex, subprocess, sys
sys.path.insert(0, 'reference/python')
import params as PY, diseases as D

class Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

NAMES = {'mmvd': 'MMVD', 'ckd': 'CKD', 'addison': 'Addison', 'pneumonia': 'Pneumonie', 'atrial_fibrillation': 'FA', 'av_block': 'BAV', 'ventricular_premature': 'ESV', 'sick_sinus': 'SSS'}
ap = argparse.ArgumentParser(); ap.add_argument('--cmd', required=True); a = ap.parse_args()
D.KM['zsd'] = 0.0
combos = [[]] + [[(n, s)] for n in NAMES for s in (0.2, 0.5, 0.8, 0.95)] + [[('mmvd', 0.8), ('ckd', 0.6)], [('addison', 0.9), ('pneumonia', 0.4)]]
bad = total = 0
for bw in (5, 20, 40):
    for combo in combos:
        args = ['params', '--bw', str(bw)] + [x for n, s in combo for x in ('--disease', f'{n}:{s}')]
        got = json.loads(subprocess.check_output(shlex.split(a.cmd) + args, text=True))
        ref = PY.healthy_params(bw)
        for n, s in combo:
            ref = D.apply(ref, D.REGISTRY[NAMES[n]].sample(Stub(), severity=s))
        for k in sorted(set(ref) | set(got)):
            total += 1
            if k not in got or k not in ref or not math.isclose(got[k], ref[k], rel_tol=1e-9, abs_tol=1e-12):
                bad += 1; print('ECART', ' '.join(args), k, got.get(k), ref.get(k))
print(f'{total - bad}/{total} paramètres conformes'); sys.exit(1 if bad else 0)
