"""Compare le banc de calibration Rust (`vetsim-cli calibrate`) au tableau de référence
calibration/calibration_run_python.json (jalon M5).
  python tests/compare_calibration.py --cmd "target/release/vetsim-cli" [--n-healthy 200] [--n-disease 96]

Les générateurs aléatoires diffèrent (NumPy PCG64 contre ChaCha8) : la comparaison des
cohortes est statistique. Une moyenne est conforme si
  écart relatif < 5 %   OU   |écart| < 3 erreurs-types combinées (√(ET_py²/n_py + ET_rs²/n_rs)).
La seconde condition traite les grandeurs proches de zéro (excès de base, fraction de régurgitation
sans MMVD), pour lesquelles un écart relatif n'a pas de sens.
PK (déterministe, sans variabilité) : écart relatif < 1e-4 et cibles identiques.
Lignes VFC (jalon M6) : 20 chiens côté Python, --n-hrv côté Rust (défaut 100), même critère.
Code de sortie != 0 si une ligne n'est pas conforme."""
import argparse, json, math, shlex, subprocess, sys

ap = argparse.ArgumentParser()
ap.add_argument('--cmd', required=True)
ap.add_argument('--n-healthy', type=int, default=200)
ap.add_argument('--n-disease', type=int, default=96)
ap.add_argument('--n-hrv', type=int, default=100)
a = ap.parse_args()
PY = json.load(open('calibration/calibration_run_python.json'))
RS = json.loads(subprocess.check_output(shlex.split(a.cmd) + ['calibrate', '--n-healthy', str(a.n_healthy),
                                                               '--n-disease', str(a.n_disease), '--n-hrv', str(a.n_hrv)], text=True))
N_PY_H, N_PY_D, N_PY_HRV = 50, 24, 20
NAMES = {'MMVD': 'mmvd', 'CKD': 'ckd', 'Addison': 'addison'}
HRV = {'RR moyen', 'SDNN', 'RMSSD', 'LF/HF'}
# Écart connu (décision de l'utilisateur du 17/11/2026) : systole calée sur B1→B2 du beagle
# (ts_coef 0,325) → PAM saine plus haute → LF/HF sous la cible. Même règle que signal_properties.rs.
KNOWN = {'LF/HF'}
bad, rels, lines = [], [], []

def verdict(label, m_py, sd_py, n_py, m_rs, sd_rs, n_rs):
    d = m_rs - m_py
    rel = abs(d) / abs(m_py) if m_py != 0 else (0.0 if d == 0 else math.inf)
    se = math.sqrt(sd_py ** 2 / n_py + sd_rs ** 2 / n_rs)
    z = abs(d) / se if se > 0 else (0.0 if d == 0 else math.inf)
    ok = rel < 0.05 or z < 3
    if math.isfinite(rel):
        rels.append(rel)
    lines.append(f"{'OK ' if ok else 'ÉCART'} {label:42s} Python {m_py:10.4f}  Rust {m_rs:10.4f}  écart {100 * rel:6.2f} %  z {z:5.2f}")
    if not ok:
        bad.append(label)

rs_h = {r['var']: r for r in RS['healthy']}
for r in PY['healthy']:
    q = rs_h[r['var']]
    assert q['target'] == r['target'] and q['tol'] == r['tol'], f"cible différente pour {r['var']}"
    n_py = N_PY_HRV if r['var'] in HRV else N_PY_H
    verdict(f"sain {r['var']}", r['model_mean'], r['model_sd'], n_py, q['model_mean'], q['model_sd'], q['n'])
    if r['var'] in KNOWN and not q['ok']:
        # écart connu déclaré (docs/NOUVELLE_REFERENCE.md) : cible inchangée, borne ±2 ET
        lines.append(f"      ÉCART CONNU {r['var']} : Rust {q['model_mean']:.4f} (cible {r['target']} ± {r['tol']})")
        if abs(q['model_mean'] - r['target']) > 2 * r['tol']:
            bad.append(f"{r['var']} au-delà de ±2 ET de la cible publiée (écart connu)")
    elif r['var'] in HRV and not q['ok']:
        bad.append(f"{r['var']} hors de ±1 ET de la cible publiée")
    elif r['var'] in KNOWN:
        lines.append(f"      {r['var']} redevenu conforme côté Rust : retirer l'écart connu")
    if q['ok'] != r['ok']:
        lines.append(f"      verdict de calage différent pour {r['var']} : Python {r['ok']}, Rust {q['ok']}")

for r, q in zip(PY['pk'], RS['pk']):
    assert (r['module'], r['var'], r['target']) == (q['module'], q['var'], q['target']), (r, q)
    rel = abs(q['model_mean'] - r['model_mean']) / abs(r['model_mean'])
    ok = rel < 1e-4
    rels.append(rel)
    lines.append(f"{'OK ' if ok else 'ÉCART'} {r['module'] + ' ' + r['var']:42s} Python {r['model_mean']:10.4f}  Rust {q['model_mean']:10.4f}  écart {rel:.1e}")
    if not ok:
        bad.append(r['var'])

def stats(xs):
    m = sum(xs) / len(xs)
    return m, math.sqrt(sum((x - m) ** 2 for x in xs) / len(xs))

# décès mécanistiques (exclus des moyennes ; générateurs différents : information, pas critère)
def _norm(k):
    d, st = k.split('|')
    return f"{NAMES.get(d, d)}|{st}"
_pyd = {_norm(k): v for k, v in PY.get('deaths', {}).items()}
_rsd = {_norm(k): v for k, v in RS.get('deaths', {}).items()}
for key in sorted(set(_pyd) | set(_rsd)):
    print(f"décès {key} : Python {len(_pyd.get(key, []))}/{N_PY_D} {_pyd.get(key, [])} ; Rust {len(_rsd.get(key, []))}/{a.n_disease} {_rsd.get(key, [])}")
for key, rows in PY['disease'].items():
    d, st = key.split('|')
    rrows = RS['disease'][f'{NAMES[d]}|{st}']
    for k in rows[0]:
        mp, sp = stats([x[k] for x in rows]); mr, sr = stats([x[k] for x in rrows])
        verdict(f'{d} {st} {k}', mp, sp, len(rows), mr, sr, len(rrows))

print('\n'.join(lines))
print(f"{len(lines) - len(bad) - sum(l.startswith('      ') for l in lines)} lignes conformes, {len(bad)} écarts ; "
      f"écart relatif moyen {100 * sum(rels) / len(rels):.2f} % (médian {100 * sorted(rels)[len(rels) // 2]:.2f} %, max {100 * max(rels):.2f} %)")
sys.exit(1 if bad else 0)
