"""Signaux synthétiques : ECG (dérivation II) et phonocardiogramme.

ECG : modèle phénoménologique (somme de gaussiennes) piloté par FC, K+, etc.
PCG : sons déclenchés par les événements valvulaires SIMULÉS ; le souffle est modulé
      par le débit régurgitant simulé (pas de règle de maladie codée en dur).
"""
import numpy as np
from scipy.signal import butter, sosfilt


# ------------------------------------------------------------------ ECG
def ecg_features(p):
    """Intervalles et amplitudes effectifs (effet K+ simple, par paliers continus)."""
    k = p['k']
    pr, qrs, qt = p['pr'], p['qrs'], p['qt']
    p_amp, t_amp, r_amp = p['p_amp'], p['t_amp'], p['r_amp']
    t_w = 0.045
    p_visible = True
    if k > 5.5:
        d = k - 5.5
        t_amp *= 1 + 0.45 * d                 # onde T pointue, ample
        t_w = max(0.025, 0.045 - 0.006 * d)   # et étroite
        p_amp *= max(0.0, 1 - 0.25 * max(0.0, k - 6.0))
        pr *= 1 + 0.07 * max(0.0, k - 6.0)
        qrs *= 1 + 0.12 * max(0.0, k - 6.5)
        if k > 7.8:
            p_visible = False                  # stase atriale (rythme sinoventriculaire)
    elif k < 3.2:
        d = 3.2 - k
        t_amp *= max(0.2, 1 - 0.3 * d)
        qt *= 1 + 0.08 * d
    return dict(pr=pr, qrs=qrs, qt=qt, p_amp=p_amp if p_visible else 0.0,
                t_amp=t_amp, r_amp=r_amp, t_w=t_w)


def make_ecg(p, hr, rr_resp, dur=8.0, fs=500, seed=0, r_times=None):
    rng = np.random.default_rng(seed)
    f = ecg_features(p)
    f['qt'] *= (95.0 / max(hr, 30.0)) ** (1.0 / 3.0)       # correction de fréquence (Fridericia)
    t = np.arange(0, dur, 1 / fs)
    sig = np.zeros_like(t)
    if r_times is not None:
        tR = list(r_times)
    else:                                          # repli : RSA sinusoïdale simple
        tR = []
        tcur = 0.25
        while tcur < dur + 0.5:
            tR.append(tcur)
            tcur += 60.0 / hr * (1 + 0.06 * np.sin(2 * np.pi * (rr_resp / 60.0) * tcur))
    g = lambda c, a, w: a * np.exp(-0.5 * ((t - c) / w) ** 2)
    for tr_ in tR:
        qrs_w = f['qrs'] / 5.0
        sig += g(tr_ - f['pr'] + 0.5 * 0.03, f['p_amp'], 0.022)                    # P
        sig += g(tr_ - 0.55 * f['qrs'], -0.12 * f['r_amp'], qrs_w * 0.8)            # Q
        sig += g(tr_, f['r_amp'], qrs_w)                                            # R
        sig += g(tr_ + 0.55 * f['qrs'], -0.20 * f['r_amp'], qrs_w * 0.9)            # S
        t_center = tr_ - 0.5 * f['qrs'] + f['qt'] - 2.2 * f['t_w']
        sig += g(t_center, f['t_amp'], f['t_w'])                                    # T
    sig += 0.01 * rng.standard_normal(len(t)) + 0.02 * np.sin(2 * np.pi * 0.25 * t)
    return t, sig, f


# ------------------------------------------------------------------ PCG
def _beat_events(h, dt=0.002):
    t = h['t']
    qao, qmi = h['Q_ao'], h['Q_mi']
    plv, pao, ppv = h['P_lv'], h['P_ao'], h['P_pv']
    # fermeture mitrale : P_lv dépasse P_pv après avoir été inférieure
    d = plv - ppv
    idx = np.where((d[:-1] < 0) & (d[1:] >= 0))[0]
    i_mc = int(idx[0]) if len(idx) else 0
    # fermeture aortique : débit aortique passe de >0 à <=0 après le pic
    ipk = int(np.argmax(qao))
    neg = np.where(qao[ipk:] <= 0)[0]
    i_ac = ipk + int(neg[0]) if len(neg) else min(ipk + 100, len(t) - 1)
    dpdt = np.gradient(plv, dt).max()
    q_early = qmi[i_ac:].max() if i_ac < len(qmi) else 0.0
    return dict(i_mc=i_mc, i_ac=i_ac, dpdt_max=dpdt, q_early=q_early,
                dp_close=pao[i_ac] - plv[i_ac] + pao[i_ac])


def _burst(n, fs, f0, tau, amp, phase=0.0):
    tt = np.arange(n) / fs
    return amp * np.exp(-tt / tau) * np.sin(2 * np.pi * f0 * tt + phase)


def make_pcg(h, fs=4000, n_beats=4, seed=1):
    rng = np.random.default_rng(seed)
    dt = h['t'][1] - h['t'][0]
    ev = _beat_events(h, dt)
    T = h['T']
    nb = int(round(T * fs))
    sig = np.zeros(nb * n_beats)
    # flux de régurgitation systolique (valve mitrale + aortique diastolique) et éjection
    q_reg = np.maximum(-h['Q_mi'], 0.0) + np.maximum(-h['Q_ao'], 0.0)
    q_ej = np.maximum(h['Q_ao'], 0.0)
    tt = np.arange(nb) / fs
    q_reg_u = np.interp(tt, h['t'], q_reg)
    q_ej_u = np.interp(tt, h['t'], q_ej)
    # bruit coloré 90-450 Hz
    sos = butter(3, [90, 450], btype='band', fs=fs, output='sos')
    noise = sosfilt(sos, rng.standard_normal(nb * n_beats))
    t_mc, t_ac = ev['i_mc'] * dt, ev['i_ac'] * dt
    s1_amp = 0.5 * min(2.0, ev['dpdt_max'] / 1800.0)
    s2_amp = 0.4 * min(2.0, ev['dp_close'] / 190.0)
    s3_amp = 0.35 * max(0.0, (ev['q_early'] - 160.0) / 160.0) if h['la_mean'] > 15 else 0.0
    for b in range(n_beats):
        o = b * nb
        def put(t0, x):
            i0 = o + int(t0 * fs)
            seg = sig[i0:i0 + len(x)]
            sig[i0:i0 + len(seg)] += x[:len(seg)]
        put(t_mc, _burst(int(0.09 * fs), fs, 70, 0.025, s1_amp))
        put(t_ac, _burst(int(0.08 * fs), fs, 110, 0.020, s2_amp))
        if s3_amp > 0:
            put(t_ac + 0.12, _burst(int(0.09 * fs), fs, 45, 0.030, s3_amp))
        env = 0.0035 * q_reg_u + 0.00015 * (q_ej_u / 40.0) ** 2 * 1.0
        sig[o:o + nb] += env * noise[o:o + nb]
    peak_reg = float(q_reg.max())
    # grade (Levine) indicatif d'après le débit régurgitant maximal : heuristique à calibrer
    thr = [10, 25, 50, 80, 120]
    grade = int(np.searchsorted(thr, peak_reg)) + 1
    grade = min(grade, 6) if peak_reg > 3.0 else 0
    feats = dict(s1=round(s1_amp, 2), s2=round(s2_amp, 2), s3=s3_amp > 0.02,
                 murmur_peak_flow=round(peak_reg, 1),
                 murmur='holosystolique apexien gauche' if (np.maximum(-h['Q_mi'], 0).max() > 3.0) else
                 ('diastolique basal gauche' if np.maximum(-h['Q_ao'], 0).max() > 3.0 else 'aucun'),
                 grade=grade)
    t = np.arange(len(sig)) / fs
    sig = sig / max(np.abs(sig).max(), 1e-9)
    return t, sig, feats
