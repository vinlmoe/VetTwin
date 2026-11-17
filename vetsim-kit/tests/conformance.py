"""Compare un CLI (par défaut la référence Python) aux valeurs golden.
  python tests/conformance.py                                   # vérifie la référence elle-même
  python tests/conformance.py --cmd "./target/release/vetsim-cli"
Code de sortie != 0 si un écart dépasse la tolérance."""
import json, subprocess, sys, shlex, argparse, math
ap = argparse.ArgumentParser(); ap.add_argument('--cmd', default=f'{sys.executable} reference/python/cli.py'); a = ap.parse_args()
G = json.load(open('tests/golden/cases.json')); tol = G['tolerances']; bad = total = 0
def ok(key, got, exp):
    t = tol['per_key'].get(key, {}); rt = t.get('rtol', tol['default_rtol']); at = t.get('atol', tol['default_atol'])
    return math.isclose(got, exp, rel_tol=rt, abs_tol=at)
for c in G['cases']:
    out = json.loads(subprocess.check_output(shlex.split(a.cmd) + c['args'], text=True))
    for k, exp in c['expected'].items():
        vals = zip(out.get(k, [None] * len(exp)), exp) if isinstance(exp, list) else [(out.get(k), exp)]
        for got, e in vals:
            total += 1
            if got is None or not ok(k, got, e):
                bad += 1; print(f"ECART {c['id']} {k}: obtenu {got} attendu {e}")
print(f'{total - bad}/{total} valeurs conformes'); sys.exit(1 if bad else 0)
