"""Moteur physiologique : hémodynamique (boucle fermée à élastances variables),
oxygénation, acide-base (Stewart), fonction rénale, lactate."""
import math
import numpy as np
from scipy.integrate import solve_ivp
from scipy.optimize import brentq


# ------------------------------------------------------------------ cardio
def _act(t, T, ts, tr, g=1.0):
    """Activation périodique ; montée ½(1 − cos(π(t/ts)^g)) (g = act_gamma ; 1 : demi-cosinus)."""
    tc = np.mod(t, T)
    return np.where(tc < ts, 0.5 * (1 - np.cos(np.pi * np.power(np.clip(tc / ts, 0.0, None), g))),
                    np.where(tc < ts + tr, 0.5 * (1 + np.cos(np.pi * (tc - ts) / tr)), 0.0))


def _vent_p(V, e, ees, v0, a, b, e_act=None):
    """Pression ventriculaire : élastance active e_act·Ees + raideur passive pondérée par (1 − e).
    `e_act` (défaut : e) porte l'amplitude propre du battement (battement par battement)."""
    x = np.clip(b * (V - v0), -20, 8)
    e_act = e if e_act is None else e_act
    return e_act * ees * (V - v0) + (1 - e) * a * (np.exp(x) - 1.0)


def _valve(dp, r_f, area, eps=0.3):
    s = 0.5 * (1 + np.tanh(dp / eps))
    return s * dp / r_f + _leak(dp, area, eps)


AO_CLOSE_TAU = 0.002   # miroir de hemo::AO_CLOSE_TAU (s)
AO_CLOSE_Q = 0.5       # miroir de hemo::AO_CLOSE_Q (mL/s)


def _aortic_forward(dp, qa, r_f, l_ao):
    """Éjection semi-lunaire inertielle L·dQ/dt = Δp − r·Q (miroir de hemo::aortic_forward), aortique
    ou pulmonaire ; L = 0 : débit algébrique σ·Δp/r."""
    s2 = 0.5 * (1 + np.tanh(dp / 0.3))
    if l_ao <= 0.0:
        return s2 * dp / r_f, -qa / AO_CLOSE_TAU
    s1 = 0.5 * (1 + np.tanh(qa / AO_CLOSE_Q))
    w = 1 - (1 - s1) * (1 - s2)
    return qa, w * (dp - r_f * qa) / l_ao - (1 - w) * qa / AO_CLOSE_TAU


ORIFICE_K = 50.0        # miroir de hemo::ORIFICE_K : Q (mL/s) = 50·A (cm²)·√Δp (mmHg)
ORIFICE_DELTA = 0.5     # miroir de hemo::ORIFICE_DELTA (mmHg)


def _leak(dp, area, eps=0.3):
    """Fuite par la loi d'orifice (miroir de hemo::orifice_leak)."""
    s = 0.5 * (1 + np.tanh(dp / eps))
    return (1 - s) * ORIFICE_K * area * dp / np.sqrt(np.sqrt(dp * dp + ORIFICE_DELTA ** 2))


def _valve_flows(P, qa, qp, p):
    """Débits pour des pressions données (P['ao'] et P['pa'] proximales = compliance + z·q) et les
    débits inertiels qa et qp (miroir de hemo::valve_flows)."""
    rf = p['r_valve']
    gs = (p['bw'] / 20.0) ** 0.75   # orifices normalisés à 20 kg, à l'échelle des débits (miroir de hemo::leak_scale)
    dp_ao = P['lv'] - P['ao']
    qf, dqa = _aortic_forward(dp_ao, qa, rf, p['l_ao'])
    p_c = P['ao'] - p['z_ao'] * qa
    p_cp = P['pa'] - p['z_pa'] * qp
    dp_pu = P['rv'] - P['pa']
    qpf, dqp = _aortic_forward(dp_pu, qp, rf, p['l_pa'])
    Q = dict(
        mi=_valve(P['pv'] - P['lv'], rf, p['a_mr'] * gs),
        ao=qf + _leak(dp_ao, p['a_ar'] * gs),
        tr=_valve(P['sv'] - P['rv'], rf, p['a_tr'] * gs),
        pu=qpf + _leak(dp_pu, p['a_pr'] * gs),
        sys=(p_c - P['sv']) / (p['r_sys'] - p['z_ao']),
        pul=(p_cp - P['pv']) / (p['r_pul'] - p['z_pa']))
    return Q, dqa, dqp


def _atrial_act(t, T, p, pr=None):
    """Activation atriale : débute PR avant l'activation ventriculaire du même battement
    (PR de la conduction si fourni, sinon le paramètre `pr`)."""
    # origine des temps : début mécanique ventriculaire (emd_v − QRS/2 après le pic R) ; la
    # contraction atriale débute emd_a après le début de l'onde P, elle-même PR avant le pic R
    lead = (p['pr'] if pr is None else pr) + mech_offset(p, False) - p['emd_a']
    return _act(t + lead, T, p['ta_atr'], p['tra_atr'])


def mech_offset(p, wide):
    """Début mécanique d'un battement ventriculaire par rapport à son pic R (s) : début du QRS
    tracé par l'ECG (bornes ± 2 σ, comme signals::ecg_windows du moteur) + délai électromécanique emd_v."""
    from signals import WIDE_QRS, WIDE_R_SIGMA, Q_POS, Q_SIGMA
    K = 2.0                                  # bornes des ondes : ± 2 écarts-types (signals::ecg_windows)
    lead = K * WIDE_R_SIGMA * p['qrs'] * WIDE_QRS if wide else (Q_POS + K * Q_SIGMA) * p['qrs']
    return p['emd_v'] - lead


def _flows(V, t, p, T, ts, tr, pr=None):
    lv, rv, ao, sv, pa, pv, qa, qp = V
    e = _act(t, T, ts, tr, p['act_gamma'])
    ea = _atrial_act(t, T, p, pr)
    # oreillettes contractiles : élastance active proportionnelle à la pression passive
    # (pv = OG + veines pulmonaires, sv = OD + veines systémiques) ; k = 0 : oreillettes passives
    P = dict(
        lv=_vent_p(lv, e, p['ees_lv'], p['v0_lv'], p['a_lv'], p['b_lv']),
        rv=_vent_p(rv, e, p['ees_rv'], p['v0_rv'], p['a_rv'], p['b_rv']),
        ao=(ao - p['v0_ao']) / p['c_ao'] + p['z_ao'] * qa, sv=(1 + p['k_ra'] * ea) * (sv - p['v0_sv']) / (p['c_sv'] * p['ven_cap']),
        pa=(pa - p['v0_pa']) / p['c_pa'] + p['z_pa'] * qp, pv=(1 + p['k_la'] * ea) * (pv - p['v0_pv']) / p['c_pv'])
    Q, dqa, dqp = _valve_flows(P, qa, qp, p)
    return P, Q, e, (dqa, dqp)


def _initial_volumes(p):
    bw = p['bw']
    vtot = 85.0 * bw
    lv = p['v0_lv'] + math.log(1 + 8.0 / p['a_lv']) / p['b_lv']
    rv = p['v0_rv'] + math.log(1 + 4.0 / p['a_rv']) / p['b_rv']
    ao = p['v0_ao'] + p['c_ao'] * 100.0
    sv = p['v0_sv'] + p['c_sv'] * 4.0 + (p['bvf'] - 1.0) * vtot + p['vol_shift']
    pa = p['v0_pa'] + p['c_pa'] * 14.0
    pv = p['v0_pv'] + p['c_pv'] * 6.0
    return np.array([lv, rv, ao, sv, pa, pv, 0.0, 0.0])


def _edp_at(t, plv, tb):
    """PTDVG au début mécanique exact tb, interpolée entre les deux échantillons qui l'encadrent
    (miroir de hemo::simulate_hemo_pr)."""
    n = len(t)
    k = min(max(int(np.searchsorted(t, tb, side='right')) - 1, 0), n - 2)
    a, b = float(plv[k]), float(plv[k + 1])
    return a + (b - a) * (tb - float(t[k])) / (float(t[k + 1]) - float(t[k]))


def _edp_beat(t, plv, m, a):
    """PTDVG au début mécanique a d'un battement (interpolation avec l'échantillon qui précède)."""
    j = int(np.argmax(m))
    if j > 0:
        i = j - 1
        return float(plv[i]) + (float(plv[j]) - float(plv[i])) * (a - float(t[i])) / (float(t[j]) - float(t[i]))
    return float(plv[j])


# Miroir de hemo::N_BEATS (22), hemo::MAP_DRIFT_TOL et hemo::MAX_BEATS (Rust normatif) : si la PAM dérive de plus de
# 0,5 % entre les deux derniers battements, le régime n'est pas périodique et la simulation est
# refaite avec deux fois plus de battements (hypovolémie : PAM 70 au lieu de 50 mmHg à 11 battements).
MAP_DRIFT_TOL = 5e-3
MAX_BEATS = 176


def simulate_hemo(p, n_beats=22, dt=0.002, pr=None):
    """Simule la circulation (rythme régulier) ; renvoie séries temporelles du dernier cycle et
    indices. `pr` : délai oreillette → ventricule de la conduction (défaut : paramètre `pr`).
    Battements doublés tant que le dernier n'est pas périodique (voir MAP_DRIFT_TOL)."""
    nb = n_beats
    while True:
        out = _simulate_hemo_once(p, nb, dt, pr)
        if out['map_drift'] <= MAP_DRIFT_TOL or 2 * nb > MAX_BEATS:
            return out
        nb *= 2


def _simulate_hemo_once(p, n_beats, dt, pr):
    T = 60.0 / p['hr']
    ts = p['ts_coef'] * math.sqrt(T)        # durée de systole (ts_coef calé sur B1→B2 du beagle)
    tr = p['tr_frac'] * ts
    V0 = _initial_volumes(p)

    def rhs(t, V):
        _, Q, _, (dqa, dqp) = _flows(V, t, p, T, ts, tr, pr)
        return [Q['mi'] - Q['ao'], Q['tr'] - Q['pu'], Q['ao'] - Q['sys'],
                Q['sys'] - Q['tr'], Q['pu'] - Q['pul'], Q['pul'] - Q['mi'], dqa, dqp]

    t_end = n_beats * T
    t_eval = np.arange(t_end - 2 * T, t_end, dt)
    sol = solve_ivp(rhs, (0, t_end), V0, method='LSODA', t_eval=t_eval,
                    rtol=1e-6, atol=1e-6, max_step=0.004)
    V = sol.y
    t = sol.t
    P, Q, e, _ = _flows(V, t, p, T, ts, tr, pr)
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
        lv_edp=_edp_at(t, P['lv'], t_end - T), converged=abs(sv_tot - sv_prev) / max(sv_tot, 1e-6) < 0.03,
        map_drift=abs(P['ao'][last].mean() - P['ao'][prev].mean()) / max(P['ao'][last].mean(), 1e-6))
    # flux mitral : pic protodiastolique E (hors activation atriale) et pic télédiastolique A (pendant)
    ea = _atrial_act(t[last], T, p, pr)
    qf = np.maximum(qmi, 0.0)
    out['mitral_e'] = float(qf[ea <= 0.0].max()) if np.any(ea <= 0.0) else 0.0
    out['mitral_a'] = float(qf[ea > 0.0].max()) if np.any(ea > 0.0) else 0.0
    out['hr_v'] = 60.0 / T
    return out


# ------------------------------------------------------------------ gaz / acide-base
# Gain de la commande ventilatoire à l'acidémie (miroir de gas::PH_GAIN, Rust normatif).
PH_GAIN = 6.0

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
                * math.exp(0.05 - PH_GAIN * (ph - 7.38)) * (1 + 0.03 * max(0.0, 70.0 - paO2))
            return pco2 - 0.863 * vco2 / max(va, 1e-3)
        pco2 = brentq(resid, 5.0, 250.0)
        ph, hco3 = stewart_ph(p, pco2, p['_lact'])
        va = 0.863 * vco2 / pco2
        pAO2 = p['fio2'] * (p['pb'] - 47.0) - pco2 / p['rq']
        aa = p['aagrad'] + p['edema_k'] * max(0.0, hemo['la_mean'] - LA_EDEMA)
        paO2 = max(pAO2 - aa, 15.0)
    p50 = p['p50'] * 10 ** (0.37 * (7.4 - ph))   # Bohr canin : dlogP50/dpH = -0.370 (Cambier 2004)
    sao2 = hb_o2_sat(paO2, p50, p['hill_n'])
    cao2 = o2_content(paO2, p['hb'], p50, p['hill_n'])
    return dict(pco2=pco2, ph=ph, hco3=hco3, be=base_excess(ph, hco3, p['hb']),
                pao2=paO2, sao2=100 * sao2, cao2=cao2, va=va, p50=p50,
                rr=20.0 * (va / va0) ** 0.5 * p['rr_mult'], aa=aa)


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


def _mr_bound(x, amax):
    """Borne anatomique de l'orifice mitral (miroir de engine::mr_bound)."""
    return x / (1.0 + x / amax) if amax > 0 else x


MR_DYN_MIN_A = 0.005      # orifice sous lequel l'orifice dynamique est ignoré (chien sain)
MR_DYN_MAX_ITER = 12


def _hemo_with_rhythm(q, v_eff, n_beats):
    """Orifice mitral dynamique (Yoran 1979) : A = a_mr·(VTD/mr_vref)^mr_k, fixe pendant le
    battement, point fixe amorti sur ln g. Renvoie (hémodynamique, conduction, g retenu)."""
    if q['mr_k'] == 0.0 or q['a_mr'] < MR_DYN_MIN_A:
        h, cond = _hemo_with_rhythm_static(q, v_eff, n_beats)
        return h, cond, q['a_mr']
    qq = dict(q)
    lg = 0.0
    for _ in range(MR_DYN_MAX_ITER):
        qq['a_mr'] = _mr_bound(q['a_mr'] * np.exp(lg), q['mr_amax'])
        h, cond = _hemo_with_rhythm_static(qq, v_eff, n_beats)
        target = q['mr_k'] * np.log(max(h['edv'], 1e-6) / q['mr_vref'])
        if abs(target - lg) < 1e-3:
            break
        lg = 0.5 * lg + 0.5 * target
    return h, cond, qq['a_mr']


def _hemo_with_rhythm_static(q, v_eff, n_beats):
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


BARO_MAX_ITER, BARO_TOL = 16, 0.01


def baro_next(d, g, prev, lo, hi):
    """Solveur du point fixe g(d) = d_t(d) − d = 0 sur [−1, 1] (miroir de engine::baro_next) :
    premier pas amorti d + g/2, puis sécante ; bisection de l'encadrement [lo, hi] si elle en sort."""
    if prev is not None and abs(g - prev[1]) > 1e-12:
        nxt = d - g * (d - prev[0]) / (g - prev[1])
    else:
        nxt = d + 0.5 * g
    if not (lo < nxt < hi):
        nxt = 0.5 * (lo + hi)
    return min(1.0, max(-1.0, nxt))


def _evaluate_once(p, n_beats=22):
    """Une passe : baroréflexe, hémodynamique, aval métabolique (paramètres déjà couplés)."""
    base = dict(p)
    d = 0.0                                   # commande barorécepteur (>0 : hypotension)
    lo, hi, prev = -1.0, 1.0, None
    for it in range(BARO_MAX_ITER):
        s_tot = float(np.clip(base['s0'] + 0.8 * d, 0.0, 1.0))
        v_tot = float(np.clip(base['v0'] + base['vagal_add'] - 0.6 * d, 0.0, 1.0))
        s_eff = s_tot * (1 - base['beta_block'])          # voie chronotrope/inotrope bloquable
        v_eff = v_tot * (1 - base['vagal_block'])         # voie vagale bloquable (atropine)
        q = dict(base)
        # FC sinusale (nœud sinusal) ; K⁺ agit par la conduction (rhythm), plus par un frein ici
        q['hr'] = base['hr'] * hr_factor(s_eff, v_eff, base) / hr_factor(S_REF, V_REF, base)
        q['ees_lv'] = base['ees_lv'] * (1 + 0.4 * (s_eff - base['s0']))
        q['ees_rv'] = base['ees_rv'] * (1 + 0.4 * (s_eff - base['s0']))
        symp = max(0.1, 1.0 - base['av_symp'] * (s_eff - S_REF))   # effet dromotrope du sympathique (ERP, AH)
        q['av_erp'] = base['av_erp'] * symp
        q['pr'] = base['hv'] + base['pa_ia'] + (base['pr'] - base['hv'] - base['pa_ia']) * symp   # AH seul modulé
        # foyers d'échappement : accélérés par le sympathique, le jonctionnel ralenti par le vague
        esc_s = (1.0 + base['esc_a_s'] * s_eff) / (1.0 + base['esc_a_s'] * S_REF)
        esc_v = max(0.0, 1.0 - base['esc_a_v'] * v_eff) / (1.0 - base['esc_a_v'] * V_REF)
        q['junct_rate'] = base['junct_rate'] * esc_s * esc_v
        q['vent_escape_rate'] = base['vent_escape_rate'] * esc_s
        q['r_sys'] = base['r_sys'] * (1 + 0.5 * (s_tot - base['s0']))
        q['vol_shift'] = base['vol_shift'] + 0.20 * 85.0 * base['bw'] * (s_tot - base['s0'])   # recrutement du volume non contraint
        h, cond, g_eff = _hemo_with_rhythm(q, v_eff, n_beats)
        q['a_mr'], q['mr_k'] = g_eff, 0.0       # orifice retenu, réutilisé en aval (signaux, son)
        d_t = float(np.clip(base['baro_gain'] * (base['map_set'] - h['map']) / 45.0, -1, 1))
        g = d_t - d
        d_used = d
        if abs(g) < BARO_TOL:
            break
        if g > 0:
            lo = max(lo, d)
        else:
            hi = min(hi, d)
        nxt = baro_next(d, g, prev, lo, hi)
        prev = (d, g)
        d = nxt
    p = q
    p['hr_sinus'] = q['hr']
    p['hr'] = h['hr_v']                       # FC rapportée : FC ventriculaire
    p['sympathetic'] = s_tot
    auto = dict(s=s_tot, v=v_tot, s_eff=s_eff, v_eff=v_eff, d=d_used, map=h['map'])
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
    perf = float(min(np.clip((h['map'] - 40.0) / 35.0, 0, 1), np.clip((h['co'] / co_ref - (p['renal_co_auto'] - 0.5)) / 0.5, 0, 1)))
    perf = max(perf, 0.25)        # plateau de l'azotémie prérénale (au-delà : IRA, non modélisée)
    # axe cardio-rénal (miroir de engine::metabolic_state) : congestion (PVC) et vasoconstriction rénale
    f_cong = max(0.3, 1.0 - p['renal_k_cong'] * max(0.0, h['cvp'] - p['renal_cvp_ref']))
    f_symp = max(0.3, 1.0 - p['renal_k_symp'] * max(0.0, p['sympathetic'] - p['renal_s_ref']))
    gfr_ratio = p['nephron_mass'] * perf * f_cong * f_symp
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
LA_EDEMA = 24.0           # POG au-delà de laquelle le gradient A-a s'ouvre (gas_exchange) ; 18 → 24 le 21/11/2026 (Guyton & Lindsey 1959)
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


def hypoxemic_drive(p, pao2):
    """Commande de la réponse à l'hypoxémie (miroir de engine::hypoxemic_drive)."""
    span = p['hypox_pao2'] - p['hypox_pao2_sat']
    if p['hypox_pao2'] <= 0.0 or span <= 0.0:
        return 0.0
    x = min(1.0, max(0.0, (p['hypox_pao2'] - pao2) / span))
    return x ** p['hypox_n']


def evaluate(p, n_beats=22):
    """Évalue le patient avec les couplages de décompensation (point fixe amorti).

    Hypokaliémie -> faiblesse ventilatoire (direct) ; acidémie et hypoxie tissulaire -> dépression
    de la contractilité, acidémie -> vasoplégie (boucle). Lève NonViable si la pompe s'arrête
    (FC < hr_arrest), si la circulation s'effondre (DC < co_arrest_frac × DC de référence), si aucun
    état stationnaire n'existe (racine introuvable) ou si la spirale ne converge pas."""
    import periph
    p0 = periph.apply_temperature(dict(p))   # FC intrinsèque, VO2, P50 (identité à 38,6 °C)
    bw = p0['bw']
    co_ref = 3.29 * (bw / 20.0) ** 0.75
    f_vent = max(F_FLOOR, 1.0 - p0['k_weak_vent'] * max(0.0, p0['k_weak'] - p0['k']))
    fc, fr, fh = 1.0, 1.0, 0.0
    last = None
    converged = False
    it = 0
    for it in range(DECOMP_MAX_ITER):
        q = dict(p0)
        q['vent_drive'] = p0['vent_drive'] * f_vent
        q['s0'] = min(1.0, p0['s0'] + p0['hypox_symp'] * fh)       # réponse à l'hypoxémie
        q['ees_lv'] = p0['ees_lv'] * fc * (1.0 + p0['hypox_inotropy'] * fh)
        q['ees_rv'] = p0['ees_rv'] * fc * (1.0 + p0['hypox_inotropy'] * fh)
        q['r_sys'] = p0['r_sys'] * fr * max(0.2, 1.0 - p0['hypox_vaso'] * fh)
        if p0['anemia_visc'] != 0.0 and p0['hb_ref'] > 0.0:      # rein → cœur : anémie (viscosité)
            q['r_sys'] *= (p0['hb'] / p0['hb_ref']) ** p0['anemia_visc']
        try:
            res = _evaluate_once(q, n_beats)
        except (ValueError, ZeroDivisionError):
            raise NonViable('no_steady_state', last)
        if res['p']['hr'] < p0['hr_arrest']:
            raise NonViable('cardiac_arrest', res)
        if res['hemo']['co'] < p0['co_arrest_frac'] * co_ref:
            raise NonViable('circulatory_collapse', res)
        if res['hemo']['map'] < p0['map_arrest']:
            raise NonViable('irreversible_shock', res)    # PAM sous le seuil de perfusion (choc irréversible)
        if res['er'] >= 1.0:
            raise NonViable('oxygen_debt', res)       # VO2 > DO2 : aucun état aérobie possible
        last = res
        tc, tr, f_acid, f_hyp, f_cor = coupling_targets(p0, res)
        th = hypoxemic_drive(p0, res['art']['pao2'])
        if abs(tc - fc) < DECOMP_TOL and abs(tr - fr) < DECOMP_TOL and abs(th - fh) < DECOMP_TOL:
            converged = True
            break
        fc = 0.5 * fc + 0.5 * tc
        fr = 0.5 * fr + 0.5 * tr
        fh = 0.5 * fh + 0.5 * th
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
    if fh > 1e-3:
        mech.append(dict(id='hypoxemic_drive', value=res['art']['pao2'], threshold=p0['hypox_pao2'], factor=fh))
    res['outcome'] = dict(state='decompensated' if mech else 'stable', mechanisms=mech)
    return res


# ------------------------------------------------------------------ hémodynamique battement par battement
def _act1(tau, ts, tr, g=1.0):
    """Activation d'un battement isolé, `tau` secondes après son début (même forme que _act)."""
    return np.where((tau >= 0) & (tau < ts), 0.5 * (1 - np.cos(np.pi * np.power(np.clip(tau / ts, 0.0, None), g))),
                    np.where((tau >= ts) & (tau < ts + tr), 0.5 * (1 + np.cos(np.pi * (tau - ts) / tr)), 0.0))


def beat_schedule(cond, period0, p):
    """Battements ventriculaires (t, ts, tr, c) et activations atriales organisées (t) d'une
    séquence de conduction (rhythm.simulate_conduction).

    Durée de systole : ts_coef·√(période moyenne)·(RR_i / période moyenne)^ts_restitution,
    RR_i = intervalle qui précède (période nominale pour le premier battement).

    Amplitude d'élastance active c_i (relation force-intervalle, modèle de recirculation du Ca²⁺
    à deux compartiments du réticulum sarcoplasmique) :
    - à partir de la fin de la systole précédente, le Ca²⁺ recapté (compartiment de captation,
      contenu u) gagne le compartiment de libération avec la constante `ca_tau` ; au battement,
      tout le compartiment de libération est libéré : libération = u·(1 − exp(−d/ca_tau)),
      d = temps écoulé depuis la fin de la systole précédente ;
    - après le battement, une fraction `ca_recirc` du Ca²⁺ libéré est recaptée, le reste est
      extrudé et remplacé par l'entrée transsarcolemmale (1 − ca_recirc) ; le Ca²⁺ non transféré
      reste dans le compartiment de captation.
    En rythme régulier, quelle que soit la fréquence, la libération vaut 1 (régime établi : le
    périodique est retrouvé). Un battement prématuré libère moins (restitution mécanique) ; le
    Ca²⁺ resté en attente renforce le battement suivant (potentialisation post-extrasystolique).
    Un battement à QRS large (ectopique, échappement ventriculaire) active le myocarde hors du
    réseau de Purkinje : contraction dyssynchrone, amplitude multipliée par `ees_wide`."""
    tv = [b['t'] for b in cond['ventricular']]
    wide = [b.get('wide', False) for b in cond['ventricular']]
    ts0 = p['ts_coef'] * math.sqrt(period0)
    tau, rho = p['ca_tau'], p['ca_recirc']
    u = 1.0 / (1.0 - math.exp(-(period0 - ts0) / tau))     # compartiment de captation, régime établi
    beats = []
    ts_prev = ts0
    for i, t in enumerate(tv):
        rr = t - tv[i - 1] if i > 0 else period0
        # durée de systole adaptée à la fréquence moyenne, avec une restitution modeste au RR
        # précédent (exposant ts_restitution) : identique au régime périodique si RR = RR moyen
        ts = p['ts_coef'] * math.sqrt(period0) * (rr / period0) ** p['ts_restitution']
        x = math.exp(-max(0.0, rr - ts_prev) / tau)
        rel = u * (1.0 - x)                                  # Ca²⁺ libéré (1 en régime établi)
        u = u * x + rho * rel + (1.0 - rho)
        c = rel * (p['ees_wide'] if wide[i] else 1.0)
        beats.append((t + mech_offset(p, wide[i]), ts, p['tr_frac'] * ts, c))
        ts_prev = ts
    atr = [t + p['emd_a'] for t, organized in cond['atrial'] if organized]
    return beats, atr


def _activations(t, beats, atr, p):
    """Activation ventriculaire e (max des deux derniers battements, pour qu'un battement prématuré
    survienne pendant la relaxation du précédent), activation active e_act (max des activations
    pondérées par l'amplitude c de chaque battement) et activation atriale à l'instant t."""
    import bisect
    tv = beat_starts = [b[0] for b in beats]
    i = bisect.bisect_right(beat_starts, t) - 1
    e = e_act = 0.0
    for j in (i - 1, i):
        if 0 <= j < len(beats):
            t0, ts, tr, c = beats[j]
            a = float(_act1(t - t0, ts, tr, p['act_gamma']))
            e = max(e, a)
            e_act = max(e_act, c * a)
    k = bisect.bisect_right(atr, t) - 1
    ea = 0.0
    for j in (k - 1, k):
        if 0 <= j < len(atr):
            ea = max(ea, float(_act1(t - atr[j], p['ta_atr'], p['tra_atr'])))
    return e, e_act, ea


def _flows_beats(V, t, p, beats, atr):
    lv, rv, ao, sv, pa, pv, qa, qp = V
    e, e_act, ea = _activations(t, beats, atr, p)
    P = dict(
        lv=_vent_p(lv, e, p['ees_lv'], p['v0_lv'], p['a_lv'], p['b_lv'], e_act),
        rv=_vent_p(rv, e, p['ees_rv'], p['v0_rv'], p['a_rv'], p['b_rv'], e_act),
        ao=(ao - p['v0_ao']) / p['c_ao'] + p['z_ao'] * qa, sv=(1 + p['k_ra'] * ea) * (sv - p['v0_sv']) / (p['c_sv'] * p['ven_cap']),
        pa=(pa - p['v0_pa']) / p['c_pa'] + p['z_pa'] * qp, pv=(1 + p['k_la'] * ea) * (pv - p['v0_pv']) / p['c_pv'])
    Q, dqa, dqp = _valve_flows(P, qa, qp, p)
    Q['_dqa'] = dqa
    Q['_dqp'] = dqp
    return P, Q, e, ea


def simulate_hemo_beats(p, cond, t_end, warm, period0, dt=0.002):
    """Simule la circulation sur une séquence de battements quelconque (rythme irrégulier, blocs,
    extrasystoles, FA). Indicateurs moyennés sur les battements complets après `warm` secondes ;
    chaque battement garde son VES, sa PAS et sa PAD (déficit de pouls, alternance)."""
    beats, atr = beat_schedule(cond, period0, p)
    V0 = _initial_volumes(p)

    def rhs(t, V):
        _, Q, _, _ = _flows_beats(V, t, p, beats, atr)
        return [Q['mi'] - Q['ao'], Q['tr'] - Q['pu'], Q['ao'] - Q['sys'],
                Q['sys'] - Q['tr'], Q['pu'] - Q['pul'], Q['pul'] - Q['mi'], Q['_dqa'], Q['_dqp']]

    t_eval = np.arange(warm, t_end, dt)
    sol = solve_ivp(rhs, (0, t_end), V0, method='LSODA', t_eval=t_eval, rtol=1e-6, atol=1e-6, max_step=0.004)
    V, t = sol.y, sol.t
    rows = [_flows_beats(V[:, j], t[j], p, beats, atr) for j in range(len(t))]
    P = {k: np.array([r[0][k] for r in rows]) for k in rows[0][0]}
    Q = {k: np.array([r[1][k] for r in rows]) for k in rows[0][1] if k not in ('_dqa', '_dqp')}
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
        bt = next((bb for bb in beats if bb[0] == a), None)
        ts_a = bt[1] if bt is not None else p['ts_coef'] * math.sqrt(b - a)
        # volumes du battement mesurés sur SA contraction (début → fin de relaxation, ou battement
        # suivant) : VTS = minimum, VTD = maximum avant ce minimum. Le remplissage de la diastole
        # qui suit appartient au battement suivant (sinon une pause gonfle le VES du battement prématuré).
        tr_a = bt[2] if bt is not None else p['tr_frac'] * ts_a
        vs = V[0][m & (t < a + ts_a + tr_a)]
        if len(vs) < 3:
            vs = vl
        j_es = int(np.argmin(vs))
        esv_b, edv_b = float(vs[j_es]), float(vs[:j_es + 1].max())
        per_beat.append(dict(t=a, rr=b - a, relaxed=relaxed.get(a, True), ts=ts_a, c=bt[3] if bt is not None else 1.0,
                             edv=edv_b, esv=esv_b, sv=edv_b - esv_b,
                             sap=P['ao'][m].max(), dap=P['ao'][m].min(), lv_edp=_edp_beat(t, P['lv'], m, a),
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
