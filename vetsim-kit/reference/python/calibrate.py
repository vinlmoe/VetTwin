"""Banc de calibration / validation du patient sain et des modules maladie contre des données canines publiées.

Sorties : out/calibration.json et tableau imprimé.  Les cibles et leurs sources sont dans TARGETS.
Convention : 'tol' = demi-largeur d'acceptation (1 ET publié quand il est disponible, sinon tolérance choisie ici, marquée *).
"""
import json, sys, numpy as np
from multiprocessing import Pool
from patient import Patient
from autonomic import simulate_autonomic
import pkpd
import physio

BSA20 = 10.1 * 20000 ** (2 / 3) / 1e4      # m2, formule de Meeh (k=10,1)

# (module, variable, cible, tol, unité, source, extracteur(r))
T = [
 ('Hémodynamique', 'FC', 87, 10, '/min', 'Haskins 2005 (97 chiens)*', lambda r: r['p']['hr']),
 ('Hémodynamique', 'PAM', 103, 8, 'mmHg', 'Haskins 2005*', lambda r: r['hemo']['map']),
 ('Hémodynamique', 'Débit cardiaque', 4.42 * BSA20, 0.6, 'L/min', 'Haskins 2005 : IC 4,42 L/min/m2 x SC(20 kg)*', lambda r: r['hemo']['co']),
 ('Hémodynamique', 'PAP moy', 14, 3, 'mmHg', 'Haskins 2005*', lambda r: r['hemo']['pap_mean']),
 ('Hémodynamique', 'POG (PAOP)', 5.5, 2.5, 'mmHg', 'Haskins 2005*', lambda r: r['hemo']['la_mean']),
 ('Gaz du sang', 'pH artériel', 7.38, 0.03, '', 'Haskins 2005*', lambda r: r['art']['ph']),
 ('Gaz du sang', 'PaCO2', 40.2, 3, 'mmHg', 'Haskins 2005*', lambda r: r['art']['pco2']),
 ('Gaz du sang', 'PaO2', 99.5, 8, 'mmHg', 'Haskins 2005*', lambda r: r['art']['pao2']),
 ('Gaz du sang', 'pH veineux mêlé', 7.36, 0.03, '', 'Haskins 2005*', lambda r: r['ven']['ph']),
 ('Gaz du sang', 'PvCO2', 44.1, 4, 'mmHg', 'Haskins 2005*', lambda r: r['ven']['pco2']),
 ('Gaz du sang', 'PvO2', 49.3, 5, 'mmHg', 'Haskins 2005*', lambda r: r['ven']['po2']),
 ('Gaz du sang', 'Excès de base', -2.1, 2, 'mEq/L', 'Haskins 2005*', lambda r: r['art']['be']),
 ('Gaz du sang', 'P50 (pH 7,4)', 30.0, 1.3, 'mmHg', 'Cambier 2004 (ET publié)', lambda r: r['art']['p50'] * 10 ** (-0.37 * (7.4 - r['art']['ph']))),
 ('Acide-base', 'HCO3 art.', 22.4, 2, 'mmol/L', 'Constable & Stämpfli 2005*', lambda r: r['art']['hco3']),
 ('Acide-base', 'Na', 150.7, 3, 'mmol/L', 'Constable & Stämpfli 2005*', lambda r: r['chem']['na']),
 ('Acide-base', 'K', 4.07, 0.4, 'mmol/L', 'Constable & Stämpfli 2005*', lambda r: r['chem']['k']),
 ('Acide-base', 'Cl', 113.6, 2.5, 'mmol/L', 'Constable & Stämpfli 2005*', lambda r: r['chem']['cl']),
 ('Acide-base', 'Albumine', 37.7, 3, 'g/L', 'Constable & Stämpfli 2005*', lambda r: r['chem']['alb']),
 ('Acide-base', 'Phosphate', 1.42, 0.2, 'mmol/L', 'Constable & Stämpfli 2005*', lambda r: r['chem']['phos']),
 ('Acide-base', 'Trou anionique', 18.8, 3, 'mEq/L', 'Constable & Stämpfli 2005*', lambda r: r['chem']['anion_gap']),
 ('Métabolisme', 'VO2', 5.4, 0.8, 'mL/min/kg', 'dérivé des REE publiées (PMC6095264)*', lambda r: r['p']['vo2']),
]
TH = [  # variabilité cardiaque, 60 min de repos (Bogucki & Noszczyk-Nowak 2015, n=50, SD publiés)
 ('VFC', 'RR moyen', 677.7, 126.9, 'ms', lambda h: 60000.0 / h['fc_moy']),
 ('VFC', 'SDNN', 208.9, 77.1, 'ms', lambda h: h['sdnn_ms']),
 ('VFC', 'RMSSD', 259.0, 120.2, 'ms', lambda h: h['rmssd_ms']),
 ('VFC', 'LF/HF', 0.28, 0.11, '', lambda h: h['lf_hf']),
]


def _one(seed):
    pt = Patient(20.0, seed=seed)
    r = pt.state(signals=False)
    out = [f(r) for *_, f in T]
    au = simulate_autonomic(r, dur=300, seed=seed)['hrv'] if seed < 20 else None
    return out, au


def healthy_cohort(n=50):
    with Pool(2) as P:
        res = P.map(_one, range(n))
    rows = []
    X = np.array([r[0] for r in res], float)
    for j, (mod, var, tg, tol, unit, src, _) in enumerate(T):
        m, sd = X[:, j].mean(), X[:, j].std()
        rows.append(dict(module=mod, var=var, target=tg, tol=tol, unit=unit, source=src, model_mean=m, model_sd=sd,
                         ok=bool(abs(m - tg) <= tol), frac_in=float(np.mean(np.abs(X[:, j] - tg) <= 2 * tol))))
    H = [r[1] for r in res if r[1]]
    for mod, var, tg, sd, unit, f in TH:
        v = np.array([f(h) for h in H])
        rows.append(dict(module=mod, var=var, target=tg, tol=sd, unit=unit, source='Bogucki 2015 (ET publié)',
                         model_mean=v.mean(), model_sd=v.std(), ok=bool(abs(v.mean() - tg) <= sd),
                         frac_in=float(np.mean(np.abs(v - tg) <= 2 * sd))))
    return rows


def pk_checks():
    rows = []
    # pimobendan 0.5 mg/kg PO (Her 2020) : Cmax 49.1 ng/mL, Tmax 2.1 h, t1/2 1.8 h, AUC 148.4 ng.h/mL
    p = Patient(20.0, variability=False).p
    tg = np.linspace(0, 24, 481)
    for drug, dose, ref in [('pimobendan', 0.5, dict(cmax=(49.1, 'ng/mL', 1000, 'Her 2020'), tmax=(2.1, 'h', 1, 'Her 2020'), auc=(148.4, 'ng.h/mL', 1000, 'Her 2020'))),
                            ('furosemide', 5.0, dict(cmax=(2126, 'ng/mL', 1000, 'RCP Noah (5 mg/kg PO)'), tmax=(1.0, 'h', 1, 'RCP Noah 0,8-1,1 h'))),
                            ('furosemide', 1.9, dict(cmax=(900, 'ng/mL', 1000, 'RCP Noah (1,9 mg/kg PO)')))]:
        tc = pkpd.timecourse(p, [(0.0, drug, dose, 'po')], 24.0, list(tg))
        t = np.array(tc['t']); c = np.array(tc[drug])
        i = c.argmax()
        mod = dict(cmax=c.max() * 1000, tmax=t[i], auc=np.trapezoid(c, t) * 1000)
        for k, (tv, u, _, src) in ref.items():
            rows.append(dict(module='PK ' + drug, var=f'{k} ({dose} mg/kg PO)', target=tv, unit=u, source=src, model_mean=float(mod[k])))
    # furosémide IV 2 mg/kg : demi-vie 1-1,5 h (BMC Vet Res 2021)
    tc = pkpd.timecourse(p, [(0.0, 'furosemide', 2.0, 'iv')], 12.0, list(np.linspace(0, 12, 241)))
    t = np.array(tc['t']); c = np.array(tc['furosemide']); m = (t > 2) & (t < 8)
    k = -np.polyfit(t[m], np.log(c[m]), 1)[0]
    rows.append(dict(module='PK furosemide', var='t1/2 IV', target=1.25, unit='h', source='BMC Vet Res 2021 (1-1,5 h)', model_mean=float(np.log(2) / k)))
    return rows


def _dis(args):
    name, stage, seed = args
    pt = Patient(20.0, seed=seed)
    pt.add_disease(name, stage=stage)
    try:
        r = pt.state(signals=False)
    except physio.NonViable as e:
        return dict(dead=e.cause)            # décès mécanistique : compté à part, hors moyennes
    return dict(la=r['hemo']['la_mean'], rf=r['hemo']['reg_fraction'], map=r['hemo']['map'], co=r['hemo']['co'],
                creat=r['chem']['creat'], k=r['chem']['k'], na=r['chem']['na'], hco3=r['art']['hco3'], ph=r['art']['ph'],
                pao2=r['art']['pao2'], hr=r['p']['hr'], urea=r['chem']['urea'])


def disease_checks(n=24):
    jobs = [(d, s, 100 + i) for d, ss in [('MMVD', ['B1', 'B2', 'C', 'D']), ('CKD', ['IRIS1', 'IRIS2', 'IRIS3', 'IRIS4']),
                                          ('Addison', ['discret', 'modere', 'crise'])] for s in ss for i in range(n)]
    with Pool(2) as P:
        res = P.map(_dis, jobs)
    out, deaths = {}, {}
    for (d, s, _), r in zip(jobs, res):
        out.setdefault((d, s), [])
        if 'dead' in r:
            deaths.setdefault(f'{d}|{s}', []).append(r['dead'])
        else:
            out[(d, s)].append(r)
    return out, deaths


if __name__ == '__main__':
    rows = healthy_cohort(50)
    pk = pk_checks()
    dis, deaths = disease_checks(24)
    json.dump(dict(healthy=rows, pk=pk, disease={f'{a}|{b}': v for (a, b), v in dis.items()}, deaths=deaths),
              open('out/calibration.json', 'w'), indent=1, default=float)
    for r in rows:
        print(f"{r['module'][:14]:14s} {r['var'][:18]:18s} cible {r['target']:8.2f} ±{r['tol']:<6.2f} modèle {r['model_mean']:8.2f} (ET {r['model_sd']:.2f}) {'OK' if r['ok'] else '--'} {100*r['frac_in']:.0f}%")
    for r in pk:
        print(f"{r['module']:14s} {r['var'][:24]:24s} cible {r['target']:8.1f} modèle {r['model_mean']:8.1f}")
    for (d, s), v in dis.items():
        a = lambda k: np.array([x[k] for x in v])
        print(d, s, 'POG %.1f±%.1f RF %.2f PAM %.0f CO %.2f creat %.0f urée %.0f K %.1f Na %.0f Na:K %.1f pH %.3f HCO3 %.1f PaO2 %.0f' % (
            a('la').mean(), a('la').std(), a('rf').mean(), a('map').mean(), a('co').mean(), a('creat').mean(), a('urea').mean(), a('k').mean(), a('na').mean(), (a('na') / a('k')).mean(), a('ph').mean(), a('hco3').mean(), a('pao2').mean()))
