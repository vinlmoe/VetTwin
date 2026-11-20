"""Génère des cas hors golden pour le jalon M3 (hémodynamique + baroréflexe), avec le CLI de référence.
Exécuter depuis la racine du kit :  python tests/make_m3_fixture.py
Sortie : crates/vetsim-core/tests/fixtures/python_reference_m3.json
But : couvrir les zones sensibles (crise addisonienne, MMVD stade D, combinaisons, poids extrêmes)
que le golden (sévérités 0,2/0,5/0,8 à 20 kg) n'explore pas. Comparé avec les tolérances du golden.
Pour chaque cas, on enregistre aussi le nombre de passages du baroréflexe et la marge du critère d'arrêt
(min |(|d_t - d|) - 0,04|) : près de cette marge, la sortie de la référence est discontinue."""
import json, subprocess, sys
sys.path.insert(0, 'reference/python')
import params as PY, diseases as D, physio
CLI = [sys.executable, 'reference/python/cli.py']
NAMES = {'mmvd': 'MMVD', 'ckd': 'CKD', 'addison': 'Addison', 'pneumonia': 'Pneumonie'}

class Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

def baro_trace(a):
    """Rejoue physio.evaluate (référence inchangée) en espionnant simulate_hemo."""
    D.KM['zsd'] = 0.0
    bw = float(a[a.index('--bw') + 1]) if '--bw' in a else 20.0
    p = PY.healthy_params(bw)
    for i, x in enumerate(a):
        if x == '--disease':
            n, s = a[i + 1].split(':'); p = D.apply(p, D.REGISTRY[NAMES[n]].sample(Stub(), severity=float(s)))
    maps, orig = [], physio.simulate_hemo
    physio.simulate_hemo = lambda q, n_beats=22, dt=0.002, pr=None: (lambda h: (maps.append(h['map']), h)[1])(orig(q, n_beats, dt, pr))
    try:
        physio.evaluate(p)
    finally:
        physio.simulate_hemo = orig
    d, margin = 0.0, float('inf')
    for m in maps:
        dt = min(1.0, max(-1.0, p['baro_gain'] * (p['map_set'] - m) / 45.0))
        margin = min(margin, abs(abs(dt - d) - 0.04))
        if abs(dt - d) < 0.04: break
        d = 0.5 * d + 0.5 * dt
    return len(maps), margin
ARGS = [
    ['--bw', '2.5'], ['--bw', '63'], ['--bw', '10', '--disease', 'mmvd:0.65'],
    ['--disease', 'mmvd:0.95'], ['--disease', 'mmvd:1.0'], ['--disease', 'ckd:0.95'],
    ['--disease', 'addison:0.9'], ['--disease', 'addison:0.95'], ['--disease', 'addison:1.0'],
    ['--disease', 'pneumonia:1.0'], ['--disease', 'mmvd:0.8', '--disease', 'ckd:0.6'],
    ['--bw', '35', '--disease', 'addison:0.7', '--disease', 'pneumonia:0.5'],
    ['--bw', '8', '--disease', 'ckd:0.9', '--disease', 'mmvd:0.5'],
]
cases = []
for a in ARGS:
    args = ['eval'] + a
    n, margin = baro_trace(a)
    cases.append(dict(args=args, baro_passes=n, baro_margin=margin,
                      expected=json.loads(subprocess.check_output(CLI + args, text=True))))
dst = 'crates/vetsim-core/tests/fixtures/python_reference_m3.json'
with open(dst, 'w') as f:
    json.dump(dict(note=__doc__.splitlines()[0], cases=cases), f, indent=1); f.write('\n')
print(dst, len(cases), 'cas')
