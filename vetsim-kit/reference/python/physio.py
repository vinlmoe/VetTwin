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


def _atrial_act(t, T, p, pr=None):
    """Activation atriale : débute PR avant l'activation ventriculaire du même battement
    (PR de la conduction si fourni, sinon le paramètre `pr`)."""
    return _act(t + (p['pr'] if pr is None else pr), T, p['ta_atr'], p['tra_atr'])


def _flows(V, t, p, T, ts, tr, pr=None):
    lv, rv, ao, sv, pa, pv = V
    e = _act(t, T, ts, tr)
    ea = _atrial_act(t, T, p, pr)
    # oreillettes contractiles : élastance active proportionnelle à la pression passive
    # (pv = OG + veines pulmonaires, sv = OD + veines systémiques) ; k = 0 : oreillettes passives
    P = dict(
        lv=_vent_p(lv, e, p['ees_lv'], p['v0_lv'], p['a_lv'], p['b_lv']),
        rv=_vent_p(rv, e, p['ees_rv'], p['v0_rv'], p['a_rv'], p['b_rv']),
        ao=(ao - p['v0_ao']) / p['c_ao'], sv=(1 + p['k_ra'] * ea) * (sv - p['v0_sv']) / p['c_sv'],
        pa=(pa - p['v0_pa']) / p['c_pa'], pv=(1 + p['k_la'] * ea) * (pv - p['v0_pv']) / p['c_pv'])
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


def simulate_hemo(p, n_beats=11, dt=0.002, pr=None):
    """Simule la circulation (rythme régulier) ; renvoie séries temporelles du dernier cycle et
    indices. `pr` : délai oreillette → ventricule de la conduction (défaut : paramètre `pr`)."""
    T = 60.0 / p['hr']
    ts = 0.37 * math.sqrt(T)
    tr = 0.4 * ts
    V0 = _initial_volumes(p)

    def rhs(t, V):
        _, Q, _ = _flows(V, t, p, T, ts, tr, pr)
        return [Q['mi'] - Q['ao'], Q['tr'] - Q['pu'], Q['ao'] - Q['sys'],
                Q['sys'] - Q['tr'], Q['pu'] - Q['pul'], Q['pul'] - Q['mi']]

    t_end = n_beats * T
    t_eval = np.arange(t_end - 2 * T, t_end, dt)
    sol = solve_ivp(rhs, (0, t_end), V0, method='LSODA', t_eval=t_eval,
                    rtol=1e-6, atol=1e-6, max_step=0.004)
    V = sol.y
    t = sol.t
    P, Q, e = _flows(V, t, p, T, ts, tr, pr)
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
    # flux mitral : pic protodiastolique E (hors activation atriale) et pic télédiastolique A (pendant)
    ea = _atrial_act(t[last], T, p, pr)
    qf = np.maximum(qmi, 0.0)
    out['mitral_e'] = float(qf[ea <= 0.0].max()) if np.any(ea <= 0.0) else 0.0
    out['mitral_a'] = float(qf[ea > 0.0].max()) if np.any(ea > 0.0) else 0.0
    out['hr_v'] = 60.0 / T
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


# Rythme non trivial : séquence de conduction simulée sur RHYTHM_T_END secondes, indicateurs
# moyennés après RHYTHM_WARM secondes ; aléa (FA, ectopie, blocs) : SplitMix64 à graine fixe,
# identique au bit près en Rust (rhythm::SplitMix64).
RHYTHM_SEED = 0
RHYTHM_T_END = 30.0
RHYTHM_WARM = 10.0


def _hemo_with_rhythm(q, v_eff, n_beats):
    """Hémodynamique d'une passe : périodique si la conduction est sinusale 1:1 à PR constant
    (PR de la conduction), sinon battement par battement sur la séquence de conduction."""
    import rhythm
    T_s = 60.0 / q['hr']
    if rhythm.is_trivial(q, v_eff, T_s):
        return simulate_hemo(q, n_beats, pr=rhythm.steady_pr(q, v_eff, T_s)), None
    rng = rhythm.SplitMix64(RHYTHM_SEED)
    cond = rhythm.simulate_conduction(q, v_eff, T_s, RHYTHM_T_END, rng.random)
    if sum(1 for b in cond['ventricular'] if b['t'] >= RHYTHM_WARM) < 3:
        raise NonViable('cardiac_arrest')        # asystolie : aucune activation ventriculaire
    return simulate_hemo_beats(q, cond, RHYTHM_T_END, RHYTHM_WARM, T_s), cond


def _evaluate_once(p, n_beats=11):
    """Une passe : baroréflexe, hémodynamique, aval métabolique (paramètres déjà couplés)."""
    base = dict(p)
    d = 0.0                                   # commande barorécepteur (>0 : hypotension)
    for it in range(5):
        s_tot = float(np.clip(base['s0'] + 0.8 * d, 0.0, 1.0))
        v_tot = float(np.clip(base['v0'] + base['vagal_add'] - 0.6 * d, 0.0, 1.0))
        s_eff = s_tot * (1 - base['beta_block'])          # voie chronotrope/inotrope bloquable
        v_eff = v_tot * (1 - base['vagal_block'])         # voie vagale bloquable (atropine)
        q = dict(base)
        # FC sinusale (nœud sinusal) ; K⁺ agit par la conduction (rhythm), plus par un frein ici
        q['hr'] = base['hr'] * hr_factor(s_eff, v_eff, base) / hr_factor(S_REF, V_REF, base)
        q['ees_lv'] = base['ees_lv'] * (1 + 0.4 * (s_eff - base['s0']))
        q['ees_rv'] = base['ees_rv'] * (1 + 0.4 * (s_eff - base['s0']))
        q['r_sys'] = base['r_sys'] * (1 + 0.5 * (s_tot - base['s0']))
        q['vol_shift'] = base['vol_shift'] + 0.20 * 85.0 * base['bw'] * (s_tot - base['s0'])   # recrutement du volume non contraint
        h, cond = _hemo_with_rhythm(q, v_eff, n_beats)
        d_t = float(np.clip(base['baro_gain'] * (base['map_set'] - h['map']) / 45.0, -1, 1))
        if abs(d_t - d) < 0.04:
            break
        d = 0.5 * d + 0.5 * d_t
    p = q
    p['hr_sinus'] = q['hr']
    p['hr'] = h['hr_v']                       # FC rapportée : FC ventriculaire
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
    mech = []
    if f_acid < 1.0:
        mech.append(dict(id='acidemia_depression', value=res['art']['ph'], threshold=p0['acid_ph0'], factor=f_acid))
    if f_hyp < 1.0:
        mech.append(dict(id='hypoxic_depression', value=res['ven']['po2'], threshold=p0['hypox_pvo2'], factor=f_hyp))
    if f_cor < 1.0:
        mech.append(dict(id='coronary_ischemia', value=res['hemo']['dap'] - res['hemo']['lv_edp'], threshold=p0['cpp_min'], factor=f_cor))
    if f_vent < 1.0:
        mech.append(dict(id='hypokalemic_weakness', value=p0['k'], threshold=p0['k_weak'], factor=f_vent))
    if p0['k'] >= p0['k_atrial_standstill']:
        mech.append(dict(id='atrial_standstill', value=p0['k'], threshold=p0['k_atrial_standstill'], factor=1.0))
    if res['p']['hr'] < 0.98 * res['p']['hr_sinus']:
        mech.append(dict(id='conduction_block', value=res['p']['hr'], threshold=res['p']['hr_sinus'], factor=res['p']['hr'] / res['p']['hr_sinus']))
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


# ------------------------------------------------------------------ hémodynamique battement par battement
def _act1(tau, ts, tr):
    """Activation d'un battement isolé, `tau` secondes après son début (même forme que _act)."""
    return np.where((tau >= 0) & (tau < ts), 0.5 * (1 - np.cos(np.pi * tau / ts)),
                    np.where((tau >= ts) & (tau < ts + tr), 0.5 * (1 + np.cos(np.pi * (tau - ts) / tr)), 0.0))


def beat_schedule(cond, period0):
    """Battements ventriculaires (t, ts, tr) et activations atriales organisées (t) d'une séquence
    de conduction (rhythm.simulate_conduction). Durée de systole du battement i : 0,37·√RR_i,
    RR_i = intervalle qui le précède (période nominale pour le premier)."""
    tv = [b['t'] for b in cond['ventricular']]
    beats = []
    for i, t in enumerate(tv):
        rr = t - tv[i - 1] if i > 0 else period0
        ts = 0.37 * math.sqrt(rr)
        beats.append((t, ts, 0.4 * ts))
    atr = [t for t, organized in cond['atrial'] if organized]
    return beats, atr


def _activations(t, beats, atr, p):
    """Activation ventriculaire (max des deux derniers battements, pour qu'un battement prématuré
    survienne pendant la relaxation du précédent) et activation atriale à l'instant t."""
    import bisect
    tv = beat_starts = [b[0] for b in beats]
    i = bisect.bisect_right(beat_starts, t) - 1
    e = 0.0
    for j in (i - 1, i):
        if 0 <= j < len(beats):
            t0, ts, tr = beats[j]
            e = max(e, float(_act1(t - t0, ts, tr)))
    k = bisect.bisect_right(atr, t) - 1
    ea = 0.0
    for j in (k - 1, k):
        if 0 <= j < len(atr):
            ea = max(ea, float(_act1(t - atr[j], p['ta_atr'], p['tra_atr'])))
    return e, ea


def _flows_beats(V, t, p, beats, atr):
    lv, rv, ao, sv, pa, pv = V
    e, ea = _activations(t, beats, atr, p)
    P = dict(
        lv=_vent_p(lv, e, p['ees_lv'], p['v0_lv'], p['a_lv'], p['b_lv']),
        rv=_vent_p(rv, e, p['ees_rv'], p['v0_rv'], p['a_rv'], p['b_rv']),
        ao=(ao - p['v0_ao']) / p['c_ao'], sv=(1 + p['k_ra'] * ea) * (sv - p['v0_sv']) / p['c_sv'],
        pa=(pa - p['v0_pa']) / p['c_pa'], pv=(1 + p['k_la'] * ea) * (pv - p['v0_pv']) / p['c_pv'])
    rf = p['r_valve']
    Q = dict(
        mi=_valve(P['pv'] - P['lv'], rf, p['g_mr']), ao=_valve(P['lv'] - P['ao'], rf, p['g_ar']),
        tr=_valve(P['sv'] - P['rv'], rf, p['g_tr']), pu=_valve(P['rv'] - P['pa'], rf, p['g_pr']),
        sys=(P['ao'] - P['sv']) / p['r_sys'], pul=(P['pa'] - P['pv']) / p['r_pul'])
    return P, Q, e, ea


def simulate_hemo_beats(p, cond, t_end, warm, period0, dt=0.002):
    """Simule la circulation sur une séquence de battements quelconque (rythme irrégulier, blocs,
    extrasystoles, FA). Indicateurs moyennés sur les battements complets après `warm` secondes ;
    chaque battement garde son VES, sa PAS et sa PAD (déficit de pouls, alternance)."""
    beats, atr = beat_schedule(cond, period0)
    V0 = _initial_volumes(p)

    def rhs(t, V):
        _, Q, _, _ = _flows_beats(V, t, p, beats, atr)
        return [Q['mi'] - Q['ao'], Q['tr'] - Q['pu'], Q['ao'] - Q['sys'],
                Q['sys'] - Q['tr'], Q['pu'] - Q['pul'], Q['pul'] - Q['mi']]

    t_eval = np.arange(warm, t_end, dt)
    sol = solve_ivp(rhs, (0, t_end), V0, method='LSODA', t_eval=t_eval, rtol=1e-6, atol=1e-6, max_step=0.004)
    V, t = sol.y, sol.t
    rows = [_flows_beats(V[:, j], t[j], p, beats, atr) for j in range(len(t))]
    P = {k: np.array([r[0][k] for r in rows]) for k in rows[0][0]}
    Q = {k: np.array([r[1][k] for r in rows]) for k in rows[0][1]}
    ea = np.array([r[3] for r in rows])
    # battements complets dans la fenêtre
    starts = [b[0] for b in beats if b[0] >= warm]
    # contraction précédente terminée au début du battement (sinon la « télédiastole » n'existe pas)
    relaxed = {b1[0]: (b1[0] - b0[0] >= b0[1] + b0[2]) for b0, b1 in zip(beats[:-1], beats[1:])}
    per_beat = []
    for a, b in zip(starts[:-1], starts[1:]):
        m = (t >= a) & (t < b)
        if m.sum() < 3:
            continue
        vl = V[0][m]
        qmi, qao = Q['mi'][m], Q['ao'][m]
        qf = np.maximum(qmi, 0.0)
        per_beat.append(dict(t=a, rr=b - a, relaxed=relaxed.get(a, True), edv=vl.max(), esv=vl.min(), sv=vl.max() - vl.min(),
                             sap=P['ao'][m].max(), dap=P['ao'][m].min(), lv_edp=P['lv'][m][0],
                             v_reg=(-np.minimum(qmi, 0).sum() - np.minimum(qao, 0).sum()) * dt,
                             mitral_e=float(qf[ea[m] <= 0].max()) if np.any(ea[m] <= 0) else 0.0,
                             mitral_a=float(qf[ea[m] > 0].max()) if np.any(ea[m] > 0) else 0.0))
    if not per_beat:
        raise ValueError('aucun battement complet dans la fenêtre')
    win = (t >= starts[0]) & (t < starts[-1])
    mean = lambda k: float(np.mean([x[k] for x in per_beat]))
    sv_tot = sum(x['sv'] for x in per_beat)
    rr_mean = mean('rr')
    out = dict(t=t, beats=per_beat, T=rr_mean,
               map=float(P['ao'][win].mean()), sap=mean('sap'), dap=mean('dap'),
               co=float(Q['sys'][win].mean()) * 60 / 1000.0,
               edv=mean('edv'), esv=mean('esv'), sv=mean('sv'), ef=mean('sv') / mean('edv'),
               reg_fraction=sum(x['v_reg'] for x in per_beat) / sv_tot if sv_tot > 0 else 0.0,
               la_mean=float(P['pv'][win].mean()), la_max=float(P['pv'][win].max()),
               pap_mean=float(P['pa'][win].mean()), cvp=float(P['sv'][win].mean()),
               lv_edp=float(np.mean([x['lv_edp'] for x in per_beat if x['relaxed']] or [math.nan])),
               mitral_e=mean('mitral_e'), mitral_a=mean('mitral_a'),
               hr_v=60.0 / rr_mean, converged=True)
    for k, v in P.items():
        out['P_' + k] = v
    for k, v in Q.items():
        out['Q_' + k] = v
    out['V_lv'] = V[0]
    return out
