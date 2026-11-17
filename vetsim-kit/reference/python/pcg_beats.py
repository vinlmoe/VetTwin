"""Auscultation battement par battement (référence ; port Rust : `pcg::synthesize_pcg_beats`).

Même synthèse que signals_pcg_v5.make_pcg (qui reste la référence du rythme régulier, inchangée) :
mêmes briques (_beat_events, _s_sound, _burst), mêmes gains de site, même variabilité de contact
et modulation respiratoire. Différence : chaque battement a SA propre hémodynamique, simulée
battement par battement sur la séquence de conduction (physio.simulate_hemo_beats). B1 suit le
dP/dt et la fermeture mitrale de ce battement, B2 sa fermeture aortique, le souffle son débit
régurgitant, B3 son remplissage rapide. En FA ou avec des extrasystoles, l'intensité variable de B1
et le déficit de pouls deviennent audibles.

Constantes : modules/auscultation.json (comme le port Rust).
"""
import json
import os
import numpy as np
from scipy.signal import butter, sosfilt
import physio
import signals_pcg_v5 as V

_CFG_PATH = os.path.join(os.path.dirname(__file__), '..', '..', 'modules', 'auscultation.json')
CFG = json.load(open(_CFG_PATH, encoding='utf-8'))
SIGNAL_WARM_S = 5.0        # chauffe avant la fenêtre des signaux (comme signals::SIGNAL_WARM_S)


def signal_beats(p, cond, period0, warm, dur, end_margin):
    """Battements du signal avec leur hémodynamique (temps relatif au début de chaque battement),
    ramenés à l'origine du signal ; renvoie aussi la POG moyenne."""
    r = physio.simulate_hemo_beats(p, cond, warm + dur + 1.0, warm, period0)
    t = r['t']
    out = []
    for b in r['beats']:
        if b['t'] >= warm + dur - end_margin:
            break
        m = (t >= b['t']) & (t < b['t'] + b['rr'])
        if m.sum() < 3:
            continue
        out.append(dict(t=b['t'] - warm, h=dict(t=t[m] - b['t'], Q_ao=r['Q_ao'][m], Q_mi=r['Q_mi'][m],
                                                P_lv=r['P_lv'][m], P_ao=r['P_ao'][m], P_pv=r['P_pv'][m],
                                                T=b['rr'], ts=b['ts'], la_mean=r['la_mean'])))
    return out, r['la_mean']


def make_pcg_beats(beats, la_mean, rng, fs=4000, dur=8.0, resp_hz=0.35, site='base'):
    """Synthèse battement par battement. Ordre des tirages : bruit du souffle (N normales), par
    battement 2 lognormales, 3 + 3 + 3 phases, [1 uniforme si souffle], bruit de fond (N normales)."""
    c = CFG
    st = c['sites'][site]
    N = int(dur * fs)
    sig = np.zeros(N)
    q_reg = lambda h: np.maximum(-h['Q_mi'], 0.0) + np.maximum(-h['Q_ao'], 0.0)
    peak_reg = float(max(q_reg(b['h']).max() for b in beats))
    murmur_on = peak_reg > c['murmur']['flow_threshold']
    sos_m = butter(c['murmur']['filter_order'], c['murmur']['band_hz'], btype='band', fs=fs, output='sos')
    noise = sosfilt(sos_m, rng.standard_normal(N)); noise /= noise.std()
    k_m = st['murmur_gain'] * c['murmur']['amp']
    s1 = s2 = s3 = tmc = tac = 0.0

    def put(t0, x):
        i0 = int(t0 * fs); seg = sig[i0:i0 + len(x)]
        sig[i0:i0 + len(seg)] += x[:len(seg)]

    for bt in beats:
        b, h = bt['t'], bt['h']
        dt = h['t'][1] - h['t'][0]
        ev = V._beat_events(h, dt)
        t_mc = ev['i_mc'] * dt
        t_ac = max(ev['i_ac'] * dt, t_mc + c['events']['guard_systole_frac'] * h['ts'])
        s1_amp = st['s1_gain'] * c['s1']['base_amp'] * min(c['s1']['cap'], ev['dpdt_max'] / c['s1']['dpdt_ref'])
        s2_amp = st['s2_gain'] * c['s2']['base_amp'] * min(c['s2']['cap'], ev['dp_close'] / c['s2']['dp_ref'])
        s3_amp = (c['s3']['amp'] * max(0.0, (ev['q_early'] - c['s3']['q_ref']) / c['s3']['q_ref'])
                  if la_mean > c['s3']['la_threshold_mmhg'] else 0.0)
        s1 += s1_amp; s2 += s2_amp; s3 += s3_amp; tmc += t_mc; tac += t_ac
        resp = 1.0 + c['variability']['resp_modulation'] * np.sin(2 * np.pi * resp_hz * b)
        g1 = resp * rng.lognormal(0, c['variability']['gain_lognormal_sd'])
        g2 = resp * rng.lognormal(0, c['variability']['gain_lognormal_sd'])
        put(b + t_mc, V._s_sound(int(c['s1']['duration_s'] * fs), fs, c['s1']['f0_hz'], c['s1']['tau_s'], s1_amp * g1, rng))
        insp = max(0.0, np.sin(2 * np.pi * resp_hz * b))
        split = c['s2']['split_base_s'] + c['s2']['split_insp_s'] * insp
        n2 = int(c['s2']['duration_s'] * fs)
        put(b + t_ac, V._s_sound(n2, fs, c['s2']['f0_aortic_hz'], c['s2']['tau_s'],
                                 s2_amp * g2 * (1.0 - c['s2']['aortic_insp_drop'] * insp), rng))
        put(b + t_ac + split, V._s_sound(n2, fs, c['s2']['f0_pulmonic_hz'], c['s2']['tau_s'],
                                         s2_amp * g2 * (c['s2']['pulmonic_base'] + c['s2']['pulmonic_insp'] * insp), rng))
        if s3_amp > 0:
            put(b + t_ac + c['s3']['delay_s'], V._burst(int(c['s3']['duration_s'] * fs), fs, c['s3']['f0_hz'],
                                                        c['s3']['tau_s'], s3_amp * g1))
        if murmur_on:
            env = np.interp(np.arange(int(h['T'] * fs)) / fs, h['t'], q_reg(h), right=0.0)
            jitter = rng.uniform(*c['murmur']['jitter'])
            env = np.sqrt(np.maximum(env, 0.0)) * k_m * resp * jitter
            i0 = int(b * fs); seg = sig[i0:i0 + len(env)]
            sig[i0:i0 + len(seg)] += env[:len(seg)] * noise[i0:i0 + len(seg)]
    bgc = c['background']
    sos_b = butter(bgc['filter_order'], bgc['band_hz'], btype='band', fs=fs, output='sos')
    bg = sosfilt(sos_b, rng.standard_normal(N)); bg /= bg.std()
    swell = bgc['swell_base'] + bgc['swell_amp'] * np.sin(2 * np.pi * resp_hz * np.arange(N) / fs + bgc['swell_phase']) ** 2
    sig += bgc['amp'] * bg * swell
    thr = c['grade']['thresholds_ml_s']
    grade = min(int(np.searchsorted(thr, peak_reg)) + 1, int(c['grade']['max'])) if peak_reg > c['grade']['min_flow_ml_s'] else 0
    nb = len(beats)
    back = lambda k: max(np.maximum(-b['h'][k], 0).max() for b in beats)
    feats = dict(s1=s1 / nb, s2=s2 / nb, s3_amp=s3 / nb, murmur_peak_flow=peak_reg, grade=grade,
                 murmur='holosystolic' if back('Q_mi') > c['murmur']['flow_threshold'] else
                 ('diastolic' if back('Q_ao') > c['murmur']['flow_threshold'] else 'none'),
                 t_mc=tmc / nb, t_ac=tac / nb)
    sig = sig / max(np.abs(sig).max(), 1e-9)
    return np.arange(N) / fs, sig, feats
