"""Comportements émergents de la couche de conduction (reference/python/rhythm.py).
Chaque critère est accompagné d'un contrôle d'échec : on retire le mécanisme et le critère doit tomber.
Usage : python tests/check_rhythm.py   (depuis la racine du kit) ; code de sortie != 0 en cas d'échec."""
import sys
sys.path.insert(0, 'reference/python')
import numpy as np
import params, rhythm

def run(v_eff=0.6, hr=88.0, t_end=300.0, seed=1, **kw):
    p = params.healthy_params(20); p.update(kw)
    ev = rhythm.simulate_conduction(p, v_eff, 60.0 / hr, t_end, np.random.default_rng(seed).random)
    return ev, rhythm.summarize(ev, 10.0, t_end)

bad = []
def check(name, ok, detail):
    print(('OK   ' if ok else 'ÉCHEC'), name, detail)
    if not ok: bad.append(name)

# 1. Chien sain : conduction 1:1, PR constant, rythme trivial
p = params.healthy_params(20)
check('sain 1:1 trivial', rhythm.is_trivial(p, 0.6, 60 / 88), '')
# 2. FA : FC ventriculaire et irrégularité dans l'IQR publié (129,7 [116,8-156,0] ; CVRR 0,309 [0,247-0,442])
hrs, cvs = zip(*[(s['hr_v'], s['cv_rr']) for s in (run(af=1.0, seed=k, t_end=400)[1] for k in range(6))])
check('FA FC dans l IQR', 116.8 <= np.mean(hrs) <= 156.0, f'{np.mean(hrs):.1f}')
check('FA CVRR dans l IQR', 0.247 <= np.mean(cvs) <= 0.442, f'{np.mean(cvs):.3f}')
_, s0 = run(af=1.0, av_conceal=0.0, t_end=400)
check('contrôle : sans conduction cachée, la FA sort de l IQR', not (0.247 <= s0['cv_rr'] <= 0.442), f"CVRR {s0['cv_rr']:.3f}")
# 3. BAV de 3e degré : échappement ventriculaire 42 ± 11 /min (n = 97). Fréquence OBSERVÉE, donc en
#    boucle fermée (baroréflexe, gain sympathique des foyers) ; la couche de conduction seule donne
#    la fréquence intrinsèque `vent_escape_rate` (27 /min depuis le 20/11/2026).
_, s = run(hp_conduction=0.0)
check('BAV3 échappement ventriculaire seul', set(s['origins']) == {'ventricular_escape'}, f"{s['hr_v']:.1f} {s['origins']}")
import cli, io, contextlib, json
_b = io.StringIO()
with contextlib.redirect_stdout(_b):
    cli.main(['eval', '--bw', '20', '--disease', 'av_block:1'])
_hr = json.loads(_b.getvalue())['hr']
check('BAV3 échappement en boucle fermée (42 ± 11)', 31 <= _hr <= 53, f"{_hr:.1f}")
_, s1 = run(hp_conduction=0.0, vent_escape_rate=0.0, t_end=60)
check('contrôle : sans foyer d échappement, asystolie', s1['n'] == 0, f"n={s1['n']}")
# 4. Wenckebach : PR croissant avant chaque onde P bloquée
ev, s = run(av_erp=0.45, hr=110, t_end=60)
prs = [b['pr'] for b in ev['ventricular'] if b['pr']]
check('Wenckebach : PR progressif', max(prs) - min(prs) > 0.03 and s['hr_v'] < 0.8 * 110, f'PR {min(prs):.3f}-{max(prs):.3f}, FCv {s["hr_v"]:.0f}')
ev0, _ = run(av_erp=0.45, av_beta=0.0, hr=110, t_end=60)
prs0 = [b['pr'] for b in ev0['ventricular'] if b['pr']]
check('contrôle : sans délai de récupération, PR fixe', max(prs0) - min(prs0) < 1e-9, f'{max(prs0) - min(prs0):.2e}')
# 5. Hyperkaliémie : arrêt atrial dès k_atrial_standstill, rythme sino-ventriculaire conservé
ev, s = run(k=9.0)
check('arrêt atrial à K 9', s['hr_a'] == 0.0 and s['origins'].get('sinus', 0) > 0, f"FCa {s['hr_a']}, {s['origins']}")
ev, s = run(k=10.8)
check('bradycardie à K 10,8 (bloc infranodal, échappement)', s['hr_v'] < 70, f"FCv {s['hr_v']:.0f} {s['origins']}")
# 6. Maladie du sinus : pauses au-delà de la plus longue pause du chien sain (3,8 ± 0,8 s)… avec échappement jonctionnel
_, s = run(sa_block=0.5, junct_rate=0.0, vent_escape_rate=0.0, t_end=600)
check('maladie du sinus : pauses longues sans échappement', s['max_rr'] > 3.8, f"pause max {s['max_rr']:.2f} s")
print('OK' if not bad else f'{len(bad)} échec(s) : {bad}')
sys.exit(1 if bad else 0)
