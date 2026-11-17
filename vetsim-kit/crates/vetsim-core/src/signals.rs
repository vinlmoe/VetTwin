//! Signaux synthétiques : ECG (dérivation II) et phonocardiogramme
//! (miroir de `reference/python/signals.py`, MODEL_SPEC §8).
//!
//! - ECG : modèle phénoménologique (somme de gaussiennes P, Q, R, S, T) piloté par la FC et la
//!   kaliémie ; battements aux instants R du modèle autonome.
//! - PCG : bruits déclenchés par les événements valvulaires **simulés** ; le souffle est modulé
//!   par le débit régurgitant simulé (aucune règle de maladie codée en dur).
//!
//! Signaux pédagogiques : la morphologie n'est pas validée en sortie (PROVENANCE.md).

use crate::autonomic::{arange, interp, simulate_autonomic, AutonomicOptions, AutonomicSeries};
use crate::engine::Evaluation;
use crate::hemo::HemoResult;
use crate::math;
use crate::params::Params;
use crate::rng::NoiseSource;
use crate::rng::SimRng;
use std::f64::consts::PI;

// ------------------------------------------------------------------------------------- ECG

/// Intervalles (s) et amplitudes (mV) effectifs de l'ECG.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EcgFeatures {
    /// Intervalle PR.
    pub pr: f64,
    /// Durée du QRS.
    pub qrs: f64,
    /// Intervalle QT (corrigé de la fréquence par Fridericia dans [`make_ecg`]).
    pub qt: f64,
    /// Amplitude de P (0 si l'onde P a disparu).
    pub p_amp: f64,
    /// Amplitude de T.
    pub t_amp: f64,
    /// Amplitude de R.
    pub r_amp: f64,
    /// Largeur de l'onde T.
    pub t_w: f64,
}

/// Effets de la kaliémie (paliers continus) :
/// - hyperkaliémie > 5,5 mmol/L : T ample, pointue et étroite ; au-delà de 6, P s'aplatit et PR
///   s'allonge ; au-delà de 6,5, le QRS s'élargit ; au-delà de 7,8, P disparaît (rythme
///   sinoventriculaire, stase atriale) ;
/// - hypokaliémie < 3,2 mmol/L : T aplatie, QT allongé.
pub fn ecg_features(p: &Params) -> EcgFeatures {
    let k = p.k;
    let (mut pr, mut qrs, mut qt) = (p.pr, p.qrs, p.qt);
    let (mut p_amp, mut t_amp, r_amp) = (p.p_amp, p.t_amp, p.r_amp);
    let mut t_w = 0.045;
    let mut p_visible = true;
    if k > 5.5 {
        let d = k - 5.5;
        t_amp *= 1.0 + 0.45 * d;
        t_w = f64::max(0.025, 0.045 - 0.006 * d);
        p_amp *= f64::max(0.0, 1.0 - 0.25 * f64::max(0.0, k - 6.0));
        pr *= 1.0 + 0.07 * f64::max(0.0, k - 6.0);
        qrs *= 1.0 + 0.12 * f64::max(0.0, k - 6.5);
        if k > 7.8 {
            p_visible = false;
        }
    } else if k < 3.2 {
        let d = 3.2 - k;
        t_amp *= f64::max(0.2, 1.0 - 0.3 * d);
        qt *= 1.0 + 0.08 * d;
    }
    EcgFeatures {
        pr,
        qrs,
        qt,
        p_amp: if p_visible { p_amp } else { 0.0 },
        t_amp,
        r_amp,
        t_w,
    }
}

/// Tracé ECG.
#[derive(Debug, Clone, PartialEq)]
pub struct Ecg {
    /// Temps (s).
    pub t: Vec<f64>,
    /// Tension (mV).
    pub v: Vec<f64>,
    /// Caractéristiques utilisées (QT corrigé).
    pub features: EcgFeatures,
    /// Instants R utilisés (s).
    pub r_times: Vec<f64>,
}

/// ECG de `duration_s` secondes à `fs` Hz. Battements aux instants `r_times` ; à défaut,
/// repli à RSA sinusoïdale simple (fréquence respiratoire `rr_resp` /min). `noise` fournit une
/// normale par échantillon (bruit de 10 µV) ; s'y ajoute une dérive de ligne de base à 0,25 Hz.
pub fn make_ecg(
    p: &Params,
    hr: f64,
    rr_resp: f64,
    duration_s: f64,
    fs: f64,
    r_times: Option<&[f64]>,
    noise: &mut dyn NoiseSource,
) -> Ecg {
    let mut f = ecg_features(p);
    f.qt *= math::pow(95.0 / hr.max(30.0), 1.0 / 3.0); // correction de fréquence (Fridericia)
    let t = arange(0.0, duration_s, 1.0 / fs);
    let mut sig = vec![0.0; t.len()];
    let t_r: Vec<f64> = match r_times {
        Some(r) => r.to_vec(),
        None => {
            let mut out = Vec::new();
            let mut tcur = 0.25;
            while tcur < duration_s + 0.5 {
                out.push(tcur);
                tcur += 60.0 / hr * (1.0 + 0.06 * math::sin(2.0 * PI * (rr_resp / 60.0) * tcur));
            }
            out
        }
    };
    let mut add = |c: f64, a: f64, w: f64| {
        for (s, ti) in sig.iter_mut().zip(&t) {
            let z = (ti - c) / w;
            *s += a * math::exp(-0.5 * (z * z));
        }
    };
    for &tr in &t_r {
        let qrs_w = f.qrs / 5.0;
        add(tr - f.pr + 0.5 * 0.03, f.p_amp, 0.022); // P
        add(tr - 0.55 * f.qrs, -0.12 * f.r_amp, qrs_w * 0.8); // Q
        add(tr, f.r_amp, qrs_w); // R
        add(tr + 0.55 * f.qrs, -0.20 * f.r_amp, qrs_w * 0.9); // S
        let t_center = tr - 0.5 * f.qrs + f.qt - 2.2 * f.t_w;
        add(t_center, f.t_amp, f.t_w); // T
    }
    for (s, ti) in sig.iter_mut().zip(&t) {
        *s += 0.01 * noise.standard_normal() + 0.02 * math::sin(2.0 * PI * 0.25 * ti);
    }
    Ecg {
        t,
        v: sig,
        features: f,
        r_times: t_r,
    }
}

// ------------------------------------------------------------------------------------- PCG

/// Fréquence d'échantillonnage du PCG (Hz) : celle de la référence (filtre conçu pour elle).
pub const PCG_FS: f64 = 4000.0;

/// Passe-bande de Butterworth d'ordre 3, 90–450 Hz à 4000 Hz, en sections du second ordre
/// `[b0, b1, b2, a0, a1, a2]`. Coefficients calculés par
/// `scipy.signal.butter(3, [90, 450], btype='band', fs=4000, output='sos')` (scipy 1.13),
/// figés ici ; la réponse impulsionnelle est vérifiée contre `scipy.signal.sosfilt`.
pub const PCG_BANDPASS_SOS: [[f64; 6]; 3] = [
    [
        0.01382007388379196,
        0.02764014776758392,
        0.01382007388379196,
        1.0,
        -1.4708536451469336,
        0.5497546521927701,
    ],
    [1.0, 0.0, -1.0, 1.0, -1.2821638529585362, 0.6407869645287507],
    [1.0, -2.0, 1.0, 1.0, -1.8800442844146525, 0.9010713886556404],
];

/// Filtrage en cascade de sections du second ordre (forme directe II transposée, comme
/// `scipy.signal.sosfilt`, conditions initiales nulles).
pub fn sosfilt(sos: &[[f64; 6]], x: &[f64]) -> Vec<f64> {
    let mut zi = vec![[0.0f64; 2]; sos.len()];
    x.iter()
        .map(|&v| {
            let mut cur = v;
            for (s, z) in sos.iter().zip(zi.iter_mut()) {
                let new = s[0] * cur + z[0];
                z[0] = s[1] * cur - s[4] * new + z[1];
                z[1] = s[2] * cur - s[5] * new;
                cur = new;
            }
            cur
        })
        .collect()
}

/// Séries hémodynamiques d'un battement nécessaires au PCG.
#[derive(Debug, Clone, PartialEq)]
pub struct PcgInputs {
    /// Temps (s, pas constant).
    pub t: Vec<f64>,
    /// Débit aortique (mL/s).
    pub q_ao: Vec<f64>,
    /// Débit mitral (mL/s).
    pub q_mi: Vec<f64>,
    /// Pression VG (mmHg).
    pub p_lv: Vec<f64>,
    /// Pression aortique (mmHg).
    pub p_ao: Vec<f64>,
    /// Pression OG (mmHg).
    pub p_pv: Vec<f64>,
    /// Période cardiaque (s).
    pub period: f64,
    /// POG moyenne (mmHg).
    pub la_mean: f64,
}

impl From<&HemoResult> for PcgInputs {
    fn from(h: &HemoResult) -> Self {
        let w = &h.waveforms;
        Self {
            t: w.t.clone(),
            q_ao: w.q_ao.clone(),
            q_mi: w.q_mi.clone(),
            p_lv: w.p_lv.clone(),
            p_ao: w.p_ao.clone(),
            p_pv: w.p_pv.clone(),
            period: h.timing.period,
            la_mean: h.summary.la_mean,
        }
    }
}

/// Type de souffle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Murmur {
    /// Pas de souffle.
    None,
    /// Souffle holosystolique apexien gauche (régurgitation mitrale).
    HolosystolicLeftApical,
    /// Souffle diastolique basal gauche (régurgitation aortique).
    DiastolicLeftBasilar,
}

impl Murmur {
    /// Libellé (identique à la référence).
    pub fn label_fr(self) -> &'static str {
        match self {
            Murmur::None => "aucun",
            Murmur::HolosystolicLeftApical => "holosystolique apexien gauche",
            Murmur::DiastolicLeftBasilar => "diastolique basal gauche",
        }
    }
}

/// Caractéristiques auscultatoires (valeurs non arrondies ; la référence arrondit S1 et S2 à
/// 0,01 et le débit à 0,1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PcgFeatures {
    /// Amplitude de B1.
    pub s1: f64,
    /// Amplitude de B2.
    pub s2: f64,
    /// Amplitude de B3 (0 si absent).
    pub s3_amp: f64,
    /// B3 audible.
    pub s3: bool,
    /// Débit régurgitant maximal (mL/s).
    pub murmur_peak_flow: f64,
    /// Type de souffle.
    pub murmur: Murmur,
    /// Grade (Levine, indicatif) 0–6 d'après le débit régurgitant : heuristique à calibrer.
    pub grade: u8,
}

/// Phonocardiogramme.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcg {
    /// Temps (s).
    pub t: Vec<f64>,
    /// Signal normalisé (max |x| = 1).
    pub v: Vec<f64>,
    /// Caractéristiques.
    pub features: PcgFeatures,
}

struct BeatEvents {
    i_mc: usize,
    i_ac: usize,
    dpdt_max: f64,
    q_early: f64,
    dp_close: f64,
}

/// Événements valvulaires : fermeture mitrale (P_VG dépasse P_OG), fermeture aortique (débit
/// aortique repasse ≤ 0 après son pic), dP/dt max du VG, remplissage rapide.
fn beat_events(h: &PcgInputs, dt: f64) -> BeatEvents {
    let n = h.t.len();
    let d: Vec<f64> = h.p_lv.iter().zip(&h.p_pv).map(|(a, b)| a - b).collect();
    let i_mc = (0..n.saturating_sub(1))
        .find(|&i| d[i] < 0.0 && d[i + 1] >= 0.0)
        .unwrap_or(0);
    let mut ipk = 0;
    for (i, v) in h.q_ao.iter().enumerate() {
        if *v > h.q_ao[ipk] {
            ipk = i;
        }
    }
    let i_ac = (ipk..n)
        .find(|&i| h.q_ao[i] <= 0.0)
        .unwrap_or_else(|| (ipk + 100).min(n - 1));
    // np.gradient : différences centrées, décentrées aux bords
    let mut dpdt_max = f64::NEG_INFINITY;
    for i in 0..n {
        let g = if i == 0 {
            (h.p_lv[1] - h.p_lv[0]) / dt
        } else if i == n - 1 {
            (h.p_lv[n - 1] - h.p_lv[n - 2]) / dt
        } else {
            (h.p_lv[i + 1] - h.p_lv[i - 1]) / (2.0 * dt)
        };
        dpdt_max = dpdt_max.max(g);
    }
    let q_early = if i_ac < n {
        h.q_mi[i_ac..]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        0.0
    };
    BeatEvents {
        i_mc,
        i_ac,
        dpdt_max,
        q_early,
        dp_close: h.p_ao[i_ac] - h.p_lv[i_ac] + h.p_ao[i_ac],
    }
}

fn burst(n: usize, fs: f64, f0: f64, tau: f64, amp: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let tt = i as f64 / fs;
            amp * math::exp(-tt / tau) * math::sin(2.0 * PI * f0 * tt)
        })
        .collect()
}

/// PCG de `n_beats` battements identiques à 4000 Hz. `noise` fournit une normale par
/// échantillon (bruit coloré 90–450 Hz du souffle).
pub fn make_pcg(h: &PcgInputs, n_beats: usize, noise: &mut dyn NoiseSource) -> Pcg {
    let fs = PCG_FS;
    let dt = h.t[1] - h.t[0];
    let ev = beat_events(h, dt);
    let nb = (h.period * fs).round_ties_even() as usize;
    let total = nb * n_beats;
    let mut sig = vec![0.0; total];
    // flux régurgitant (mitral systolique + aortique diastolique) et éjection
    let q_reg: Vec<f64> = h
        .q_mi
        .iter()
        .zip(&h.q_ao)
        .map(|(m, a)| (-m).max(0.0) + (-a).max(0.0))
        .collect();
    let q_ej: Vec<f64> = h.q_ao.iter().map(|a| a.max(0.0)).collect();
    let tt: Vec<f64> = (0..nb).map(|i| i as f64 / fs).collect();
    let q_reg_u = interp(&tt, &h.t, &q_reg);
    let q_ej_u = interp(&tt, &h.t, &q_ej);
    let white: Vec<f64> = (0..total).map(|_| noise.standard_normal()).collect();
    let colored = sosfilt(&PCG_BANDPASS_SOS, &white);
    let (t_mc, t_ac) = (ev.i_mc as f64 * dt, ev.i_ac as f64 * dt);
    let s1 = 0.5 * f64::min(2.0, ev.dpdt_max / 1800.0);
    let s2 = 0.4 * f64::min(2.0, ev.dp_close / 190.0);
    let s3_amp = if h.la_mean > 15.0 {
        0.35 * f64::max(0.0, (ev.q_early - 160.0) / 160.0)
    } else {
        0.0
    };
    let b1 = burst((0.09 * fs) as usize, fs, 70.0, 0.025, s1);
    let b2 = burst((0.08 * fs) as usize, fs, 110.0, 0.020, s2);
    let b3 = burst((0.09 * fs) as usize, fs, 45.0, 0.030, s3_amp);
    let env: Vec<f64> = q_reg_u
        .iter()
        .zip(&q_ej_u)
        .map(|(r, e)| {
            let q = e / 40.0;
            0.0035 * r + 0.00015 * (q * q) * 1.0
        })
        .collect();
    for b in 0..n_beats {
        let o = b * nb;
        let mut put = |t0: f64, x: &[f64]| {
            let i0 = o + (t0 * fs) as usize;
            for (j, v) in x.iter().enumerate() {
                if let Some(s) = sig.get_mut(i0 + j) {
                    *s += v;
                }
            }
        };
        put(t_mc, &b1);
        put(t_ac, &b2);
        if s3_amp > 0.0 {
            put(t_ac + 0.12, &b3);
        }
        for j in 0..nb {
            sig[o + j] += env[j] * colored[o + j];
        }
    }
    let peak_reg = q_reg.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    const THR: [f64; 5] = [10.0, 25.0, 50.0, 80.0, 120.0];
    let grade = if peak_reg > 3.0 {
        (THR.iter().filter(|t| **t < peak_reg).count() + 1).min(6) as u8
    } else {
        0
    };
    let max_mi = h.q_mi.iter().map(|m| (-m).max(0.0)).fold(0.0, f64::max);
    let max_ao = h.q_ao.iter().map(|a| (-a).max(0.0)).fold(0.0, f64::max);
    let murmur = if max_mi > 3.0 {
        Murmur::HolosystolicLeftApical
    } else if max_ao > 3.0 {
        Murmur::DiastolicLeftBasilar
    } else {
        Murmur::None
    };
    let norm = sig.iter().map(|x| x.abs()).fold(0.0, f64::max).max(1e-9);
    let v: Vec<f64> = sig.iter().map(|x| x / norm).collect();
    Pcg {
        t: (0..total).map(|i| i as f64 / fs).collect(),
        v,
        features: PcgFeatures {
            s1,
            s2,
            s3_amp,
            s3: s3_amp > 0.02,
            murmur_peak_flow: peak_reg,
            murmur,
            grade,
        },
    }
}

// ------------------------------------------------------------------------- patient complet

/// Flux aléatoires (ChaCha « stream id ») utilisés pour une même graine.
pub mod streams {
    /// Variabilité interindividuelle et tirages des maladies (flux par défaut).
    pub const PATIENT: u64 = 0;
    /// Bruit du système autonome.
    pub const AUTONOMIC: u64 = 1;
    /// Bruit de l'ECG.
    pub const ECG: u64 = 2;
    /// Bruit du PCG.
    pub const PCG: u64 = 3;
}

/// Options des signaux d'un patient.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalOptions {
    /// Durée de l'ECG (s) ; la simulation autonome dure au moins 120 s (comme la référence).
    pub duration_s: f64,
    /// Fréquence d'échantillonnage de l'ECG (Hz).
    pub ecg_fs: f64,
    /// Nombre de battements du PCG.
    pub pcg_beats: usize,
}

impl Default for SignalOptions {
    fn default() -> Self {
        Self {
            duration_s: 8.0,
            ecg_fs: 500.0,
            pcg_beats: 4,
        }
    }
}

/// Signaux d'un patient évalué.
#[derive(Debug, Clone, PartialEq)]
pub struct PatientSignals {
    /// Séries autonomes (instants R, VFC).
    pub autonomic: AutonomicSeries,
    /// ECG (battements aux instants R de l'autonome).
    pub ecg: Ecg,
    /// PCG (événements du dernier battement simulé).
    pub pcg: Pcg,
}

/// Signaux d'un patient (comme `Patient.state(signals=True)`) : simulation autonome, ECG sur
/// les instants R de `[0, duration_s)`, PCG. Reproductible : même graine, mêmes signaux.
pub fn patient_signals(ev: &Evaluation, opts: &SignalOptions, seed: u64) -> PatientSignals {
    let au_opts = AutonomicOptions {
        duration_s: opts.duration_s.max(120.0),
        ..AutonomicOptions::default()
    };
    let au = simulate_autonomic(
        &ev.into(),
        &au_opts,
        &mut SimRng::for_stream(seed, streams::AUTONOMIC),
    );
    let r_times: Vec<f64> = au
        .beats
        .iter()
        .copied()
        .filter(|b| *b < opts.duration_s)
        .collect();
    let ecg = make_ecg(
        &ev.params,
        ev.params.hr,
        ev.metabolic.art.rr,
        opts.duration_s,
        opts.ecg_fs,
        Some(&r_times),
        &mut SimRng::for_stream(seed, streams::ECG),
    );
    let pcg = make_pcg(
        &PcgInputs::from(&ev.hemo),
        opts.pcg_beats,
        &mut SimRng::for_stream(seed, streams::PCG),
    );
    PatientSignals {
        autonomic: au,
        ecg,
        pcg,
    }
}
