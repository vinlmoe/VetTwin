"""PK/PD : modèle à 2 compartiments + dépôt oral, clairance modulée par fonction rénale/hépatique,
effet Emax sur des PARAMÈTRES physiologiques (ees, r_sys, volume, électrolytes, ventilation).

ATTENTION : toutes les valeurs ci-dessous sont des PLACEHOLDERS pédagogiques
(ordres de grandeur), pas des données validées. Ne pas utiliser pour dose réelle.
"""
import numpy as np
from scipy.integrate import solve_ivp
from params import copy_params

DRUGS = {
    # furosémide : PK et PD calées sur Koh 2021 (BMC Vet Res 17:295, texte intégral) et Wong 1977 ;
    # voir modules/drugs/furosemide.json (notes) et docs/VERIFICATION_MALADIES_MEDICAMENTS.tex
    'furosemide': dict(vc=0.2073, vp=0.4602, cl=10.724, q=3.341, ka=0.724, f=0.55,   # L/kg ; mL/min/kg ; 1/h
                       renal_exp=0.7, hep_exp=0.0, ec50=0.2412, emax=1.0, hill=3.0),
    'pimobendan': dict(vc=2.0, vp=1.1, cl=27.9, q=16.7, ka=1.08, f=0.5,        # Her 2020 (0,5 mg/kg PO) ; F=0,5 supposé
                       renal_exp=0.0, hep_exp=1.0, ec50=0.03, emax=1.0, hill=1.2),  # PD : placeholder
    # atropine 1.1.0 : PK Thiermann 1996 (V 3,2 L/kg, Cl 44, ka IM 5,94/h) ; blocage au site d'effet
    'atropine': dict(vc=1.0, vp=2.2, cl=44.0, q=44.0, ka=2.0, f=0.8, ka_im=5.94, f_im=1.0, ke0=20.0,
                     renal_exp=0.3, hep_exp=0.5, ec50=0.0106, emax=1.0, hill=1.5),
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
    'opioide': [dict(id='respiratory_depression', emax=1.0, ec50=1.0, hill=3.0)],   # chien vigile : pas d'apnée (Bailey 1987)
    # atropine : termes secondaires (sources dans modules/drugs/atropine.json)
    'atropine': [dict(id='paradoxical_vagal', emax=1.0, ec50=0.004, hill=2.0, site='plasma'),
                 dict(id='antimuscarinic_peripheral', emax=1.0, ec50=0.0015, hill=1.5, site='effect'),
                 dict(id='excess_tachycardia', emax=1.0, ec50=0.045, hill=3.0, site='plasma')],
}
VB_ML_KG = 85.0
URINE_MAX = 0.2044    # mL/kg/min à effet maximal (furosémide, Koh 2021)
F_PLASMA = 0.3        # fraction du volume urinaire prélevée sur le volume sanguin
TAU_REFILL = 4.0      # h, retour du volume (apport hydrique, transfert interstitiel)
TAU_ELECT = 200.0     # h, oubli de la diurèse effective (Koh 2021 : pas de récupération à jeun sur 8 h)
TAU_ADAPT, PHI_ADAPT = 12.0, 0.9808   # adaptation (aldostérone, apport, FE_K) : U_eff = U − φ·W ; Adin 2020, TEST 2017
A_K, A_CL, A_NA = 0.00585, 0.00225, 0.00032   # pertes proportionnelles : X ← X·exp(−a·U_eff) (par mL/kg)


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


S = 5   # états par médicament : dépôt digestif, central, périphérique, dépôt IM, Ce (site d'effet)


def timecourse(p, doses, t_end_h, t_eval_h):
    """doses : liste de (t_h, drug, mg_per_kg, route 'iv'|'po'|'im').
    Renvoie dict de séries : conc[drug], conc au site d'effet [drug + '@e'], D (déficit de volume,
    mL), U (diurèse cumulée mL/kg)."""
    bw = p['bw']
    drugs = sorted({d[1] for d in doses})
    idx = {d: i for i, d in enumerate(drugs)}
    n = S * len(drugs) + 3           # (gut, central, periph, im, Ce) x drugs + D + U + W (adaptation)
    state = np.zeros(n)
    events = sorted(doses)
    times = sorted({0.0, t_end_h, *[e[0] for e in events], *t_eval_h})
    ev_i = 0
    out = {k: [] for k in ['t', 'D', 'U'] + drugs + [d + '@e' for d in drugs]}
    eff = {d: [] for d in drugs}
    pars = {d: _k(DRUGS[d], p) for d in drugs}

    def rhs(t, y):
        dy = np.zeros(n)
        for d in drugs:
            i = S * idx[d]
            cl, vc, vp, q = pars[d]
            ka = DRUGS[d]['ka']
            ka_im = DRUGS[d].get('ka_im', 0.0)
            gut, ac, ap, im, ce = y[i], y[i + 1], y[i + 2], y[i + 3], y[i + 4]
            dy[i] = -ka * gut
            dy[i + 1] = ka * gut + ka_im * im - (cl / vc) * ac - (q / vc) * ac + (q / vp) * ap
            dy[i + 2] = (q / vc) * ac - (q / vp) * ap
            dy[i + 3] = -ka_im * im
            dy[i + 4] = DRUGS[d]['ke0'] * (ac / vc - ce) if 'ke0' in DRUGS[d] else 0.0
        D, U, W = y[-3], y[-2], y[-1]
        u = 0.0
        if 'furosemide' in idx:
            i = S * idx['furosemide']
            # site d'action luminal : effet piloté par l'excrétion urinaire (C × masse néphronique)
            u = URINE_MAX * effect('furosemide', y[i + 1] / pars['furosemide'][1] * p['nephron_mass'])   # mL/kg/min
        dy[-3] = F_PLASMA * u * bw * 60.0 - D / TAU_REFILL        # mL/h
        dy[-2] = u * 60.0 - U / TAU_ELECT                         # diurèse 'effective' (mL/kg)
        if 'furosemide' in idx:
            dy[-1] = (U - W) / TAU_ADAPT                           # adaptation lente
        return dy

    def record(tt):
        out['t'].append(tt)
        out['D'].append(state[-3])
        out['U'].append(state[-2] - (PHI_ADAPT * state[-1] if 'furosemide' in idx else 0.0))
        for d in drugs:
            c = state[S * idx[d] + 1] / pars[d][1]
            out[d].append(c)
            out[d + '@e'].append(state[S * idx[d] + 4] if 'ke0' in DRUGS[d] else c)

    for j, a in enumerate(times):
        while ev_i < len(events) and abs(events[ev_i][0] - a) < 1e-9:
            _, drug, dose, route = events[ev_i]
            i = S * idx[drug]
            mg = dose * bw
            if route == 'iv':
                state[i + 1] += mg
            elif route in ('im', 'sc'):
                if 'ka_im' not in DRUGS[drug]:
                    raise ValueError(f'voie IM non décrite pour {drug}')
                state[i + 3] += mg * DRUGS[drug].get('f_im', 1.0)
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


def apply_drug_effects(p, conc, D, U, conc_e=None):
    """Retourne une copie des paramètres modifiés par les concentrations et l'état de volume/diurèse.
    `conc_e` : concentrations au site d'effet (médicaments à `ke0`) ; à défaut Ce = C."""
    q = copy_params(p)
    ce = lambda d: (conc_e or {}).get(d, conc.get(d, 0.0))   # noqa: E731
    if 'pimobendan' in conc:
        e = effect('pimobendan', conc['pimobendan'])
        q['ees_lv'] *= 1 + 0.35 * e
        q['ees_rv'] *= 1 + 0.20 * e
        q['r_sys'] *= 1 - 0.20 * e
        q['pr'] *= 1 - 0.25 * e                # PQ −11,8 % à 60 min (Pichayapaiboon 2021)
        q['av_erp'] *= 1 - 0.25 * e            # réfractarité nodale ↓ (Kitzen 1988)
        t = tox_effect(TOX['pimobendan'][0], conc['pimobendan'])
        q['r_sys'] *= 1 - 0.8 * t             # toxique : vasodilatation excessive (inhibition PDE3)
    if 'opioide' in conc:
        e = effect('opioide', conc['opioide'])
        q['vent_drive'] *= 1 - 0.25 * e       # chien vigile : PaCO2 +2,5 à +3 mmHg (Wunsch 2010, Maiante 2009)
        q['vagal_add'] += 0.27 * e            # tonus vagal accru -> bradycardie (FC −32 % : Mills 2025)
        q['r_sys'] *= 1 + 0.36 * e            # vasoconstriction : PAM conservée (Maiante 2009, Garofalo 2012)
        q['t_set'] += -2.0 * e                 # consigne thermique abaissée (Amon 2021, Lucas 2001)
        q['pupil_ew'] += 1.0 * e               # myosis (noyau d'Edinger-Westphal, Lee & Wang 1975)
        q['gi_inhib'] = min(1.0, max(0.0, q['gi_inhib'] + 0.9 * e))   # CMM inhibé (Sarna 1985)
        q['sedation'] += 0.2 * e               # sédation légère du vigile
        t = tox_effect(TOX['opioide'][0], conc['opioide'])
        q['vent_drive'] *= 1 - 0.8 * t         # toxique : plateau de PaCO2 ≈ 65 mmHg, sans apnée (Bailey 1987)
    if 'atropine' in conc:
        e = effect('atropine', ce('atropine'))            # blocage au site d'effet (ke0)
        q['vagal_block'] = min(1.0, q['vagal_block'] + e)
        q['pupil_block'] = min(1.0, max(0.0, q['pupil_block'] + e))
        T = {t['id']: tox_effect(t, conc['atropine'] if t['site'] == 'plasma' else ce('atropine'))
             for t in TOX['atropine']}
        q['vagal_add'] += 0.2 * T['paradoxical_vagal']
        q['musc_block'] = min(1.0, max(0.0, q['musc_block'] + T['antimuscarinic_peripheral']))
        q['hr'] *= 1 + 0.2 * T['excess_tachycardia']
    if 'betabloquant' in conc:
        q['beta_block'] = min(1.0, q['beta_block'] + effect('betabloquant', conc['betabloquant']))
        # profil aténolol : pas de terme toxique de stabilisation de membrane (propranolol)
    if 'furosemide' in conc:
        # veinodilatation dépendante du rein (Bayne 1979) : E suit l'excrétion urinaire
        q['ven_cap'] *= 1 + 0.36 * effect('furosemide', conc['furosemide'] * p['nephron_mass'])
    if D or U:
        q['vol_shift'] -= D
        vb = VB_ML_KG * p['bvf'] * p['bw']        # hémoconcentration : D prélevé sur le plasma
        vp = vb * (1 - 3.0 * p['hb'] / 100.0)
        q['hb'] *= vb / (vb - D)
        q['alb'] *= vp / (vp - D)
        q['k'] *= np.exp(-A_K * U)        # pertes proportionnelles à la concentration : jamais négatives
        q['cl'] *= np.exp(-A_CL * U)
        q['na'] *= np.exp(-A_NA * U)
    return q
