"""Système nerveux autonome : simulation temporelle du tonus sympathique et vagal, du baroréflexe,
de l'arythmie sinusale respiratoire (RSA) et des ondes de Mayer ; nœud sinusal par intégration
de phase (IPFM) -> instants des ondes R -> variabilité de la fréquence cardiaque (HRV).

Mécanismes inclus
- Tonus moyen (s_bar, v_bar) fourni par le moteur (baroréflexe en régime permanent, médicaments, maladie).
- RSA centrale : la porte respiratoire inhibe le tonus vagal central pendant l'inspiration ;
  profondeur proportionnelle au tonus vagal et au volume courant. La voie vagale est un filtre
  passe-bas rapide (tau_v) : l'amplitude de RSA chute aux fréquences respiratoires élevées (halètement).
- Voie sympathique lente (tau_s) avec retard baroréflexe (~2,5 s) : ondes de Mayer vers 0,1 Hz.
- Baroréflexe vagal rapide : relaie les oscillations de PA (respiratoires) sur la FC.
Les valeurs de gains sont plausibles mais à calibrer sur des données canines.
"""
import numpy as np
from scipy.signal import welch
from physio import hr_factor


def _rho(ph, f_insp=0.4):
    """Profil de volume pulmonaire normalisé (0..1) en fonction de la phase respiratoire."""
    ph = ph % 1.0
    return np.where(ph < f_insp, 0.5 * (1 - np.cos(np.pi * ph / f_insp)),
                    0.5 * (1 + np.cos(np.pi * (ph - f_insp) / (1 - f_insp))))


def simulate_autonomic(r, dur=120.0, warm=20.0, fs=20.0, seed=0):
    rng = np.random.default_rng(seed)
    p = r['p']
    a = r['autonomic']
    hr_bar, s_bar, v_bar = p.get('hr_sinus', p['hr']), a['s_eff'], a['v_eff']   # nœud sinusal
    mapbar = r['hemo']['map']
    rr_rate = r['art']['rr']
    fr = rr_rate / 60.0
    vt_n = (r['art']['va'] / rr_rate) / (p['va0'] * p['bw'] / 20.0)        # profondeur ventilatoire relative
    dt = 1.0 / fs
    n = int((dur + warm) * fs)
    # --- constantes physiologiques (à calibrer)
    tau_v, tau_s = 0.25, 3.0
    delay_s, delay_v = int(2.5 * fs), int(0.3 * fs)
    w_b = 10.0                                   # largeur sigmoïde barorécepteur (mmHg)
    fb = lambda m: 1.0 / (1.0 + np.exp(-(m - p['map_set']) / w_b))
    fb_bar = fb(mapbar)
    g_s = 1.4 * p['baro_sens']
    g_v = 1.0 * p['baro_sens']
    g_vas = 25.0                                 # mmHg par unité de tonus sympathique (vasomoteur)
    a_ip = 2.0 * vt_n                            # variation de PA liée à la respiration (mmHg)
    m = p['rsa_depth'] * np.clip(vt_n ** 0.7, 0.3, 1.5)
    # --- états
    s_e, v_e = s_bar, v_bar
    mapbuf = np.full(max(delay_s, delay_v) + 2, mapbar)
    phi_r, phi_n = rng.random(), 0.0
    xi = 0.0
    xv = 0.0
    out = dict(t=np.zeros(n), s=np.zeros(n), v=np.zeros(n), hr=np.zeros(n), rho=np.zeros(n), map=np.zeros(n))
    beats = []
    ph_prev = 0.0
    for i in range(n):
        xi += (-xi * dt / 8.0) + 0.05 * np.sqrt(dt) * rng.standard_normal()          # variabilité lente de la FR
        phi_r += fr * (1 + xi) * dt
        rho = float(_rho(phi_r))
        rho_lag = float(_rho(phi_r - 0.25))
        mp = mapbar - a_ip * (rho_lag - 0.5) + g_vas * (s_e - s_bar)
        mapbuf = np.roll(mapbuf, 1); mapbuf[0] = mp
        df_s = fb(mapbuf[delay_s]) - fb_bar
        df_v = fb(mapbuf[delay_v]) - fb_bar
        # cibles efférentes
        s_tar = s_bar - g_s * df_s
        xv += -xv * dt / 12.0 + p.get('vag_noise', 0.0) * np.sqrt(dt) * rng.standard_normal()   # dérive vagale lente (LF)
        v_tar = v_bar * (1 - m * (rho - 0.5)) + g_v * df_v + xv      # modulation centrée : conserve le tonus moyen
        s_e += dt / tau_s * (s_tar - s_e) + p.get('sym_noise', 0.06) * np.sqrt(dt) * rng.standard_normal()   # bruit central (ondes de Mayer)
        v_e += dt / tau_v * (v_tar - v_e)
        s_c, v_c = float(np.clip(s_e, 0, 1.2)), float(np.clip(v_e, 0, 1.0))
        hr = hr_bar * hr_factor(s_c, v_c, p) / hr_factor(s_bar, v_bar, p)
        hr = float(np.clip(hr, 20, 300))
        ph_prev = phi_n
        phi_n += hr / 60.0 * dt
        if phi_n >= 1.0:
            frac = (1.0 - ph_prev) / (phi_n - ph_prev)
            beats.append(i * dt + frac * dt)
            phi_n -= 1.0
        out['t'][i], out['s'][i], out['v'][i], out['hr'][i], out['rho'][i], out['map'][i] = i * dt, s_c, v_c, hr, rho, mp
    beats = np.array(beats)
    keep = out['t'] >= warm
    for k in out:
        out[k] = out[k][keep] - (warm if k == 't' else 0)
    beats = beats[beats >= warm] - warm
    out['beats'] = beats
    out['fr'] = fr
    out['hrv'] = hrv_metrics(beats, fr)
    return out


def hrv_metrics(beats, fr):
    rr = np.diff(beats)
    if len(rr) < 20:
        return {}
    hr_i = 60.0 / rr
    # tachogramme rééchantillonné à 4 Hz, bandes adaptées au chien (LF 0,04-0,15 ; HF 0,15-1,0 Hz)
    tt = beats[1:]
    t4 = np.arange(tt[0], tt[-1], 0.25)
    x = np.interp(t4, tt, rr * 1000.0)
    f, P = welch(x - x.mean(), fs=4.0, nperseg=min(len(x), 256), detrend='linear')
    band = lambda lo, hi: float(np.trapezoid(P[(f >= lo) & (f < hi)], f[(f >= lo) & (f < hi)]))
    lf, hf = band(0.04, 0.15), band(0.15, 1.0)
    # amplitude de RSA : max-min de la FC instantanée par cycle respiratoire
    T = 1.0 / fr
    rsa = []
    k = 0
    while (k + 1) * T < tt[-1]:
        m = (tt >= k * T) & (tt < (k + 1) * T)
        if m.sum() >= 3:
            rsa.append(hr_i[m].max() - hr_i[m].min())
        k += 1
    return dict(fc_moy=float(60.0 / rr.mean()), sdnn_ms=float(rr.std() * 1000), rmssd_ms=float(np.sqrt(np.mean(np.diff(rr) ** 2)) * 1000),
                lf=lf, hf=hf, lf_hf=lf / hf if hf > 0 else float('nan'), rsa_bpm=float(np.mean(rsa)) if rsa else 0.0,
                f_peak=float(f[np.argmax(P)]) if len(P) else 0.0)


def spectrum(beats):
    """Spectre de puissance du tachogramme (intervalles RR en ms, rééchantillonné à 4 Hz)."""
    rr = np.diff(beats)
    tt = beats[1:]
    t4 = np.arange(tt[0], tt[-1], 0.25)
    x = np.interp(t4, tt, rr * 1000.0)
    return welch(x - x.mean(), fs=4.0, nperseg=min(len(x), 256), detrend='linear')
