"""Périphérie : thermorégulation, pupille, digestif et sécrétions, SNC (miroir de
`crates/vetsim-core/src/periph.rs`, référence normative ; sources dans le code Rust).
Usage pédagogique uniquement."""
import math

T_REF = 38.6; C_BODY = 3470.0; CORE_FRAC = 0.69; T_SHELL0 = 34.6; T_NEUTRAL = 26.0
E_RESP0 = 0.20; J_PER_ML_O2 = 20.1; VO2_REF = 5.5; Q10_VO2 = 1.7; Q10_HR = 1.51
BOHR_T = 0.021; MAC_T = 0.086; VM_GAIN = 3.0; VM_MIN = 0.2; VM_MAX = 3.0
SHIVER_MAX = 3.0; SHIVER_OFFSET = 0.5; SHIVER_SPAN = 1.5
PANT_MAX = 4.0; PANT_OFFSET = 0.2; PANT_SPAN = 1.0
NASAL_GLAND_FRAC = 0.27; ANESTH_MET = 0.10; THERMO_DT = 60.0; THERMO_EQ_H = 72.0

PUPIL_MIN = 1.0; PUPIL_MAX = 11.0; PUPIL_AMBIENT = 8.53; PUPIL_DARK = 10.06; PUPIL_FLASH = 6.57
S_REF = 0.2
LES_BASE = 47.9; LES_MUSC = 1.0 - 13.2 / 47.9; GI_ANESTH = 1.0 - 3.0 / 11.0
MAC_SED50 = 0.4; SED_BASE = 1.0
BIS_TABLE = [(0.8, 65.0), (1.0, 60.0), (1.5, 52.0), (2.0, 31.0)]


def _clip(x, lo, hi):
    return min(max(x, lo), hi)


def effectors(p, tc):
    inhib = _clip(p['anesth_mac'], 0.0, 1.0)
    e = tc - p['t_set']
    vm = _clip(VM_GAIN * e, math.log(VM_MIN), math.log(VM_MAX))
    return dict(inhib=inhib, vasomotor=math.exp((1 - inhib) * vm),
                shiver=(1 - inhib) * SHIVER_MAX * _clip((-e - SHIVER_OFFSET) / SHIVER_SPAN, 0.0, 1.0),
                panting=(1 - inhib) * PANT_MAX * _clip((e - PANT_OFFSET) / PANT_SPAN, 0.0, 1.0))


def _conductances(p):
    m_ref = VO2_REF * J_PER_ML_O2 / 60.0
    s = p['bw'] / 20.0
    k0 = (1 - E_RESP0) * m_ref / (T_REF - T_SHELL0) * s ** -0.25
    g = (1 - E_RESP0) * m_ref / (T_SHELL0 - T_NEUTRAL) * s ** (-1.0 / 3.0)
    return k0, g


def thermo_rhs(p, tc, ts):
    m0 = p['vo2'] * J_PER_ML_O2 / 60.0
    k0, g = _conductances(p)
    ef = effectors(p, tc)
    m_basal = m0 * (1 - ANESTH_MET * ef['inhib']) * Q10_VO2 ** ((tc - T_REF) / 10.0)
    m = m_basal + m0 * ef['shiver']
    evap = (E_RESP0 * m_basal + m0 * ef['panting']) * (1 - NASAL_GLAND_FRAC * p['musc_block'])
    q_cs = k0 * ef['vasomotor'] * (tc - ts)
    return ((m - evap - q_cs) / (CORE_FRAC * C_BODY),
            (q_cs - g * (ts - p['t_amb']) + p['warming']) / ((1 - CORE_FRAC) * C_BODY))


def _rk4(p, tc, ts, dt):
    a = thermo_rhs(p, tc, ts)
    b = thermo_rhs(p, tc + dt / 2 * a[0], ts + dt / 2 * a[1])
    c = thermo_rhs(p, tc + dt / 2 * b[0], ts + dt / 2 * b[1])
    d = thermo_rhs(p, tc + dt * c[0], ts + dt * c[1])
    return (tc + dt / 6 * (a[0] + 2 * b[0] + 2 * c[0] + d[0]),
            ts + dt / 6 * (a[1] + 2 * b[1] + 2 * c[1] + d[1]))


def thermo_equilibrium(p):
    tc, ts = p['t_set'], T_SHELL0
    for _ in range(int(round(THERMO_EQ_H * 3600 / THERMO_DT))):
        tc, ts = _rk4(p, tc, ts, THERMO_DT)
    return tc, ts


def thermal_course(p, doses, t_eval_h):
    """Températures (noyau, enveloppe) aux instants demandés ; mêmes conventions que le Rust."""
    import pkpd
    t_end = max([0.0, *t_eval_h])
    n = int(math.ceil(t_end * 3600 / THERMO_DT))
    grid = [k * THERMO_DT / 3600 for k in range(n + 1)]
    tc_ = pkpd.timecourse(p, doses, grid[-1], grid) if doses else None
    drugs = sorted({d[1] for d in doses})
    st = [thermo_equilibrium(p)]
    tc, ts = st[0]
    for k in range(n):
        if tc_ is None:
            q = p
        else:
            conc = {d: tc_[d][k] for d in drugs}
            conc_e = {d: tc_[d + '@e'][k] for d in drugs}
            q = pkpd.apply_drug_effects(p, conc, tc_['D'][k], tc_['U'][k], conc_e)
        tc, ts = _rk4(q, tc, ts, THERMO_DT)
        st.append((tc, ts))
    out = []
    for t in t_eval_h:
        x = t * 3600 / THERMO_DT
        i = min(int(math.floor(x)), n); j = min(i + 1, n); w = x - i
        out.append((st[i][0] + w * (st[j][0] - st[i][0]), st[i][1] + w * (st[j][1] - st[i][1])))
    return out


def apply_temperature(p):
    q = dict(p)
    dt = p['temp_c'] - T_REF
    ef = effectors(p, p['temp_c'])
    q['hr'] = p['hr'] * Q10_HR ** (dt / 10.0)
    q['vo2'] = p['vo2'] * ((1 - ANESTH_MET * ef['inhib']) * Q10_VO2 ** (dt / 10.0) + ef['shiver'])
    q['p50'] = p['p50'] * 10 ** (BOHR_T * dt)
    return q


def _open(dil, sph):
    return dil / (dil + sph) if dil + sph > 0 else 0.5


def pupil(p, s_total):
    span = PUPIL_MAX - PUPIL_MIN
    f = lambda d: (d - PUPIL_MIN) / span   # noqa: E731
    dil0 = f(PUPIL_AMBIENT) / (1 - f(PUPIL_AMBIENT))
    sph_dark = dil0 * (1 - f(PUPIL_DARK)) / f(PUPIL_DARK)
    flash_gain = dil0 * (1 - f(PUPIL_FLASH)) / f(PUPIL_FLASH)
    dil = dil0 * max(0.0, s_total) / S_REF
    light = _clip(p['light'], 0.0, 1.0)
    sph = (1 + p['pupil_ew']) * (sph_dark + (1 - sph_dark) * light) * (1 - _clip(p['pupil_block'], 0.0, 1.0))
    d = PUPIL_MIN + span * _open(dil, sph)
    df = PUPIL_MIN + span * _open(dil, sph * flash_gain)
    return dict(diameter=d, flash=df, plr_pct=100 * (d - df) / d)


def bis(mac):
    if not BIS_TABLE[0][0] <= mac <= BIS_TABLE[-1][0]:
        return None
    for (x0, y0), (x1, y1) in zip(BIS_TABLE, BIS_TABLE[1:]):
        if mac <= x1:
            return y0 + (y1 - y0) * (mac - x0) / (x1 - x0)


def peripheral(p, s_total):
    mac_eff = max(0.0, p['anesth_mac']) * math.exp(-MAC_T * (p['temp_c'] - T_REF))
    musc = _clip(p['musc_block'], 0.0, 1.0)
    drive = max(0.0, p['sedation']) + (mac_eff / MAC_SED50) ** 4
    th = effectors(p, p['temp_c'])
    pu = pupil(p, s_total)
    out = dict(temp_c=p['temp_c'], shiver=th['shiver'], panting=th['panting'], vasomotor=th['vasomotor'],
               pupil_mm=pu['diameter'], plr_pct=pu['plr_pct'],
               gi_motility=(1 - musc) * (1 - _clip(p['gi_inhib'], 0.0, 1.0)) * (1 - GI_ANESTH * min(mac_eff, 1.0)),
               les_mmhg=LES_BASE * (1 - LES_MUSC * musc), salivation=1 - musc, mac_eff=mac_eff,
               sedation_score=SED_BASE + (21 - SED_BASE) * drive / (1 + drive),
               excitation=_clip(p['excitation'], 0.0, 1.0))
    b = bis(mac_eff)
    if b is not None:
        out['bis'] = b
    return out
