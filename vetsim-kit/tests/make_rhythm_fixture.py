"""Fixture de la couche de conduction : tirages uniformes enregistrés et événements produits par
reference/python/rhythm.py. Le test Rust rejoue les mêmes tirages et compare les événements.
Sortie : crates/vetsim-core/tests/fixtures/python_reference_rhythm.json
Usage : python tests/make_rhythm_fixture.py   (depuis la racine du kit)"""
import json, sys
sys.path.insert(0, 'reference/python')
import numpy as np
import params, rhythm

SCEN = [
    dict(id='sain', hr=88.0, v_eff=0.6, t_end=30.0),
    dict(id='tachy_vagal0', hr=230.0, v_eff=0.0, t_end=20.0),
    dict(id='fa', hr=88.0, v_eff=0.6, t_end=60.0, over=dict(af=1.0)),
    dict(id='fa_vagal1', hr=88.0, v_eff=1.0, t_end=60.0, over=dict(af=1.0)),
    dict(id='wenckebach', hr=110.0, v_eff=0.6, t_end=30.0, over=dict(av_erp=0.45)),
    dict(id='mobitz2', hr=88.0, v_eff=0.6, t_end=60.0, over=dict(hp_conduction=0.7)),
    dict(id='bav3', hr=88.0, v_eff=0.6, t_end=60.0, over=dict(hp_conduction=0.0)),
    dict(id='sinus', hr=88.0, v_eff=0.6, t_end=60.0, over=dict(sa_block=0.3)),
    dict(id='esv', hr=88.0, v_eff=0.6, t_end=60.0, over=dict(vpc_rate=6.0)),
    dict(id='hypok', hr=88.0, v_eff=0.6, t_end=120.0, over=dict(k=2.0)),
    dict(id='k9', hr=88.0, v_eff=0.6, t_end=30.0, over=dict(k=9.0)),
    dict(id='k10_8', hr=88.0, v_eff=0.6, t_end=60.0, over=dict(k=10.8)),
    dict(id='bw5_fa', hr=132.0, v_eff=0.5, t_end=40.0, bw=5.0, over=dict(af=1.0)),
]

def main():
    out = {'note': __doc__.splitlines()[0], 'cases': []}
    for i, sc in enumerate(SCEN):
        p = params.healthy_params(sc.get('bw', 20.0)); p.update(sc.get('over', {}))
        g = np.random.default_rng(1000 + i); draws = []
        def rand():
            u = float(g.random()); draws.append(u); return u
        ev = rhythm.simulate_conduction(p, sc['v_eff'], 60.0 / sc['hr'], sc['t_end'], rand)
        out['cases'].append(dict(id=sc['id'], bw=sc.get('bw', 20.0), over=sc.get('over', {}), v_eff=sc['v_eff'],
                                 sinus_period=60.0 / sc['hr'], t_end=sc['t_end'], draws=draws,
                                 atrial=[[t, o] for t, o in ev['atrial']],
                                 ventricular=[[b['t'], b['origin'], b['t_a'], b['pr'], b['wide']] for b in ev['ventricular']],
                                 trivial=rhythm.is_trivial(p, sc['v_eff'], 60.0 / sc['hr'])))
        print(sc['id'], len(draws), 'tirages,', len(ev['atrial']), 'A,', len(ev['ventricular']), 'V')
    dst = 'crates/vetsim-core/tests/fixtures/python_reference_rhythm.json'
    with open(dst, 'w') as f:
        json.dump(out, f, ensure_ascii=False); f.write('\n')

if __name__ == '__main__':
    main()
