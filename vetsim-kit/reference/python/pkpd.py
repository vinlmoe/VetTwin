"""PK/PD : modèle à 2 compartiments + dépôt oral, clairance modulée par fonction rénale/hépatique,
effet Emax sur des PARAMÈTRES physiologiques (ees, r_sys, volume, électrolytes, ventilation).

ATTENTION : toutes les valeurs ci-dessous sont des PLACEHOLDERS pédagogiques
(ordres de grandeur), pas des données validées. Ne pas utiliser pour dose réelle.
"""
import numpy as np
from scipy.integrate import solve_ivp
from params import copy_params

DRUGS = {
    # furosémide : PK et PD calées sur Koh 2021 (beagles, 2 mg/kg IV et per os ; tableaux 2, 4, 5, 6
    # du texte intégral) : voir fit_pk.py et fit_furosemide_pd.py. ec50/hill : au site d'effet (ke0).
    # cl rapportée à 20 kg (allométrie BW^0,75) : 735,7 mL/h/kg mesurés à 11,7 kg -> 10,72 mL/min/kg.
    'furosemide': dict(vc=0.207, vp=0.461, cl=10.72, q=3.343, ka=0.73, f=0.55,  # L/kg ; mL/min/kg ; 1/h
                       renal_exp=0.7, hep_exp=0.0, ec50=0.309, emax=1.0, hill=5.06, ke0=3.35),
    'pimobendan': dict(vc=2.0, vp=1.1, cl=27.9, q=16.7, ka=1.08, f=0.5,        # Her 2020 (0,5 mg/kg PO) ; F=0,5 supposé
                       renal_exp=0.0, hep_exp=1.0, ec50=0.03, emax=1.0, hill=1.2),  # PD : placeholder
    'atropine': dict(vc=0.70, vp=1.0, cl=8.0, q=10.0, ka=2.0, f=0.8,
                     renal_exp=0.3, hep_exp=0.5, ec50=0.004, emax=1.0, hill=1.5),
    'betabloquant': dict(vc=1.0, vp=1.2, cl=3.0, q=5.0, ka=0.5, f=0.6,
                         renal_exp=0.9, hep_exp=0.0, ec50=0.15, emax=0.85, hill=1.2),
    'opioide': dict(vc=3.0, vp=4.0, cl=25.0, q=30.0, ka=1.0, f=0.5,
                    renal_exp=0.0, hep_exp=1.0, ec50=0.015, emax=1.0, hill=1.3),
}
# Termes TOXIQUES : mécanismes distincts de l'effet thérapeutique, à concentrations plus hautes.
# T = emax·C^h/(EC50^h + C^h). PLACEHOLDERS (evidence D) : EC50 toxique = 10 × EC50 thérapeutique,
# aucune source ; ne jamais utiliser pour une dose réelle. Atropine : cibles toxiques (hyperthermie,
# iléus, effets centraux) non modélisées. Furosémide : toxicité volémique et électrolytique déjà
# émergente via D et U.
TOX = {
    'pimobendan': [dict(id='vasodilation', emax=1.0, ec50=0.3, hill=3.0)],
    'opioide': [dict(id='respiratory_arrest', emax=1.0, ec50=0.15, hill=3.0)],
    'betabloquant': [dict(id='cardiodepression', emax=1.0, ec50=1.5, hill=3.0)],
}
URINE_MAX = 0.200     # mL/kg/min à effet maximal (furosémide ; Koh 2021, diurèse induite par intervalle)
F_PLASMA = 0.3        # fraction du volume urinaire prélevée sur le volume sanguin
TAU_REFILL = 4.0      # h, retour du volume (apport hydrique, transfert interstitiel)
TAU_ELECT = 24.0      # h, récupération des pertes de Na+ et Cl- (placeholder ; Adin 2021 : Cl- incomplètement récupéré à 24 h)
TAU_K = 8.0           # h, récupération du K+ (U_K) : compromis entre Koh 2021 (plateau de ΔK sur 1–8 h, sujets à jeun)
                      # et Adin 2021 (K+ inchangé à 24 h) ; Roudebush 1994 : pas de variation notable sur 4 semaines


def _k(d, p):
    bw = p['bw']
    f = (bw / 20.0) ** 0.75
    cl_scale = (p['nephron_mass'] ** d['renal_exp']) * (p['hepatic_fn'] ** d['hep_exp'])
    cl = d['cl'] * 1e-3 * 60.0 * 20.0 * f * cl_scale      # L/h, allométrique BW^0.75
    vc, vp = d['vc'] * bw, d['vp'] * bw
    q = d['q'] * 1e-3 * bw * 60.0
    return cl, vc, vp, q


def effect(drug, conc):
    d = DRUGS[drug]
    c = max(conc, 0.0) ** d['hill']
    return d['emax'] * c / (d['ec50'] ** d['hill'] + c)


def tox_effect(t, conc):
    c = max(conc, 0.0) ** t['hill']
    return t['emax'] * c / (t['ec50'] ** t['hill'] + c)


def timecourse(p, doses, t_end_h, t_eval_h):
    """doses : liste de (t_h, drug, mg_per_kg, route 'iv'|'po').
    Renvoie dict de séries : conc[drug], D (déficit de volume, mL), U (diurèse effective, mL/kg,
    récupération TAU_ELECT : pertes de Na+ et Cl-) et UK (même diurèse, récupération TAU_K : pertes de K+)."""
    bw = p['bw']
    drugs = sorted({d[1] for d in doses})
    idx = {d: i for i, d in enumerate(drugs)}
    n = 4 * len(drugs) + 3           # (gut, central, periph, site d'effet) x drugs + D + U + UK
    state = np.zeros(n)
    events = sorted(doses)
    times = sorted({0.0, t_end_h, *[e[0] for e in events], *t_eval_h})
    ev_i = 0
    out = {k: [] for k in ['t', 'D', 'U', 'UK'] + drugs}
    eff = {d: [] for d in drugs}
    pars = {d: _k(DRUGS[d], p) for d in drugs}

    def rhs(t, y):
        dy = np.zeros(n)
        for d in drugs:
            i = 4 * idx[d]
            cl, vc, vp, q = pars[d]
            ka = DRUGS[d]['ka']
            gut, ac, ap, ce = y[i], y[i + 1], y[i + 2], y[i + 3]
            dy[i] = -ka * gut
            dy[i + 1] = ka * gut - (cl / vc) * ac - (q / vc) * ac + (q / vp) * ap
            dy[i + 2] = (q / vc) * ac - (q / vp) * ap
            # site d'effet (mg/L) : retard de l'effet sur le plasma (furosémide : action depuis
            # la lumière tubulaire après sécrétion) ; sans ke0, l'effet suit le plasma
            if 'ke0' in DRUGS[d]:
                dy[i + 3] = DRUGS[d]['ke0'] * (ac / vc - ce)
        D, U, UK = y[-3], y[-2], y[-1]
        u = 0.0
        if 'furosemide' in idx:
            i = 4 * idx['furosemide']
            u = URINE_MAX * effect('furosemide', y[i + 3])   # mL/kg/min, au site d'effet
        dy[-3] = F_PLASMA * u * bw * 60.0 - D / TAU_REFILL        # mL/h
        dy[-2] = u * 60.0 - U / TAU_ELECT                         # diurèse 'effective' (mL/kg), récupération de Na+, Cl-
        dy[-1] = u * 60.0 - UK / TAU_K                            # idem, récupération du K+ (apport, tampon intracellulaire)
        return dy

    def record(tt):
        out['t'].append(tt)
        out['D'].append(state[-3])
        out['U'].append(state[-2])
        out['UK'].append(state[-1])
        for d in drugs:
            out[d].append(state[4 * idx[d] + 1] / pars[d][1])

    for j, a in enumerate(times):
        while ev_i < len(events) and abs(events[ev_i][0] - a) < 1e-9:
            _, drug, dose, route = events[ev_i]
            i = 4 * idx[drug]
            mg = dose * bw
            if route == 'iv':
                state[i + 1] += mg
            else:
                state[i] += mg * DRUGS[drug]['f']
            ev_i += 1
        if a in t_eval_h:
            record(a)
        if j + 1 < len(times):
            b = times[j + 1]
            sol = solve_ivp(rhs, (a, b), state, method='LSODA', rtol=1e-8, atol=1e-10)
            state = sol.y[:, -1]
    return {k: np.array(v) for k, v in out.items()}


def apply_drug_effects(p, conc, D, U, UK=0.0):
    """Retourne une copie des paramètres modifiés par les concentrations et l'état de volume/diurèse."""
    q = copy_params(p)
    if 'pimobendan' in conc:
        e = effect('pimobendan', conc['pimobendan'])
        q['ees_lv'] *= 1 + 0.35 * e
        q['ees_rv'] *= 1 + 0.20 * e
        q['r_sys'] *= 1 - 0.20 * e
        t = tox_effect(TOX['pimobendan'][0], conc['pimobendan'])
        q['r_sys'] *= 1 - 0.8 * t             # toxique : vasodilatation excessive (inhibition PDE3)
    if 'opioide' in conc:
        e = effect('opioide', conc['opioide'])
        q['vent_drive'] *= 1 - 0.55 * e
        q['vagal_add'] += 0.25 * e            # augmentation du tonus vagal -> bradycardie, RSA accrue
        t = tox_effect(TOX['opioide'][0], conc['opioide'])
        q['vent_drive'] *= 1 - 0.99 * t        # toxique : dépression du centre respiratoire -> apnée
    if 'atropine' in conc:
        q['vagal_block'] = min(1.0, q['vagal_block'] + effect('atropine', conc['atropine']))
    if 'betabloquant' in conc:
        q['beta_block'] = min(1.0, q['beta_block'] + effect('betabloquant', conc['betabloquant']))
        t = tox_effect(TOX['betabloquant'][0], conc['betabloquant'])
        q['ees_lv'] *= 1 - 0.8 * t            # toxique : dépression myocardique directe (stabilisation de membrane)
        q['ees_rv'] *= 1 - 0.8 * t
        q['hr'] *= 1 - 0.8 * t                # toxique : dépression directe du nœud sinusal
    if D or U or UK:
        q['vol_shift'] -= D
        # pertes par mL/kg de diurèse effective, calées sur Koh 2021 (tableau 4, IV et per os, 1–8 h ;
        # K+ ajusté avec U_K, récupération TAU_K)
        q['k'] -= 0.034 * UK
        q['cl'] -= 0.236 * U
        q['na'] -= 0.073 * U
    return q
