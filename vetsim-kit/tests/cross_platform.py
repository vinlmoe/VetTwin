"""Vérifie que deux exécutables du CLI (plateformes différentes) produisent les MÊMES BITS.
  python tests/cross_platform.py --ref "target/release/vetsim-cli" \
      --cmd "wasmtime run --dir . target/wasm32-wasip1/release/vetsim-cli.wasm"
Compare la sortie JSON brute (chaîne identique, donc flottants identiques au bit près) sur tous les
cas du golden, plus des commandes stochastiques à graine fixe (signals, calibrate réduit)."""
import argparse, json, shlex, subprocess, sys
ap = argparse.ArgumentParser(); ap.add_argument('--ref', required=True); ap.add_argument('--cmd', required=True); a = ap.parse_args()
G = json.load(open('tests/golden/cases.json'))
cases = [c['args'] for c in G['cases']]
cases += [['signals', '--disease', 'mmvd:0.8', '--seed', '3'], ['signals', '--disease', 'addison:0.9', '--seed', '11', '--duration', '20'],
          ['calibrate', '--n-healthy', '3', '--n-disease', '2', '--n-hrv', '2'],
          ['eval', '--disease', 'mmvd:1.0'], ['eval', '--disease', 'addison:0.9'], ['eval', '--disease', 'addison:1.0']]
bad = 0
for args in cases:
    r = subprocess.run(shlex.split(a.ref) + args, capture_output=True, text=True)
    c = subprocess.run(shlex.split(a.cmd) + args, capture_output=True, text=True)
    if r.returncode or c.returncode or r.stdout != c.stdout:
        bad += 1
        print('ÉCART', ' '.join(args), r.returncode, c.returncode, c.stderr[:200])
        if not r.returncode and not c.returncode:
            jr, jc = json.loads(r.stdout), json.loads(c.stdout)
            diff = [k for k in jr if jr[k] != jc.get(k)][:5] if isinstance(jr, dict) else '?'
            print('   clés différentes :', diff)
print(f'{len(cases) - bad}/{len(cases)} sorties identiques au bit près'); sys.exit(1 if bad else 0)
