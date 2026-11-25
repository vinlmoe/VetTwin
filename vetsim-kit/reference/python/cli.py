"""CLI de référence (contrat : docs/CLI_CONTRACT.md). Le CLI Rust doit produire la même sortie JSON.
  python cli.py eval --bw 20 [--disease mmvd:0.5 ...] [--conc drug=mg_per_L ...] [--D mL] [--U mL_per_kg]
  python cli.py pk --bw 20 --drug furosemide --dose 2 --route iv --times 0,0.5,1,2
Mode déterministe : bruit nul, pas de variabilité interindividuelle."""
import sys, json, argparse
sys.path.insert(0, __import__('os').path.dirname(__file__))
import numpy as np
import diseases
from patient import Patient
import pkpd

class _Stub:
    def lognormal(self, *a): return 1.0
    def random(self): return 0.99

def flat(r):
    h, a, v, c = r['hemo'], r['art'], r['ven'], r['chem']
    d = dict(hr=r['p']['hr'], map=h['map'], sap=h['sap'], dap=h['dap'], co=h['co'], sv=h['sv'], edv=h['edv'], esv=h['esv'], ef=h['ef'],
             reg_fraction=h['reg_fraction'], la_mean=h['la_mean'], pap_mean=h['pap_mean'], cvp=h['cvp'], lv_edp=h['lv_edp'],
             ph=a['ph'], pco2=a['pco2'], hco3=a['hco3'], be=a['be'], pao2=a['pao2'], sao2=a['sao2'], p50=a['p50'], rr=a['rr'], lactate=a['lactate'],
             ph_v=v['ph'], pco2_v=v['pco2'], po2_v=v['po2'], hco3_v=v['hco3'],
             na=c['na'], k=c['k'], cl=c['cl'], glucose=c['glucose'], creat=c['creat'], urea=c['urea'], phos=c['phos'], alb=c['alb'],
             hct=c['hct'], gfr=c['gfr'], anion_gap=c['anion_gap'], do2_kg=r['do2_kg'], er=r['er'],
             s_tone=r['autonomic']['s'], v_tone=r['autonomic']['v'])
    import periph
    d.update(periph.peripheral(r['p'], r['autonomic']['s']))
    return {k: float(x) for k, x in d.items()}

def main(argv=None):
    ap = argparse.ArgumentParser(); sub = ap.add_subparsers(dest='cmd', required=True)
    e = sub.add_parser('eval'); e.add_argument('--bw', type=float, default=20.0); e.add_argument('--disease', action='append', default=[])
    e.add_argument('--conc', action='append', default=[]); e.add_argument('--D', type=float, default=0.0); e.add_argument('--U', type=float, default=0.0)
    k = sub.add_parser('pk'); k.add_argument('--bw', type=float, default=20.0); k.add_argument('--drug', required=True); k.add_argument('--dose', type=float, required=True)
    k.add_argument('--route', choices=['iv', 'po', 'im', 'sc'], required=True); k.add_argument('--times', required=True)
    k.add_argument('--disease', action='append', default=[])
    a = ap.parse_args(argv)
    diseases.KM['zsd'] = 0.0
    pt = Patient(a.bw, seed=0, variability=False); pt.rng = _Stub()
    for d in a.disease:
        name, s = d.split(':'); pt.add_disease({'mmvd': 'MMVD', 'ckd': 'CKD', 'addison': 'Addison', 'pneumonia': 'Pneumonie', 'atrial_fibrillation': 'FA', 'av_block': 'BAV', 'ventricular_premature': 'ESV', 'sick_sinus': 'SSS', 'pulmonary_hypertension': 'HTP'}[name], severity=float(s))
    if a.cmd == 'eval':
        conc = {kv.split('=')[0]: float(kv.split('=')[1]) for kv in a.conc}
        r = pt.state(conc, a.D, a.U, signals=False)
        print(json.dumps(flat(r)))
    else:
        times = [float(t) for t in a.times.split(',')]
        tc = pkpd.timecourse(pt.p, [(0.0, a.drug, a.dose, a.route)], max(times), times)
        idx = [int(np.argmin(abs(tc['t'] - t))) for t in times]
        print(json.dumps(dict(t=[float(tc['t'][i]) for i in idx], conc_mg_L=[float(tc[a.drug][i]) for i in idx])))

if __name__ == '__main__':
    main()
