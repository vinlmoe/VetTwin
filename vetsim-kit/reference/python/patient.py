"""Patient virtuel : base saine + modules maladie + médicaments -> variables cliniques."""
import numpy as np
from params import healthy_params, copy_params
from diseases import apply, REGISTRY
from physio import evaluate
from signals import make_ecg_conducted, make_pcg, patient_conduction
from autonomic import simulate_autonomic
import pkpd


class Patient:
    def __init__(self, bw=20.0, seed=0, variability=True):
        self.rng = np.random.default_rng(seed)
        self.p = healthy_params(bw)
        if variability:               # variabilité interindividuelle (CV ~ 6-10 %)
            for k, cv in [('ees_lv', .08), ('r_sys', .08), ('c_ao', .1), ('hr', .06), ('hb', .06),
                          ('creat0', .1), ('gfr0', .08), ('aagrad', .2), ('vco2', .05),
                          ('na', .006), ('k', .05), ('cl', .01), ('alb', .06), ('alt', .25)]:
                self.p[k] *= float(self.rng.lognormal(-0.5 * cv ** 2, cv))
        self.cases = []

    def add_disease(self, name, severity=None, stage=None):
        case = REGISTRY[name].sample(self.rng, severity, stage)
        self.p = apply(self.p, case)
        self.cases.append(case)
        return case

    def state(self, conc=None, D=0.0, U=0.0, signals=True):
        p = pkpd.apply_drug_effects(self.p, conc or {}, D, U) if (conc or D or U) else self.p
        r = evaluate(p)
        if signals:
            pe = r['p']
            au = simulate_autonomic(r, seed=int(self.rng.integers(1 << 30)))
            r['autonomic_ts'] = au
            cond = patient_conduction(r, au['beats'], seed=int(self.rng.integers(1 << 30)))
            r['conduction_ts'] = cond
            t, ecg, f = make_ecg_conducted(pe, cond, pe['hr'])
            r['ecg'] = dict(t=t, v=ecg, **f, hr=pe['hr'],
                            r_times=[b['t'] for b in cond['ventricular'] if b['t'] < 8.0])
            t2, pcg, feats = make_pcg(r['hemo'])
            r['pcg'] = dict(t=t2, v=pcg, **feats)
        return r

    def timecourse(self, doses, t_end_h, t_eval_h, signals=False):
        tc = pkpd.timecourse(self.p, doses, t_end_h, list(t_eval_h))
        drugs = [k for k in tc if k not in ('t', 'D', 'U')]
        out = []
        for i, t in enumerate(tc['t']):
            conc = {d: tc[d][i] for d in drugs}
            out.append((t, conc, self.state(conc, tc['D'][i], tc['U'][i], signals)))
        return tc, out


def summary(r):
    h, a, v, c = r['hemo'], r['art'], r['ven'], r['chem']
    return {
        'FC': round(r['p']['hr']), 'PAS': round(h['sap']), 'PAD': round(h['dap']), 'PAM': round(h['map']),
        'DC L/min': round(h['co'], 2), 'FE %': round(100 * h['ef']), 'Fraction régurg. %': round(100 * h['reg_fraction']),
        'POG moy': round(h['la_mean'], 1), 'DO2 mL/min/kg': round(r['do2_kg'], 1),
        'pH art': round(a['ph'], 3), 'PaCO2': round(a['pco2'], 1), 'HCO3': round(a['hco3'], 1), 'BE': round(a['be'], 1),
        'PaO2': round(a['pao2']), 'SaO2 %': round(a['sao2']), 'Lactate': round(a['lactate'], 1),
        'pH vein': round(v['ph'], 3), 'PvCO2': round(v['pco2'], 1), 'PvO2': round(v['po2']),
        'Na': round(c['na']), 'K': round(c['k'], 1), 'Cl': round(c['cl']), 'TA': round(c['anion_gap'], 1),
        'Créat µmol/L': round(c['creat']), 'Urée mmol/L': round(c['urea'], 1), 'Phos': round(c['phos'], 2),
        'Alb g/L': round(c['alb'], 1), 'Hct %': round(c['hct']), 'FR': round(a['rr']),
    }
