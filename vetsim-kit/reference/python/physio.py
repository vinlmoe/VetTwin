"""Moteur physiologique : hémodynamique (boucle fermée à élastances variables),
oxygénation, acide-base (Stewart), fonction rénale, lactate."""
import math
import numpy as np
from scipy.integrate import solve_ivp
from scipy.optimize import brentq


# ------------------------------------------------------------------ cardio
def _act(t, T, ts, tr):
    tc = np.mod(t, T)
    return np.where(tc < ts, 0.5 * (1 - np.cos(np.pi * tc / ts)),
                    np.where(tc < ts + tr, 0.5 * (1 + np.cos(np.pi * (tc - ts) / tr)), 0.0))


def _vent_p(V, e, ees, v0, a, b):
    x = np.clip(b * (V - v0), -20, 8)
    return e * ees * (V - v0) + (1 - e) * a * (np.exp(x) - 1.0)


def _valve(dp, r_f, g_b, eps=0.3):
    s = 0.5 * (1 + np.tanh(dp / eps))
    return s * dp / r_f + (1 - s) * g_b * dp


def _flows(V, t, p, T, ts, tr):
    lv, rv, ao, sv, pa, pv = V
    e = _act(t, T, ts, tr)
    P = dict(
        lv=_vent_p(lv, e, p['ees_lv'], p['v0_lv'], p['a_lv'], p['b_lv']),
        rv=_vent_p(rv, e, p['ees_rv'], p['v0_rv'], p['a_rv'], p['b_rv']),
        ao=(ao - p['v0_ao']) / p['c_ao'], sv=(sv - p['v0_sv']) / p['c_sv'],
        pa=(pa - p['v0_pa']) / p['c_pa'], pv=(pv - p['v0_pv']) / p['c_pv'])
    rf = p['r_valve']
    Q = dict(
        mi=_valve(P['pv'] - P['lv'], rf, p['g_mr']),
        ao=_valve(P['lv'] - P['ao'], rf, p['g_ar']),
        tr=_valve(P['sv'] - P['rv'], rf, p['g_tr']),
        pu=_valve(P['rv'] - P['pa'], rf, p['g_pr']),
        sys=(P['ao'] - P['sv']) / p['r_sys'],
        pul=(P['pa'] - P['pv']) / p['r_pul'])
    return P, Q, e


def _initial_volumes(p):
    bw = p['bw']
    vtot = 85.0 * bw
    lv = p['v0_lv'] + math.log(1 + 8.0 / p['a_lv']) / p['b_lv']
    rv = p['v0_rv'] + math.log(1 + 4.0 / p['a_rv']) / p['b_rv']
    ao = p['v0_ao'] + p['c_ao'] * 100.0
    sv = p['v0_sv'] + p['c_sv'] * 4.0 + (p['bvf'] - 1.0) * vtot + p['vol_shift']
    pa = p['v0_pa'] + p['c_pa'] * 14.0
    pv = p['v0_pv'] + p['c_pv'] * 6.0
    return np.array([lv, rv, ao, sv, pa, pv])


def simulate_hemo(p, n_beats=11, dt=0.002):
    """Simule la circulation ; renvoie séries temporelles du dernier cycle et indices."""
    T = 60.0 / p['hr']
    ts = 0.37 * math.sqrt(T)
    tr = 0.4 * ts
    V0 = _initial_volumes(p)

    def rhs(t, V):
        _, Q, _ = _flows(V, t, p, T, ts, tr)
        return [Q['mi'] - Q['ao'], Q['tr'] - Q['pu'], Q['ao'] - Q['sys'],
                Q['sys'] - Q['tr'], Q['pu'] - Q['pul'], Q['pul'] - Q['mi']]

    t_end = n_beats * T
    t_eval = np.arange(t_end - 2 * T, t_end, dt)
    sol = solve_ivp(rhs, (0, t_end), V0, method='LSODA', t_eval=t_eval,
                    rtol=1e-6, atol=1e-6, max_step=0.004)
    V = sol.y
    t = sol.t
    P, Q, e = _flows(V, t, p, T, ts, tr)
    nb = int(round(T / dt))
    last = slice(len(t) - nb, len(t))
    prev = slice(len(t) - 2 * nb, len(t) - nb)
    out = dict(t=t[last] - t[last][0], T=T, ts=ts, tr=tr)
    for k, v in P.items():
        out['P_' + k] = v[last]
    for k, v in Q.items():
        out['Q_' + k] = v[last]
    out['V_lv'] = V[0][last]
    edv, esv = V[0][last].max(), V[0][last].min()
    sv_tot = edv - esv
    sv_prev = V[0][prev].max() - V[0][prev].min()
    qmi, qao = Q['mi'][last], Q['ao'][last]
    dtt = dt
    v_reg = (-np.minimum(qmi, 0).sum() - np.minimum(qao, 0).sum()) * dtt
    out.update(
        map=P['ao'][last].mean(), sap=P['ao'][last].max(), dap=P['ao'][last].min(),
        co=Q['sys'][last].mean() * 60 / 1000.0,       # L/min
        edv=edv, esv=esv, ef=sv_tot / edv, sv=sv_tot,
        reg_fraction=float(v_reg / sv_tot) if sv_tot > 0 else 0.0,
        la_mean=P['pv'][last].mean(), la_max=P['pv'][last].max(),
        pap_mean=P['pa'][last].mean(), cvp=P['sv'][last].mean(),
        lv_edp=P['lv'][last][0], converged=abs(sv_tot - sv_prev) / max(sv_tot, 1e-6) < 0.03)
    return out


# ------------------------------------------------------------------ gaz / acide-base
def hb_o2_sat(po2, p50, n):
    po2 = max(po2, 1e-3)
    return po2 ** n / (po2 ** n + p50 ** n)


def o2_content(po2, hb, p50, n):          # mL O2 / dL
    return 1.34 * hb * hb_o2_sat(po2, p50, n) + 0.003 * po2


def po2_from_content(c, hb, p50, n):
    f = lambda x: o2_content(x, hb, p50, n) - c
    return brentq(f, 1.0, 600.0) if f(1.0) < 0 < f(600.0) else (1.0 if c < 0 else 600.0)


def stewart_ph(p, pco2, lactate, na=None, k=None, cl=None):
    na = p['na'] if na is None else na
    k = p['k'] if k is None else k
    cl = p['cl'] if cl is None else cl
    sid = na + k - cl - lactate - p['xa']
    alb_gl = p['alb'] * 10.0

    def f(ph):
        h = 1e3 * 10 ** (-ph)
        hco3 = 0.0307 * pco2 * 10 ** (ph - 6.1)
        # albumine canine : Constable & Stämpfli 2005 (Atot 0.469 mmol/g, pKa 7.77, charge nette 0.42 mEq/g d'albumine à pH 7,4 ; xa<0 = Ca2+, Mg2+ non comptés dans le SID)
        a_alb = alb_gl * (0.28 + 0.469 / (1.0 + 10 ** (7.77 - ph)))
        a_pi = p['phos'] * (0.309 * ph - 0.469)   # phosphate : coefficient humain (pas d'équivalent canin publié trouvé)
        return sid + h - hco3 - a_alb - a_pi
    ph = brentq(f, 5.5, 10.5)
    hco3 = 0.0307 * pco2 * 10 ** (ph - 6.1)
    return ph, hco3


def base_excess(ph, hco3, hb):
    hbm = hb * 0.6206
    return (1 - 0.014 * hbm) * (hco3 - 24.4 + (2.3 * hbm + 7.7) * (ph - 7.4))


def gas_exchange(p, hemo, lactate_fn):
    """Résout PaCO2 (contrôle ventilatoire), PaO2, gaz artériel et veineux."""
    bw = p['bw']
    vco2 = p['vco2'] * bw
    va0 = p['va0'] * bw
    paO2 = 90.0
    for _ in range(3):
        def resid(pco2):
            ph, _ = stewart_ph(p, pco2, p['_lact'])
            va = va0 * p['vent_drive'] * max(0.12, 1 + 0.08 * (pco2 - 37.0)) \
                * math.exp(-2.5 * (ph - 7.4)) * (1 + 0.03 * max(0.0, 70.0 - paO2))
            return pco2 - 0.863 * vco2 / max(va, 1e-3)
        pco2 = brentq(resid, 5.0, 250.0)
        ph, hco3 = stewart_ph(p, pco2, p['_lact'])
        va = 0.863 * vco2 / pco2
        pAO2 = p['fio2'] * (p['pb'] - 47.0) - pco2 / p['rq']
        aa = p['aagrad'] + p['edema_k'] * max(0.0, hemo['la_mean'] - 18.0)
        paO2 = max(pAO2 - aa, 15.0)
    p50 = p['p50'] * 10 ** (0.37 * (7.4 - ph))   # Bohr canin : dlogP50/dpH = -0.370 (Cambier 2004)
    sao2 = hb_o2_sat(paO2, p50, p['hill_n'])
    cao2 = o2_content(paO2, p['hb'], p50, p['hill_n'])
    return dict(pco2=pco2, ph=ph, hco3=hco3, be=base_excess(ph, hco3, p['hb']),
                pao2=paO2, sao2=100 * sao2, cao2=cao2, va=va, p50=p50,
                rr=20.0 * (va / va0) ** 0.5, aa=aa)


# ------------------------------------------------------------------ orchestration
S_REF, V_REF = 0.2, 0.6      # tonus de référence (chien sain au repos) : définit 'hr' = FC de repos


def hr_factor(s, v, p):
    """Effet multiplicatif sympathique (accélérateur) et vagal (frein) sur la FC du nœud sinusal."""
    return (1 + p['a_s'] * s) * (1 - p['a_v'] * v)


def _evaluate_once(p, n_beats=11):
    """Une passe : baroréflexe, hémodynamique, aval métabolique (paramètres déjà couplés)."""
    base = dict(p)
    d = 0.0                                   # commande barorécepteur (>0 : hypotension)
    for it in range(5):
        s_tot = float(np.clip(base['s0'] + 0.8 * d, 0.0, 1.0))
        v_tot = float(np.clip(base['v0'] + base['vagal_add'] - 0.6 * d, 0.0, 1.0))
        s_eff = s_tot * (1 - base['beta_block'])          # voie chronotrope/inotrope bloquable
        v_eff = v_tot * (1 - base['vagal_block'])         # voie vagale bloquable (atropine)
        kfac = max(0.4, 1 - 0.30 * max(0.0, base['k'] - 6.0))
        q = dict(base)
        q['hr'] = base['hr'] * hr_factor(s_eff, v_eff, base) / hr_factor(S_REF, V_REF, base) * kfac
        q['ees_lv'] = base['ees_lv'] * (1 + 0.4 * (s_eff - base['s0']))
        q['ees_rv'] = base['ees_rv'] * (1 + 0.4 * (s_eff - base['s0']))
        q['r_sys'] = base['r_sys'] * (1 + 0.5 * (s_tot - base['s0']))
        q['vol_shift'] = base['vol_shift'] + 0.20 * 85.0 * base['bw'] * (s_tot - base['s0'])   # recrutement du volume non contraint
        h = simulate_hemo(q, n_beats)
        d_t = float(np.clip(base['baro_gain'] * (base['map_set'] - h['map']) / 45.0, -1, 1))
        if abs(d_t - d) < 0.04:
            break
        d = 0.5 * d + 0.5 * d_t
    p = q
    p['sympathetic'] = s_tot
    auto = dict(s=s_tot, v=v_tot, s_eff=s_eff, v_eff=v_eff, d=d, map=h['map'])
    bw = p['bw']
    # --- perfusion -> DO2, lactate
    gas0 = dict(p)
    p['_lact'] = p['lact0']
    g = gas_exchange(p, h, None)
    do2 = h['co'] * 1000.0 * g['cao2'] / 100.0          # mL/min
    vo2 = p['vo2'] * bw
    er = vo2 / do2
    lact = p['lact0'] + 9.0 / (1 + math.exp(-(er - 0.55) / 0.06))
    p['_lact'] = lact
    g = gas_exchange(p, h, None)
    do2 = h['co'] * 1000.0 * g['cao2'] / 100.0
    # --- sang veineux mixte
    cv = g['cao2'] - vo2 / (h['co'] * 10.0)               # mL/dL
    pvo2 = po2_from_content(cv, p['hb'], g['p50'], p['hill_n'])
    dpco2 = 5.5 * (125.0 * bw / 1000.0) / max(h['co'], 0.2)
    pvco2 = g['pco2'] + dpco2
    phv, hco3v = stewart_ph(p, pvco2, lact)
    # --- rein
    co_ref = 3.29 * (bw / 20.0) ** 0.75
    perf = float(min(np.clip((h['map'] - 40.0) / 35.0, 0, 1), np.clip((h['co'] / co_ref - 0.5) / 0.5, 0, 1)))
    perf = max(perf, 0.25)        # plateau de l'azotémie prérénale (au-delà : IRA, non modélisée)
    gfr_ratio = p['nephron_mass'] * perf
    gfr_ratio = max(gfr_ratio, 0.03)
    chem = dict(
        na=p['na'], k=p['k'], cl=p['cl'], lactate=lact, glucose=p['glucose'],
        creat=p['creat0'] / gfr_ratio, urea=p['urea0'] / gfr_ratio ** 0.8,
        phos=p['phos'], alb=p['alb'] * 10.0, alt=p['alt'], hct=p['hb'] * 3.0,
        gfr=p['gfr0'] * bw * gfr_ratio, anion_gap=p['na'] + p['k'] - p['cl'] - g['hco3'])
    res = dict(hemo=h, art=dict(g, lactate=lact),
               ven=dict(ph=phv, pco2=pvco2, hco3=hco3v, po2=pvo2),
               chem=chem, do2=do2, do2_kg=do2 / bw, er=er, sympathetic=s_tot, autonomic=auto, p=p)
    return res


# ------------------------------------------------------------------ décompensation et viabilité
# Couplages mécanistiques en retour de l'aval métabolique sur la pompe et les vaisseaux.
# Constantes de seuil et de pente : paramètres du patient (acid_*, hypox_*, k_weak*, *_arrest),
# PLACEHOLDERS evidence D. Inactifs dans la plage du golden (pH ≥ 7,26 ; PvO2 ≥ 26 ; K ≥ 3,7 ; ER < 0,25).
DECOMP_MAX_ITER = 30      # passes au plus avant de conclure à une spirale non résolue
DECOMP_TOL = 1e-3         # convergence des facteurs de couplage
F_FLOOR = 0.1             # plancher des facteurs (au-delà : effondrement par les critères d'arrêt)
ER_ANAEROBIC = 0.55       # extraction d'O2 au point d'inflexion du lactate (voir _evaluate_once)
LA_EDEMA = 18.0           # POG au-delà de laquelle le gradient A-a s'ouvre (gas_exchange)
MAP_RENAL = 75.0          # PAM sous laquelle la perfusion rénale baisse (40 + 35, _evaluate_once)


class NonViable(Exception):
    """Aucun état stationnaire viable : décès du patient virtuel (cause mécanistique)."""

    def __init__(self, cause, last=None):
        super().__init__(cause)
        self.cause = cause
        self.last = last          # dernière passe viable (ou None)


def coupling_targets(p, res):
    """Facteurs cibles (contractilité, résistance) imposés par l'état métabolique d'une passe."""
    ph, pvo2 = res['art']['ph'], res['ven']['po2']
    f_acid_c = max(F_FLOOR, 1.0 - p['acid_ees'] * max(0.0, p['acid_ph0'] - ph))
    f_acid_r = max(F_FLOOR, 1.0 - p['acid_rsys'] * max(0.0, p['acid_ph0'] - ph))
    f_hyp = max(F_FLOOR, 1.0 - p['hypox_ees'] * max(0.0, p['hypox_pvo2'] - pvo2))
    cpp = res['hemo']['dap'] - res['hemo']['lv_edp']
    f_cor = max(F_FLOOR, 1.0 - p['cpp_ees'] * max(0.0, p['cpp_min'] - cpp))
    return f_acid_c * f_hyp * f_cor, f_acid_r, f_acid_c, f_hyp, f_cor


def evaluate(p, n_beats=11):
    """Évalue le patient avec les couplages de décompensation (point fixe amorti).

    Hypokaliémie -> faiblesse ventilatoire (direct) ; acidémie et hypoxie tissulaire -> dépression
    de la contractilité, acidémie -> vasoplégie (boucle). Lève NonViable si la pompe s'arrête
    (FC < hr_arrest), si la circulation s'effondre (DC < co_arrest_frac × DC de référence), si aucun
    état stationnaire n'existe (racine introuvable) ou si la spirale ne converge pas."""
    p0 = dict(p)
    bw = p0['bw']
    co_ref = 3.29 * (bw / 20.0) ** 0.75
    f_vent = max(F_FLOOR, 1.0 - p0['k_weak_vent'] * max(0.0, p0['k_weak'] - p0['k']))
    fc, fr = 1.0, 1.0
    last = None
    converged = False
    it = 0
    for it in range(DECOMP_MAX_ITER):
        q = dict(p0)
        q['vent_drive'] = p0['vent_drive'] * f_vent
        q['ees_lv'] = p0['ees_lv'] * fc
        q['ees_rv'] = p0['ees_rv'] * fc
        q['r_sys'] = p0['r_sys'] * fr
        try:
            res = _evaluate_once(q, n_beats)
        except (ValueError, ZeroDivisionError):
            raise NonViable('no_steady_state', last)
        if res['p']['hr'] < p0['hr_arrest']:
            raise NonViable('cardiac_arrest', res)
        if res['hemo']['co'] < p0['co_arrest_frac'] * co_ref:
            raise NonViable('circulatory_collapse', res)
        if res['er'] >= 1.0:
            raise NonViable('oxygen_debt', res)       # VO2 > DO2 : aucun état aérobie possible
        last = res
        tc, tr, f_acid, f_hyp, f_cor = coupling_targets(p0, res)
        if abs(tc - fc) < DECOMP_TOL and abs(tr - fr) < DECOMP_TOL:
            converged = True
            break
        fc = 0.5 * fc + 0.5 * tc
        fr = 0.5 * fr + 0.5 * tr
    if not converged:
        raise NonViable('vicious_circle', last)
    kfac = max(0.4, 1 - 0.30 * max(0.0, p0['k'] - 6.0))
    mech = []
    if f_acid < 1.0:
        mech.append(dict(id='acidemia_depression', value=res['art']['ph'], threshold=p0['acid_ph0'], factor=f_acid))
    if f_hyp < 1.0:
        mech.append(dict(id='hypoxic_depression', value=res['ven']['po2'], threshold=p0['hypox_pvo2'], factor=f_hyp))
    if f_cor < 1.0:
        mech.append(dict(id='coronary_ischemia', value=res['hemo']['dap'] - res['hemo']['lv_edp'], threshold=p0['cpp_min'], factor=f_cor))
    if f_vent < 1.0:
        mech.append(dict(id='hypokalemic_weakness', value=p0['k'], threshold=p0['k_weak'], factor=f_vent))
    if kfac < 1.0:
        mech.append(dict(id='hyperkalemic_bradycardia', value=p0['k'], threshold=6.0, factor=kfac))
    if res['er'] > ER_ANAEROBIC:
        mech.append(dict(id='anaerobic_metabolism', value=res['er'], threshold=ER_ANAEROBIC, factor=1.0))
    if res['hemo']['la_mean'] > LA_EDEMA:
        mech.append(dict(id='pulmonary_edema', value=res['hemo']['la_mean'], threshold=LA_EDEMA, factor=1.0))
    if res['hemo']['map'] < MAP_RENAL:
        mech.append(dict(id='renal_hypoperfusion', value=res['hemo']['map'], threshold=MAP_RENAL, factor=1.0))
    if res['autonomic']['s'] >= 1.0:
        mech.append(dict(id='baroreflex_exhausted', value=res['autonomic']['s'], threshold=1.0, factor=1.0))
    res['decomp'] = dict(f_contract=fc, f_rsys=fr, f_vent=f_vent, iterations=it + 1)
    res['outcome'] = dict(state='decompensated' if mech else 'stable', mechanisms=mech)
    return res
