//! Système nerveux autonome et variabilité de la fréquence cardiaque
//! (miroir de `reference/python/autonomic.py`, MODEL_SPEC §7).
//!
//! Simulation à 20 Hz :
//! - porte respiratoire (profil de volume pulmonaire `ρ`) qui inhibe le tonus vagal central à
//!   l'inspiration (arythmie sinusale respiratoire, RSA), profondeur ∝ volume courant ;
//! - voie vagale rapide (τ = 0,25 s) et voie sympathique lente (τ = 3 s) avec retard baroréflexe
//!   de 2,5 s et bruit central (ondes de Mayer vers 0,1 Hz) ; dérive vagale lente (τ = 12 s) ;
//! - baroréflexe sigmoïde autour de la consigne de PAM ;
//! - nœud sinusal par intégration de phase (IPFM) → instants des ondes R → VFC.
//!
//! Les gains sont plausibles mais non calés individuellement sur des données canines
//! (calage global sur les moyennes de Bogucki & Noszczyk-Nowak 2015, voir PROVENANCE.md).
//! **Stochastique** : la validation est statistique (moyenne sur ≥ 20 graines).

use crate::engine::{hr_factor, Evaluation};
use crate::math;
use crate::params::Params;
use crate::rng::NoiseSource;
use std::collections::VecDeque;
use std::f64::consts::PI;

/// Entrées scalaires de la simulation autonome (issues de l'évaluation en régime permanent).
#[derive(Debug, Clone, PartialEq)]
pub struct AutonomicInputs {
    /// FC de régime permanent (/min).
    pub hr: f64,
    /// Tonus sympathique effectif moyen.
    pub s_eff: f64,
    /// Tonus vagal effectif moyen.
    pub v_eff: f64,
    /// PAM moyenne (mmHg).
    pub map: f64,
    /// Fréquence respiratoire (/min).
    pub rr: f64,
    /// Ventilation alvéolaire (L/min).
    pub va: f64,
    /// Paramètres utilisés : `bw`, `va0`, `map_set`, `baro_sens`, `rsa_depth`, `vag_noise`,
    /// `sym_noise`, `a_s`, `a_v`.
    pub params: Params,
}

impl From<&Evaluation> for AutonomicInputs {
    fn from(e: &Evaluation) -> Self {
        Self {
            hr: e.params.hr,
            s_eff: e.autonomic.s_eff,
            v_eff: e.autonomic.v_eff,
            map: e.hemo.summary.map,
            rr: e.metabolic.art.rr,
            va: e.metabolic.art.va,
            params: e.params.clone(),
        }
    }
}

/// Options de la simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutonomicOptions {
    /// Durée enregistrée (s).
    pub duration_s: f64,
    /// Durée de mise en régime, non enregistrée (s).
    pub warmup_s: f64,
    /// Fréquence d'échantillonnage (Hz).
    pub fs: f64,
}

impl Default for AutonomicOptions {
    fn default() -> Self {
        Self {
            duration_s: 120.0,
            warmup_s: 20.0,
            fs: 20.0,
        }
    }
}

/// Indices de VFC (bandes adaptées au chien : LF 0,04–0,15 Hz, HF 0,15–1 Hz).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HrvMetrics {
    /// FC moyenne (/min).
    pub fc_moy: f64,
    /// SDNN (ms).
    pub sdnn_ms: f64,
    /// RMSSD (ms).
    pub rmssd_ms: f64,
    /// Puissance LF (ms²).
    pub lf: f64,
    /// Puissance HF (ms²).
    pub hf: f64,
    /// Rapport LF/HF (NaN si HF nul).
    pub lf_hf: f64,
    /// Amplitude moyenne de la RSA (battements/min) par cycle respiratoire.
    pub rsa_bpm: f64,
    /// Fréquence du pic spectral (Hz).
    pub f_peak: f64,
}

/// Séries temporelles de la simulation (après mise en régime).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AutonomicSeries {
    /// Temps (s).
    pub t: Vec<f64>,
    /// Tonus sympathique.
    pub s: Vec<f64>,
    /// Tonus vagal.
    pub v: Vec<f64>,
    /// FC instantanée du nœud sinusal (/min).
    pub hr: Vec<f64>,
    /// Volume pulmonaire normalisé.
    pub rho: Vec<f64>,
    /// PAM instantanée (mmHg).
    pub map: Vec<f64>,
    /// Instants des ondes R (s).
    pub beats: Vec<f64>,
    /// Fréquence respiratoire (Hz).
    pub fr: f64,
    /// Indices de VFC (absents si moins de 20 intervalles RR).
    pub hrv: Option<HrvMetrics>,
}

/// Profil de volume pulmonaire normalisé (0–1) selon la phase respiratoire
/// (inspiration sur 40 % du cycle).
fn rho(ph: f64) -> f64 {
    const F_INSP: f64 = 0.4;
    let ph = ph.rem_euclid(1.0);
    if ph < F_INSP {
        0.5 * (1.0 - math::cos(PI * ph / F_INSP))
    } else {
        0.5 * (1.0 + math::cos(PI * (ph - F_INSP) / (1.0 - F_INSP)))
    }
}

/// Simule le système autonome ; `noise` fournit d'abord un uniforme (phase respiratoire
/// initiale), puis trois normales par pas (FR, dérive vagale, bruit sympathique).
pub fn simulate_autonomic(
    inp: &AutonomicInputs,
    opts: &AutonomicOptions,
    noise: &mut dyn NoiseSource,
) -> AutonomicSeries {
    let p = &inp.params;
    let (hr_bar, s_bar, v_bar, mapbar) = (inp.hr, inp.s_eff, inp.v_eff, inp.map);
    let fr = inp.rr / 60.0;
    // profondeur ventilatoire relative
    let vt_n = (inp.va / inp.rr) / (p.va0 * p.bw / 20.0);
    let fs = opts.fs;
    let dt = 1.0 / fs;
    let n = ((opts.duration_s + opts.warmup_s) * fs) as usize;
    let (tau_v, tau_s) = (0.25, 3.0);
    let delay_s = (2.5 * fs) as usize;
    let delay_v = (0.3 * fs) as usize;
    let w_b = 10.0; // largeur de la sigmoïde barorécepteur (mmHg)
    let fb = |m: f64| 1.0 / (1.0 + math::exp(-(m - p.map_set) / w_b));
    let fb_bar = fb(mapbar);
    let g_s = 1.4 * p.baro_sens;
    let g_v = 1.0 * p.baro_sens;
    let g_vas = 25.0; // mmHg par unité de tonus sympathique (vasomoteur)
    let a_ip = 2.0 * vt_n; // variation de PA liée à la respiration (mmHg)
    let m = p.rsa_depth * math::pow(vt_n, 0.7).clamp(0.3, 1.5);
    let sqdt = dt.sqrt();
    let hr_ref = hr_factor(s_bar, v_bar, p);

    let mut s_e = s_bar;
    let mut v_e = v_bar;
    let mut mapbuf: VecDeque<f64> = std::iter::repeat(mapbar)
        .take(delay_s.max(delay_v) + 2)
        .collect();
    let mut phi_r = noise.uniform01();
    let mut phi_n = 0.0;
    let (mut xi, mut xv) = (0.0, 0.0);
    let warm = opts.warmup_s;
    let mut out = AutonomicSeries {
        fr,
        ..Default::default()
    };
    let mut beats = Vec::new();
    for i in 0..n {
        // variabilité lente de la fréquence respiratoire
        xi += (-xi * dt / 8.0) + 0.05 * sqdt * noise.standard_normal();
        phi_r += fr * (1.0 + xi) * dt;
        let r = rho(phi_r);
        let r_lag = rho(phi_r - 0.25);
        let mp = mapbar - a_ip * (r_lag - 0.5) + g_vas * (s_e - s_bar);
        mapbuf.pop_back();
        mapbuf.push_front(mp);
        let df_s = fb(mapbuf[delay_s]) - fb_bar;
        let df_v = fb(mapbuf[delay_v]) - fb_bar;
        // cibles efférentes
        let s_tar = s_bar - g_s * df_s;
        xv += -xv * dt / 12.0 + p.vag_noise * sqdt * noise.standard_normal();
        // modulation respiratoire centrée : conserve le tonus vagal moyen
        let v_tar = v_bar * (1.0 - m * (r - 0.5)) + g_v * df_v + xv;
        s_e += dt / tau_s * (s_tar - s_e) + p.sym_noise * sqdt * noise.standard_normal();
        v_e += dt / tau_v * (v_tar - v_e);
        let s_c = s_e.clamp(0.0, 1.2);
        let v_c = v_e.clamp(0.0, 1.0);
        let hr = (hr_bar * hr_factor(s_c, v_c, p) / hr_ref).clamp(20.0, 300.0);
        let ph_prev = phi_n;
        phi_n += hr / 60.0 * dt;
        let t = i as f64 * dt;
        if phi_n >= 1.0 {
            let frac = (1.0 - ph_prev) / (phi_n - ph_prev);
            beats.push(t + frac * dt);
            phi_n -= 1.0;
        }
        if t >= warm {
            out.t.push(t - warm);
            out.s.push(s_c);
            out.v.push(v_c);
            out.hr.push(hr);
            out.rho.push(r);
            out.map.push(mp);
        }
    }
    out.beats = beats
        .into_iter()
        .filter(|b| *b >= warm)
        .map(|b| b - warm)
        .collect();
    out.hrv = hrv_metrics(&out.beats, fr);
    out
}

/// `np.arange(start, stop, step)` (longueur et valeurs comme NumPy).
pub(crate) fn arange(start: f64, stop: f64, step: f64) -> Vec<f64> {
    let len = ((stop - start) / step).ceil();
    if len <= 0.0 || !len.is_finite() {
        return Vec::new();
    }
    let delta = (start + step) - start;
    (0..len as usize)
        .map(|i| start + i as f64 * delta)
        .collect()
}

/// `np.interp(x, xp, fp)` (xp croissant, valeurs extrêmes prolongées).
pub(crate) fn interp(x: &[f64], xp: &[f64], fp: &[f64]) -> Vec<f64> {
    let n = xp.len();
    let mut j = 0;
    x.iter()
        .map(|&v| {
            if v <= xp[0] {
                return fp[0];
            }
            if v >= xp[n - 1] {
                return fp[n - 1];
            }
            if v < xp[j] {
                j = 0;
            }
            while xp[j + 1] <= v {
                j += 1;
            }
            let slope = (fp[j + 1] - fp[j]) / (xp[j + 1] - xp[j]);
            slope * (v - xp[j]) + fp[j]
        })
        .collect()
}

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}

/// Densité spectrale de puissance par la méthode de Welch, comme `scipy.signal.welch` avec
/// fenêtre de Hann périodique, recouvrement de 50 %, tendance linéaire retirée par segment,
/// `scaling='density'`, spectre unilatéral. TFD directe (segments ≤ 256 points).
pub fn welch(x: &[f64], fs: f64, nperseg: usize) -> (Vec<f64>, Vec<f64>) {
    let nseg_len = nperseg;
    let noverlap = nseg_len / 2;
    let step = nseg_len - noverlap;
    let nfreq = nseg_len / 2 + 1;
    let freqs: Vec<f64> = (0..nfreq)
        .map(|k| k as f64 * fs / nseg_len as f64)
        .collect();
    if x.len() < nseg_len || nseg_len == 0 {
        return (freqs, vec![0.0; nfreq]);
    }
    let nseg = (x.len() - noverlap) / step;
    let win: Vec<f64> = (0..nseg_len)
        .map(|i| 0.5 - 0.5 * math::cos(2.0 * PI * i as f64 / nseg_len as f64))
        .collect();
    let scale = 1.0 / (fs * win.iter().map(|w| w * w).sum::<f64>());
    // Table de rotations exp(−2iπ m/N).
    let (tc, ts): (Vec<f64>, Vec<f64>) = (0..nseg_len)
        .map(|m| {
            let a = 2.0 * PI * m as f64 / nseg_len as f64;
            (math::cos(a), math::sin(a))
        })
        .unzip();
    let mut acc = vec![0.0; nfreq];
    // Régression linéaire sur l'indice (scipy.signal.detrend, type='linear').
    let nf = nseg_len as f64;
    let im = (nf - 1.0) / 2.0;
    let sxx: f64 = (0..nseg_len)
        .map(|i| (i as f64 - im) * (i as f64 - im))
        .sum();
    for s in 0..nseg {
        let seg = &x[s * step..s * step + nseg_len];
        let my = mean(seg);
        let sxy: f64 = seg
            .iter()
            .enumerate()
            .map(|(i, y)| (i as f64 - im) * (y - my))
            .sum();
        let slope = sxy / sxx;
        let w: Vec<f64> = seg
            .iter()
            .enumerate()
            .map(|(i, y)| (y - (my + slope * (i as f64 - im))) * win[i])
            .collect();
        for (k, a) in acc.iter_mut().enumerate() {
            let (mut re, mut imag) = (0.0, 0.0);
            for (i, v) in w.iter().enumerate() {
                let idx = (k * i) % nseg_len;
                re += v * tc[idx];
                imag -= v * ts[idx];
            }
            let mut pk = (re * re + imag * imag) * scale;
            let nyquist = nseg_len % 2 == 0 && k == nfreq - 1;
            if k > 0 && !nyquist {
                pk *= 2.0;
            }
            *a += pk;
        }
    }
    for a in acc.iter_mut() {
        *a /= nseg as f64;
    }
    (freqs, acc)
}

/// Indices de VFC à partir des instants des ondes R (s) et de la fréquence respiratoire (Hz).
/// `None` si moins de 20 intervalles RR.
pub fn hrv_metrics(beats: &[f64], fr: f64) -> Option<HrvMetrics> {
    if beats.len() < 21 {
        return None;
    }
    let rr: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    let hr_i: Vec<f64> = rr.iter().map(|r| 60.0 / r).collect();
    let tt = &beats[1..];
    // tachogramme rééchantillonné à 4 Hz
    let t4 = arange(tt[0], tt[tt.len() - 1], 0.25);
    let rr_ms: Vec<f64> = rr.iter().map(|r| r * 1000.0).collect();
    let x = interp(&t4, tt, &rr_ms);
    let mx = mean(&x);
    let xc: Vec<f64> = x.iter().map(|v| v - mx).collect();
    let (f, pw) = welch(&xc, 4.0, xc.len().min(256));
    let band = |lo: f64, hi: f64| {
        let idx: Vec<usize> = (0..f.len()).filter(|&i| f[i] >= lo && f[i] < hi).collect();
        idx.windows(2)
            .map(|w| (f[w[1]] - f[w[0]]) * (pw[w[1]] + pw[w[0]]) / 2.0)
            .sum::<f64>()
    };
    let (lf, hf) = (band(0.04, 0.15), band(0.15, 1.0));
    // amplitude de RSA : max − min de la FC instantanée par cycle respiratoire
    let period = 1.0 / fr;
    let last = tt[tt.len() - 1];
    let mut rsa = Vec::new();
    let mut k = 0.0;
    while (k + 1.0) * period < last {
        let vals: Vec<f64> = tt
            .iter()
            .zip(&hr_i)
            .filter(|(t, _)| **t >= k * period && **t < (k + 1.0) * period)
            .map(|(_, h)| *h)
            .collect();
        if vals.len() >= 3 {
            let mx = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mn = vals.iter().copied().fold(f64::INFINITY, f64::min);
            rsa.push(mx - mn);
        }
        k += 1.0;
    }
    let mrr = mean(&rr);
    let sd = (rr.iter().map(|r| (r - mrr) * (r - mrr)).sum::<f64>() / rr.len() as f64).sqrt();
    let d2: Vec<f64> = rr
        .windows(2)
        .map(|w| (w[1] - w[0]) * (w[1] - w[0]))
        .collect();
    let mut i_peak = 0;
    for (i, v) in pw.iter().enumerate() {
        if *v > pw[i_peak] {
            i_peak = i;
        }
    }
    Some(HrvMetrics {
        fc_moy: 60.0 / mrr,
        sdnn_ms: sd * 1000.0,
        rmssd_ms: mean(&d2).sqrt() * 1000.0,
        lf,
        hf,
        lf_hf: if hf > 0.0 { lf / hf } else { f64::NAN },
        rsa_bpm: if rsa.is_empty() { 0.0 } else { mean(&rsa) },
        f_peak: f.get(i_peak).copied().unwrap_or(0.0),
    })
}
