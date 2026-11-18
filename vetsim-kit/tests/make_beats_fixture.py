"""Fixture de l'hémodynamique battement par battement (physio.simulate_hemo_beats) sur des
séquences de conduction à tirages enregistrés. Oreillette active (k_la = 1,45, calé sur E/A).
Sortie : crates/vetsim-core/tests/fixtures/python_reference_beats.json
Usage : python tests/make_beats_fixture.py   (depuis la racine du kit)"""
import json, sys, warnings
sys.path.insert(0, 'reference/python')
warnings.filterwarnings('ignore')
import numpy as np
import params, physio, rhythm

SCEN = [
    dict(id='sinus88', hr=88.0, over={}),
    dict(id='sinus130_sans_oreillette', hr=130.0, over=dict(k_la=0.0, k_ra=0.0)),
    dict(id='fa', hr=88.0, over=dict(af=1.0)),
    dict(id='bav1', hr=88.0, over=dict(pr=0.25, av_beta=0.0)),
    dict(id='bav3', hr=88.0, over=dict(hp_conduction=0.0)),
    dict(id='esv', hr=88.0, over=dict(vpc_rate=10.0)),
    dict(id='mmvd_like_fa', hr=110.0, over=dict(af=1.0, g_mr=2.0)),
]
KEYS = ['map', 'sap', 'dap', 'co', 'edv', 'esv', 'sv', 'la_mean', 'pap_mean', 'cvp', 'lv_edp', 'reg_fraction', 'mitral_e', 'mitral_a', 'hr_v']

def main():
    out = {'note': __doc__.splitlines()[0], 't_end': 30.0, 'warm': 12.0, 'cases': []}
    for i, sc in enumerate(SCEN):
        p = params.healthy_params(20); p.update(k_la=1.45, k_ra=0.29); p.update(sc['over'])
        g = np.random.default_rng(2000 + i); draws = []
        def rand():
            u = float(g.random()); draws.append(u); return u
        T = 60.0 / sc['hr']
        cond = rhythm.simulate_conduction(p, 0.6, T, out['t_end'], rand)
        h = physio.simulate_hemo_beats(p, cond, out['t_end'], out['warm'], T)
        out['cases'].append(dict(id=sc['id'], over=dict(sc['over'], k_la=p['k_la'], k_ra=p['k_ra']), sinus_period=T, draws=draws,
                                 summary={k: float(h[k]) for k in KEYS}, beat_sv=[float(b['sv']) for b in h['beats']]))
        print(sc['id'], ' '.join(f'{k}={h[k]:.3g}' for k in ('co', 'map', 'sv', 'hr_v')))
    with open('crates/vetsim-core/tests/fixtures/python_reference_beats.json', 'w') as f:
        json.dump(out, f, ensure_ascii=False); f.write('\n')

if __name__ == '__main__':
    main()
