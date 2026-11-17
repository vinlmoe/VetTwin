"""Critères d'acceptation du module auscultation (PROMPT_AUSCULTATION.md), mesurés par
reference/python/pcg_metrics.py sur des fichiers WAV 16 bits.

  python tests/pcg_acceptance.py --python                       # référence Python (signals_pcg_v5.py)
  python tests/pcg_acceptance.py --cmd target/release/vetsim-cli # moteur Rust (commande « pcg »)
  python tests/pcg_acceptance.py --cmd ... --report docs/PCG_CONFORMITE.md

Cas : chien sain 20 kg à FC 72 et 90 (paramètre « hr » imposé avant l'évaluation : réglage
d'usage, pas la valeur calibrée de 87/min), 20 graines ; MMVD 0,3 (léger) et 0,5 (modéré).
Chaque critère est jugé sur la médiane des 20 graines ; diastole = RR médian − B1→B2 médian.
Code de sortie != 0 si un critère du chien sain n'est pas atteint."""
import argparse, base64, io, json, os, shlex, subprocess, sys, tempfile
sys.path.insert(0, 'reference/python')
import numpy as np
from scipy.io import wavfile
import pcg_metrics

ap = argparse.ArgumentParser()
g = ap.add_mutually_exclusive_group(required=True)
g.add_argument('--python', action='store_true'); g.add_argument('--cmd')
ap.add_argument('--seeds', type=int, default=20); ap.add_argument('--report'); ap.add_argument('--json')
a = ap.parse_args()

def python_wav(hr, disease, seed, path):
    import params as PY, diseases as D, physio, autonomic, signals_pcg_v5 as S
    class Stub:
        def lognormal(self, *x): return 1.0
        def random(self): return 0.99
    D.KM['zsd'] = 0.0
    p = PY.healthy_params(20.0)
    if hr: p['hr'] = float(hr)
    if disease:
        n, s = disease
        p = D.apply(p, D.REGISTRY[{'mmvd': 'MMVD'}[n]].sample(Stub(), severity=s))
    r = physio.evaluate(p)
    au = autonomic.simulate_autonomic(r, dur=120.0, seed=seed)
    beats = au['beats'][au['beats'] < 8.0]
    t, sig, f = S.make_pcg(r['hemo'], fs=4000, seed=seed, beat_times=beats, resp_hz=r['art']['rr'] / 60.0, dur=8.0)
    wavfile.write(path, 4000, np.round(sig * 32767).astype(np.int16))
    return dict(hr=float(r['p']['hr']), beats=beats.tolist(), grade=f['grade'], murmur=f['murmur'], s3=bool(f['s3']), n=len(sig))

def rust_wav(hr, disease, seed, path):
    args = ['pcg', '--seed', str(seed), '--duration', '8']
    if hr: args += ['--hr', str(hr)]
    if disease: args += ['--disease', f'{disease[0]}:{disease[1]}']
    out = json.loads(subprocess.check_output(shlex.split(a.cmd) + args, text=True))
    raw = base64.b64decode(out['wav_base64'])
    open(path, 'wb').write(raw)
    fs, x = wavfile.read(io.BytesIO(raw))
    return dict(hr=out['hr'], beats=out['beats'], grade=out['grade'], murmur=out['murmur'], s3=out['s3'], n=len(x))

gen = python_wav if a.python else rust_wav
CASES = [('sain FC 72', 72, None), ('sain FC 90', 90, None), ('MMVD 0,3 (léger)', None, ('mmvd', 0.3)), ('MMVD 0,5 (modéré)', None, ('mmvd', 0.5))]
results = {}
tmp = tempfile.mkdtemp()
for label, hr, dis in CASES:
    rows = []
    for seed in range(a.seeds):
        path = os.path.join(tmp, f'{len(results)}_{seed}.wav')
        info = gen(hr, dis, seed, path)
        m = pcg_metrics.metrics(path)
        x = wavfile.read(path)[1]
        rr = np.diff(info['beats'])
        m.update(info, rr_median=float(np.median(rr)), rr_sd=float(np.std(rr)), clipped=int(np.sum(np.abs(x) >= 32767)),
                 finite=bool(np.isfinite(x.astype(float)).all()))
        rows.append(m)
    results[label] = rows

def med(rows, k):
    v = [r[k] for r in rows if r[k] is not None]
    return float(np.median(v)) if v else float('nan')

def band(rows, i):
    return float(np.median([r['bandes_20_50_50_100_100_200_200_400'][i] for r in rows]))

CRIT = [  # (libellé, extracteur, test, cible) — appliqués au chien sain
    ('Centroïde spectral (Hz)', lambda R: med(R, 'centroide_hz'), lambda v: 75 <= v <= 105, '90 ± 15'),
    ('Énergie 20–50 Hz', lambda R: band(R, 0), lambda v: v <= 0.08, '≤ 8 %'),
    ('Énergie 50–100 Hz', lambda R: band(R, 1), lambda v: 0.60 <= v <= 0.75, '60–75 %'),
    ('Énergie 100–200 Hz', lambda R: band(R, 2), lambda v: 0.20 <= v <= 0.35, '20–35 %'),
    ('Énergie 200–400 Hz', lambda R: band(R, 3), lambda v: v <= 0.05, '≤ 5 % (> 200 Hz)'),
    ('B2 > B1 (part des paires)', lambda R: med(R, 'part_B2_gt_B1'), lambda v: v >= 0.80, '≥ 80 %'),
    ('Rapport B2/B1 médian', lambda R: med(R, 'ratio_B2_B1_median'), lambda v: 1.5 <= v <= 2.3, '1,5–2,3'),
]
SYST = ('B1→B2 médian (s), FC 90', lambda R: med(R, 'systole_B1_B2_median_s'), lambda v: 0.15 <= v <= 0.25, '0,15–0,25')
DIAS = ('Diastole / systole', lambda R: (med(R, 'rr_median') - med(R, 'systole_B1_B2_median_s')) / med(R, 'systole_B1_B2_median_s'), lambda v: v > 1.8, '> 1,8')

table, bad = [], 0
fmt = lambda v: f'{100 * v:.1f} %' if 0 <= v < 1 and not np.isnan(v) else f'{v:.3f}'
for label, hr, dis in CASES:
    R = results[label]
    crits = CRIT + ([SYST] if hr == 90 else []) + [DIAS]
    for name, f, test, target in crits:
        v = f(R)
        ok = test(v) if not np.isnan(v) else False
        is_healthy = dis is None
        status = ('OK' if ok else 'ÉCHEC') if is_healthy else ('(dans la cible)' if ok else '(hors cible, indicatif)')
        if is_healthy and not ok: bad += 1
        table.append((label, name, target, fmt(v) if 'Énergie' in name or 'part' in name else f'{v:.3f}', status))
    extra = (f"FC {np.median([r['hr'] for r in R]):.1f}/min, SD RR {1000 * med(R, 'rr_sd'):.0f} ms, "
             f"grade {sorted(set(r['grade'] for r in R))}, souffle {sorted(set(r['murmur'] for r in R))}, "
             f"B3 {sorted(set(r['s3'] for r in R))}, écrêtage {sum(r['clipped'] for r in R)}, finis {all(r['finite'] for r in R)}, "
             f"échantillons {sorted(set(r['n'] for r in R))}")
    table.append((label, 'Informations', '', extra, ''))

src = 'référence Python (signals_pcg_v5.py)' if a.python else f'moteur Rust ({a.cmd})'
lines = [f'| Cas | Métrique | Cible | Obtenu (médiane de {a.seeds} graines) | Statut |', '|---|---|---|---|---|']
lines += [f'| {c} | {m} | {t} | {v} | {s} |' for c, m, t, v, s in table]
print(f'Source : {src}\n' + '\n'.join(lines))
print(f'\n{"Tous les critères du chien sain sont atteints." if not bad else f"{bad} critère(s) du chien sain non atteint(s)."}')
if a.report:
    with open(a.report, 'w') as fh:
        fh.write(f'# Conformité du module auscultation\n\nGénéré par `tests/pcg_acceptance.py` ; mesures par `reference/python/pcg_metrics.py`. Source : {src}.\n\n' + '\n'.join(lines) + '\n')
if a.json:
    json.dump({k: [{kk: vv for kk, vv in r.items() if kk != 'beats'} for r in v] for k, v in results.items()}, open(a.json, 'w'), indent=1, default=float)
sys.exit(1 if bad else 0)
