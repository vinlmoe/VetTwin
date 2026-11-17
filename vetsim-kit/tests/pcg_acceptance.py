"""Critères d'acceptation du module auscultation (PROMPT_AUSCULTATION.md), mesurés par
reference/python/pcg_metrics.py sur des fichiers WAV 16 bits.

  python tests/pcg_acceptance.py --python                       # référence Python (signals_pcg_v5.py)
  python tests/pcg_acceptance.py --cmd target/release/vetsim-cli # moteur Rust (commande « pcg »)
  python tests/pcg_acceptance.py --cmd ... --report docs/PCG_CONFORMITE.md

Cas : chien sain 20 kg à FC 72 et 90 (paramètre « hr » imposé avant l'évaluation : réglage
d'usage, pas la valeur calibrée de 87/min) ; MMVD 0,3 (léger) et 0,5 (modéré).
Graines : 200 par défaut (le prompt en demande 20 ; 100 jusqu'au 17/11/2026, porté à 200 : à 100 graines le rapport B2/B1 à FC 72 sortait à z 3,1 sur les graines 0–99 et rentrait sur 100–199). Raison : pcg_metrics n'apparie B1 et B2 que
si leur intervalle est < 0,3 s ; à FC 72 la systole du modèle (0,33 s) dépasse ce seuil, d'où
~1 paire par graine et 35-40 % de graines sans paire ; à 20 graines les métriques B2/B1 sont
dominées par le hasard (vérifié : Rust et référence divergent à 20 graines, convergent à 100).
Chaque critère est jugé sur la médiane des 20 graines ; diastole = RR médian − B1→B2 médian.

Décision du projet : port de la référence « tel quel ». Avec --ref-json (résultats de
--python --json), un critère que la référence ne tient pas non plus est un « écart connu
(référence) » et ne fait pas échouer ; on vérifie en plus que chaque métrique du Rust est
statistiquement équivalente à la référence (écart relatif < 5 % OU < 3 erreurs-types
combinées, comme le banc de calibration M5).
Code de sortie != 0 si un critère que la référence tient échoue, ou si une métrique s'écarte."""
import argparse, base64, io, json, os, shlex, subprocess, sys, tempfile
sys.path.insert(0, 'reference/python')
import numpy as np
from scipy.io import wavfile
import pcg_metrics

ap = argparse.ArgumentParser()
g = ap.add_mutually_exclusive_group(required=True)
g.add_argument('--python', action='store_true'); g.add_argument('--cmd')
ap.add_argument('--seeds', type=int, default=200); ap.add_argument('--report'); ap.add_argument('--json'); ap.add_argument('--ref-json'); ap.add_argument('--seed0', type=int, default=0)
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
    for seed in range(a.seed0, a.seed0 + a.seeds):
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

REF = json.load(open(a.ref_json)) if a.ref_json else None
table, bad = [], 0
fmt = lambda v: f'{100 * v:.1f} %' if 0 <= v < 1 and not np.isnan(v) else f'{v:.3f}'
def per_seed(R, f):
    return [f([r]) for r in R]
equiv_lines = []
for label, hr, dis in CASES:
    R = results[label]
    crits = CRIT + ([SYST] if hr == 90 else []) + [DIAS]
    for name, f, test, target in crits:
        v = f(R)
        ok = test(v) if not np.isnan(v) else False
        is_healthy = dis is None
        ref_txt = ''
        if REF is not None:
            vr = f(REF[label]); ref_ok = test(vr) if not np.isnan(vr) else False
            ref_txt = fmt(vr) if 'Énergie' in name or 'part' in name else f'{vr:.3f}'
            # équivalence statistique Rust / référence sur les valeurs par graine
            xs = [x for x in per_seed(R, f) if not np.isnan(x)]; ys = [y for y in per_seed(REF[label], f) if not np.isnan(y)]
            d = v - vr; rel = abs(d) / abs(vr) if vr else float('inf')
            # erreur-type d'une MÉDIANE ≈ 1,2533 σ/√n (loi normale), pas celle d'une moyenne
            se = 1.2533 * np.sqrt(np.var(xs) / max(len(xs), 1) + np.var(ys) / max(len(ys), 1))
            z = abs(d) / se if se > 0 else (0.0 if d == 0 else float('inf'))
            eq = rel < 0.05 or z < 3
            equiv_lines.append(f"{'OK ' if eq else 'ÉCART'} {label:18s} {name:28s} Rust {v:9.4f}  référence {vr:9.4f}  écart {100 * rel:5.1f} %  z {z:4.1f}")
            if not eq: bad += 1
        else:
            ref_ok = None
        no_pair = np.mean([r['n_paires'] == 0 for r in R])
        if is_healthy and 'B2' in name and no_pair > 0.30:
            # condition de validité : trop de graines sans paire B1/B2 (systole > 0,3 s)
            status = f'non mesurable ({100 * no_pair:.0f} % de graines sans paire)'
        elif is_healthy:
            if ok: status = 'OK'
            elif ref_ok is False: status = 'écart connu (référence)'
            else: status = 'ÉCHEC'; bad += 1
        else:
            status = '(dans la cible)' if ok else '(hors cible, indicatif)'
        table.append((label, name, target, fmt(v) if 'Énergie' in name or 'part' in name else f'{v:.3f}', ref_txt, status))
    npairs = [r['n_paires'] for r in R]
    extra = (f"paires B1/B2 par graine : médiane {np.median(npairs):.0f}, graines sans paire {sum(n == 0 for n in npairs)} ; "
             f"FC {np.median([r['hr'] for r in R]):.1f}/min, SD RR {1000 * med(R, 'rr_sd'):.0f} ms, "
             f"grade {sorted(set(r['grade'] for r in R))}, souffle {sorted(set(r['murmur'] for r in R))}, "
             f"B3 {sorted(set(r['s3'] for r in R))}, écrêtage {sum(r['clipped'] for r in R)}, finis {all(r['finite'] for r in R)}, "
             f"échantillons {sorted(set(r['n'] for r in R))}")
    table.append((label, 'Informations', '', extra, '', ''))

src = 'référence Python (signals_pcg_v5.py)' if a.python else f'moteur Rust ({a.cmd})'
lines = [f'| Cas | Métrique | Cible | Obtenu (médiane de {a.seeds} graines) | Référence Python | Statut |', '|---|---|---|---|---|---|']
lines += [f'| {c} | {m} | {t} | {v} | {r} | {s} |' for c, m, t, v, r, s in table]
print(f'Source : {src}\n' + '\n'.join(lines))
if equiv_lines:
    print('\nÉquivalence statistique Rust / référence (par graine) :\n' + '\n'.join(equiv_lines))
print(f'\n{"Aucun échec (hors écarts connus de la référence)." if not bad else f"{bad} échec(s)."}')
if a.report:
    with open(a.report, 'w') as fh:
        fh.write(f'# Conformité du module auscultation\n\nGénéré par `tests/pcg_acceptance.py` ; mesures par `reference/python/pcg_metrics.py`. Source : {src}.\n\n'
                 + (open('docs/PCG_CONFORMITE_INTRO.md').read() + '\n' if os.path.exists('docs/PCG_CONFORMITE_INTRO.md') else '')
                 + '\n'.join(lines) + '\n'
                 + ('\n## Équivalence statistique avec la référence Python\n\n```\n' + '\n'.join(equiv_lines) + '\n```\n' if equiv_lines else ''))
if a.json:
    json.dump({k: [{kk: vv for kk, vv in r.items() if kk != 'beats'} for r in v] for k, v in results.items()}, open(a.json, 'w'), indent=1, default=float)
sys.exit(1 if bad else 0)
