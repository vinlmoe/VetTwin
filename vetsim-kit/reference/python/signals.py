"""Signaux synthétiques : ECG (dérivation II) et phonocardiogramme.

ECG : modèle phénoménologique (somme de gaussiennes) piloté par FC, K+, etc.
PCG : sons déclenchés par les événements valvulaires SIMULÉS ; le souffle est modulé
      par le débit régurgitant simulé (pas de règle de maladie codée en dur).
"""
import numpy as np
from scipy.signal import butter, sosfilt


# ------------------------------------------------------------------ ECG
# Forme des ondes (docs/VERIFICATION_ECG.md). Chaque onde est une gaussienne ; ses largeurs sont
# calées pour que les durées VISIBLES (seuil 0,02 mV) soient celles des intervalles publiés :
# P 45 ms (40–50 ms, Lerdweeraphon 2020 ; 44 ms, Bispo 2025), QRS visible = paramètre `qrs`
# (Q et S compris), segment ST isoélectrique avant T (Romito 2022 le mesure chez 180 chiens sains).
TA_AMP, TA_WIDTH = -0.35, 2.3   # onde Ta : amplitude (× P, négative) et largeur (× P) ; Perego 2014, Mirvis 1986
P_SIGMA = 0.0105        # largeur de P (s) : 45 ms visibles à 0,2 mV
P_WIDEN_K = 0.25        # élargissement relatif de P par mmol/L de K au-delà de 6 (placeholder D)
R_SIGMA = 1.0 / 10      # largeur de R (fraction de `qrs`)
Q_POS, Q_SIGMA = 0.34, 1.0 / 14   # centre (avant R) et largeur de Q (fractions de `qrs`)
S_POS, S_SIGMA = 0.34, 1.0 / 13   # centre (après R) et largeur de S
T_W0 = 0.025            # largeur de T (s) au repos : ST de 47 ms à 88/min (placeholder D, aucune durée de T lue)
WIDE_R_SIGMA = 1.0 / 8  # battement ventriculaire : largeur de R, centre et largeur de S
WIDE_S_POS, WIDE_S_SIGMA = 0.40, 1.0 / 11   # (QRS visible = WIDE_QRS · `qrs`)


def ta_wave(p_c, p_w):
    """Centre et largeur de l'onde Ta : débute à la fin de P (± 2 σ), culmine dans le segment PR."""
    w = TA_WIDTH * p_w
    return p_c + 2.0 * p_w + w, w


def ecg_features(p):
    """Intervalles et amplitudes effectifs (effet K+ simple, par paliers continus)."""
    k = p['k']
    pr, qrs, qt = p['pr'], p['qrs'], p['qt']
    p_amp, t_amp, r_amp = p['p_amp'], p['t_amp'], p['r_amp']
    t_w = T_W0
    p_w = P_SIGMA
    if k > 5.5:
        d = k - 5.5
        t_amp *= 1 + 0.45 * d                 # onde T pointue, ample
        t_w = T_W0 * max(0.025, 0.045 - 0.006 * d) / 0.045   # et étroite (même loi relative)
        p_w *= 1 + P_WIDEN_K * max(0.0, k - 6.0)              # P plus large et plus petite
        qt *= max(0.8, 1 - 0.04 * d)          # QT raccourci (placeholder D)
        p_amp *= max(0.0, 1 - 0.25 * max(0.0, k - 6.0))
        pr *= 1 + 0.07 * max(0.0, k - 6.0)
        qrs *= 1 + 0.12 * max(0.0, k - 6.5)
        # disparition de l'onde P : arrêt atrial décidé par la conduction (rhythm), plus ici
    elif k < 3.2:
        d = 3.2 - k
        t_amp *= max(0.2, 1 - 0.3 * d)
        qt *= 1 + 0.08 * d
    return dict(pr=pr, qrs=qrs, qt=qt, p_amp=p_amp, t_amp=t_amp, r_amp=r_amp, t_w=t_w, p_w=p_w)


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
        q = f['qrs']
        sig += g(tr_ - f['pr'] + 0.5 * 0.03, f['p_amp'], f['p_w'])                  # P
        tc, tw = ta_wave(tr_ - f['pr'] + 0.5 * 0.03, f['p_w'])
        sig += g(tc, TA_AMP * f['p_amp'], tw)                                       # Ta
        sig += g(tr_ - Q_POS * q, -0.12 * f['r_amp'], Q_SIGMA * q)                  # Q
        sig += g(tr_, f['r_amp'], R_SIGMA * q)                                      # R
        sig += g(tr_ + S_POS * q, -0.20 * f['r_amp'], S_SIGMA * q)                  # S
        t_center = tr_ - 0.5 * f['qrs'] + f['qt'] - 2.2 * f['t_w']
        sig += g(t_center, f['t_amp'], f['t_w'])                                    # T
    sig += 0.01 * rng.standard_normal(len(t)) + 0.02 * np.sin(2 * np.pi * 0.25 * t)
    return t, sig, f


# Battements ventriculaires (ectopique, échappement) : QRS large et onde T discordante ;
# fibrillation atriale : petites ondes f à chaque activation atriale désorganisée, d'amplitude
# proportionnelle au temps de récupération du tissu (intervalle depuis l'activation précédente,
# rapporté au cycle moyen `af_cycle`, plafonné à F_WAVE_MAX) : la variabilité de l'amplitude
# vient de celle des cycles de la conduction, sans tirage propre. Placeholders D.
WIDE_QRS = 2.2          # facteur de largeur du QRS d'un battement ventriculaire
WIDE_R = 1.2            # facteur d'amplitude de son onde R
WIDE_T = -1.5           # onde T discordante (facteur de t_amp)
F_WAVE = 0.3            # amplitude des ondes f (facteur de p_amp) au cycle moyen
F_WAVE_MAX = 2.0        # plafond du rapport intervalle / cycle moyen


def make_ecg_conducted(p, cond, hr, dur=8.0, fs=500, seed=0):
    """ECG construit depuis la séquence de conduction (rhythm.simulate_conduction) : onde P à
    chaque activation atriale organisée (aucune en arrêt atrial), ondes f en FA, QRS à chaque
    activation ventriculaire après le PR de ce battement, QRS large et T discordante pour un
    battement ventriculaire, QT corrigé par le RR précédent (Fridericia)."""
    rng = np.random.default_rng(seed)
    f = ecg_features(p)
    t = np.arange(0, dur, 1 / fs)
    sig = np.zeros_like(t)
    g = lambda c, a, w: a * np.exp(-0.5 * ((t - c) / w) ** 2)
    prev_a = None
    for ta, organized in cond['atrial']:
        if ta > dur + 0.5:
            break
        if organized:
            sig += g(ta + 0.5 * 0.03, f['p_amp'], f['p_w'])                         # P
            tc, tw = ta_wave(ta + 0.5 * 0.03, f['p_w'])
            sig += g(tc, TA_AMP * f['p_amp'], tw)                                  # Ta
        else:
            rec = 1.0 if prev_a is None else min(F_WAVE_MAX, (ta - prev_a) / p['af_cycle'])
            sig += g(ta, F_WAVE * p['p_amp'] * rec, 0.012)                         # onde f
        prev_a = ta
    prev = None
    for b in cond['ventricular']:
        tv = b['t']
        if tv > dur + 0.5:
            break
        rr = tv - prev if prev is not None else 60.0 / hr
        prev = tv
        qt = f['qt'] * (95.0 / max(60.0 / rr, 30.0)) ** (1.0 / 3.0)
        if b['wide']:
            qrs = f['qrs'] * WIDE_QRS
            sig += g(tv, WIDE_R * f['r_amp'], WIDE_R_SIGMA * qrs)                    # R large
            sig += g(tv + WIDE_S_POS * qrs, -0.35 * f['r_amp'], WIDE_S_SIGMA * qrs)  # S
            sig += g(tv - 0.5 * qrs + qt - 2.2 * f['t_w'], WIDE_T * f['t_amp'], 1.5 * f['t_w'])
        else:
            qrs = f['qrs']
            sig += g(tv - Q_POS * qrs, -0.12 * f['r_amp'], Q_SIGMA * qrs)            # Q
            sig += g(tv, f['r_amp'], R_SIGMA * qrs)                                 # R
            sig += g(tv + S_POS * qrs, -0.20 * f['r_amp'], S_SIGMA * qrs)            # S
            sig += g(tv - 0.5 * qrs + qt - 2.2 * f['t_w'], f['t_amp'], f['t_w'])     # T
    sig += 0.01 * rng.standard_normal(len(t)) + 0.02 * np.sin(2 * np.pi * 0.25 * t)
    return t, sig, f


SIGNAL_WARM_S = 5.0     # chauffe avant la fenêtre des signaux (son battement par battement)


def shift_conduction(cond, w):
    """Séquence de conduction décalée : événements de [w, ∞) ramenés à l'origine."""
    return dict(atrial=[(t - w, o) for t, o in cond['atrial'] if t >= w],
                ventricular=[dict(b, t=b['t'] - w, t_a=None if b['t_a'] is None else b['t_a'] - w)
                             for b in cond['ventricular'] if b['t'] >= w])


def patient_conduction(r, sinus_times, seed):
    """Séquence de conduction des signaux : décharges sinusales du modèle autonome (variabilité
    respiratoire) propagées par la couche de conduction ; aléa SplitMix64(seed)."""
    import rhythm
    p = r['p']
    g = rhythm.SplitMix64(seed)
    t_end = float(sinus_times[-1]) if len(sinus_times) else 0.0
    return rhythm.simulate_conduction(p, r['autonomic']['v_eff'], 60.0 / p.get('hr_sinus', p['hr']), t_end,
                                      g.random, sinus_times=list(sinus_times))


# ------------------------------------------------------------------ PCG
def _beat_events(h, dt=0.002):
    t = h['t']
    qao, qmi = h['Q_ao'], h['Q_mi']
    plv, pao, ppv = h['P_lv'], h['P_ao'], h['P_pv']
    # fermeture mitrale : P_lv dépasse P_pv après avoir été inférieure
    d = plv - ppv
    idx = np.where((d[:-1] < 0) & (d[1:] >= 0))[0]
    # déjà fermée au premier échantillon du cycle : fermeture à son début (miroir de signals::beat_events)
    i_mc = 0 if d[0] >= 0 else (int(idx[0]) if len(idx) else 0)
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
