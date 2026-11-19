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
    /// Largeur de l'onde P.
    pub p_w: f64,
}

// Forme des ondes (miroir de `signals.py`, docs/VERIFICATION_ECG.md) : largeurs calées pour que
// les durées visibles (seuil 0,02 mV) soient celles des intervalles publiés.
/// Largeur de P (s) : 45 ms visibles à 0,2 mV.
pub const P_SIGMA: f64 = 0.0105;
/// Élargissement relatif de P par mmol/L de K au-delà de 6 (placeholder D).
pub const P_WIDEN_K: f64 = 0.25;
/// Largeur de R (fraction de `qrs`).
pub const R_SIGMA: f64 = 1.0 / 10.0;
/// Centre (avant R) de Q (fraction de `qrs`).
pub const Q_POS: f64 = 0.34;
/// Largeur de Q (fraction de `qrs`).
pub const Q_SIGMA: f64 = 1.0 / 14.0;
/// Centre (après R) de S (fraction de `qrs`).
pub const S_POS: f64 = 0.34;
/// Largeur de S (fraction de `qrs`).
pub const S_SIGMA: f64 = 1.0 / 13.0;
/// Largeur de T au repos (s) : segment ST isoélectrique (placeholder D).
pub const T_W0: f64 = 0.025;
/// Battement ventriculaire : largeur de R (fraction du QRS large).
pub const WIDE_R_SIGMA: f64 = 1.0 / 8.0;
/// Battement ventriculaire : centre de S (fraction du QRS large).
pub const WIDE_S_POS: f64 = 0.40;
/// Battement ventriculaire : largeur de S (fraction du QRS large).
pub const WIDE_S_SIGMA: f64 = 1.0 / 11.0;

/// Effets de la kaliémie (paliers continus) :
/// - hyperkaliémie > 5,5 mmol/L : T ample, pointue et étroite, QT raccourci (placeholder D) ;
///   au-delà de 6, P s'aplatit et PR s'allonge ; au-delà de 6,5, le QRS s'élargit. La
///   disparition de P (arrêt atrial) est décidée par la conduction (`rhythm`) ;
/// - hypokaliémie < 3,2 mmol/L : T aplatie, QT allongé.
pub fn ecg_features(p: &Params) -> EcgFeatures {
    let k = p.k;
    let (mut pr, mut qrs, mut qt) = (p.pr, p.qrs, p.qt);
    let (mut p_amp, mut t_amp, r_amp) = (p.p_amp, p.t_amp, p.r_amp);
    let mut t_w = T_W0;
    let mut p_w = P_SIGMA;
    if k > 5.5 {
        let d = k - 5.5;
        t_amp *= 1.0 + 0.45 * d;
        t_w = T_W0 * f64::max(0.025, 0.045 - 0.006 * d) / 0.045;
        p_w *= 1.0 + P_WIDEN_K * f64::max(0.0, k - 6.0);
        qt *= f64::max(0.8, 1.0 - 0.04 * d);
        p_amp *= f64::max(0.0, 1.0 - 0.25 * f64::max(0.0, k - 6.0));
        pr *= 1.0 + 0.07 * f64::max(0.0, k - 6.0);
        qrs *= 1.0 + 0.12 * f64::max(0.0, k - 6.5);
    } else if k < 3.2 {
        let d = 3.2 - k;
        t_amp *= f64::max(0.2, 1.0 - 0.3 * d);
        qt *= 1.0 + 0.08 * d;
    }
    EcgFeatures {
        pr,
        qrs,
        qt,
        p_amp,
        t_amp,
        r_amp,
        t_w,
        p_w,
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
        let q = f.qrs;
        add(tr - f.pr + 0.5 * 0.03, f.p_amp, f.p_w); // P
        add(tr - Q_POS * q, -0.12 * f.r_amp, Q_SIGMA * q); // Q
        add(tr, f.r_amp, R_SIGMA * q); // R
        add(tr + S_POS * q, -0.20 * f.r_amp, S_SIGMA * q); // S
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

/// Facteur de largeur du QRS d'un battement ventriculaire (placeholder D).
pub const WIDE_QRS: f64 = 2.2;
/// Facteur d'amplitude de son onde R (placeholder D).
pub const WIDE_R: f64 = 1.2;
/// Onde T discordante d'un battement ventriculaire (facteur de `t_amp`, placeholder D).
pub const WIDE_T: f64 = -1.5;
/// Amplitude des ondes f de la FA au cycle moyen (facteur de `p_amp`, placeholder D). Elle est
/// proportionnelle au temps de récupération (intervalle depuis l'activation atriale précédente
/// rapporté à `af_cycle`, plafonné à `F_WAVE_MAX`) : variabilité issue de la conduction.
pub const F_WAVE: f64 = 0.3;
/// Plafond du rapport intervalle / cycle moyen des ondes f (placeholder D).
pub const F_WAVE_MAX: f64 = 2.0;

/// ECG construit depuis la séquence de conduction (miroir de `signals.make_ecg_conducted`) :
/// onde P à chaque activation atriale organisée (aucune en arrêt atrial), ondes f en FA, QRS à
/// chaque activation ventriculaire après le PR de ce battement, QRS large et T discordante pour
/// un battement ventriculaire, QT corrigé par le RR précédent (Fridericia).
pub fn make_ecg_conducted(
    p: &Params,
    cond: &crate::rhythm::Conduction,
    hr: f64,
    duration_s: f64,
    fs: f64,
    noise: &mut dyn NoiseSource,
) -> Ecg {
    let f = ecg_features(p);
    let t = arange(0.0, duration_s, 1.0 / fs);
    let mut sig = vec![0.0; t.len()];
    let mut add = |c: f64, a: f64, w: f64| {
        for (s, ti) in sig.iter_mut().zip(&t) {
            let z = (ti - c) / w;
            *s += a * math::exp(-0.5 * (z * z));
        }
    };
    let mut prev_a: Option<f64> = None;
    for a in &cond.atrial {
        if a.t > duration_s + 0.5 {
            break;
        }
        if a.organized {
            add(a.t + 0.5 * 0.03, f.p_amp, f.p_w); // P
        } else {
            let rec = prev_a.map_or(1.0, |q| f64::min(F_WAVE_MAX, (a.t - q) / p.af_cycle));
            add(a.t, F_WAVE * p.p_amp * rec, 0.012); // onde f
        }
        prev_a = Some(a.t);
    }
    let mut prev: Option<f64> = None;
    let mut r_times = Vec::new();
    for b in &cond.ventricular {
        let tv = b.t;
        if tv > duration_s + 0.5 {
            break;
        }
        let rr = prev.map_or(60.0 / hr, |q| tv - q);
        prev = Some(tv);
        if tv < duration_s {
            r_times.push(tv);
        }
        let qt = f.qt * math::pow(95.0 / f64::max(60.0 / rr, 30.0), 1.0 / 3.0);
        if b.wide {
            let qrs = f.qrs * WIDE_QRS;
            add(tv, WIDE_R * f.r_amp, WIDE_R_SIGMA * qrs);
            add(tv + WIDE_S_POS * qrs, -0.35 * f.r_amp, WIDE_S_SIGMA * qrs);
            add(
                tv - 0.5 * qrs + qt - 2.2 * f.t_w,
                WIDE_T * f.t_amp,
                1.5 * f.t_w,
            );
        } else {
            let qrs = f.qrs;
            add(tv - Q_POS * qrs, -0.12 * f.r_amp, Q_SIGMA * qrs);
            add(tv, f.r_amp, R_SIGMA * qrs);
            add(tv + S_POS * qrs, -0.20 * f.r_amp, S_SIGMA * qrs);
            add(tv - 0.5 * qrs + qt - 2.2 * f.t_w, f.t_amp, f.t_w);
        }
    }
    for (s, ti) in sig.iter_mut().zip(&t) {
        *s += 0.01 * noise.standard_normal() + 0.02 * math::sin(2.0 * PI * 0.25 * ti);
    }
    Ecg {
        t,
        v: sig,
        features: f,
        r_times,
    }
}

/// Chauffe (s) avant la fenêtre des signaux : l'hémodynamique battement par battement du son
/// atteint son régime ; ECG et son partagent la même fenêtre décalée.
pub const SIGNAL_WARM_S: f64 = 5.0;

/// Séquence de conduction décalée : événements de `[w, ∞)` ramenés à l'origine.
pub fn shift_conduction(cond: &crate::rhythm::Conduction, w: f64) -> crate::rhythm::Conduction {
    crate::rhythm::Conduction {
        atrial: cond
            .atrial
            .iter()
            .filter(|a| a.t >= w)
            .map(|a| crate::rhythm::AtrialEvent { t: a.t - w, ..*a })
            .collect(),
        ventricular: cond
            .ventricular
            .iter()
            .filter(|b| b.t >= w)
            .map(|b| crate::rhythm::VentricularEvent {
                t: b.t - w,
                t_a: b.t_a.map(|x| x - w),
                ..*b
            })
            .collect(),
    }
}

/// Séquence de conduction des signaux : décharges sinusales du modèle autonome (variabilité
/// respiratoire) propagées par la couche de conduction ; aléa `SplitMix64(seed)` (miroir de
/// `signals.patient_conduction`).
pub fn patient_conduction(
    ev: &Evaluation,
    sinus_times: &[f64],
    seed: u64,
) -> crate::rhythm::Conduction {
    let mut g = crate::rhythm::SplitMix64::new(seed);
    let t_end = sinus_times.last().copied().unwrap_or(0.0);
    crate::rhythm::simulate_conduction(
        &ev.params,
        ev.autonomic.v_eff,
        60.0 / ev.sinus_hr,
        t_end,
        &mut || g.random(),
        Some(sinus_times),
    )
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
    /// Durée de contraction `ts = 0,37 √T` (s).
    pub ts: f64,
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
            ts: h.timing.ts,
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

pub(crate) struct BeatEvents {
    pub(crate) i_mc: usize,
    pub(crate) i_ac: usize,
    pub(crate) dpdt_max: f64,
    pub(crate) q_early: f64,
    pub(crate) dp_close: f64,
}

/// Événements valvulaires : fermeture mitrale (P_VG dépasse P_OG), fermeture aortique (débit
/// aortique repasse ≤ 0 après son pic), dP/dt max du VG, remplissage rapide.
pub(crate) fn beat_events(h: &PcgInputs, dt: f64) -> BeatEvents {
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
    /// Bruit du PCG (jalon M6).
    pub const PCG: u64 = 3;
    /// Bruit du module auscultation (pcg.rs).
    pub const AUSCULTATION: u64 = 4;
    /// Aléa de la conduction des signaux (FA, ectopie) ; le moteur utilise SplitMix64 (rhythm.rs).
    pub const RHYTHM: u64 = 5;
    /// Trajectoires Monte-Carlo : fluctuation naturelle, sévérité, PK/PD (variation.rs).
    pub const VARIATION: u64 = 6;
    /// Bruit de mesure des trajectoires Monte-Carlo.
    pub const MEASUREMENT: u64 = 7;
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
        duration_s: (opts.duration_s + SIGNAL_WARM_S).max(120.0),
        ..AutonomicOptions::default()
    };
    let au = simulate_autonomic(
        &ev.into(),
        &au_opts,
        &mut SimRng::for_stream(seed, streams::AUTONOMIC),
    );
    let cond = shift_conduction(
        &patient_conduction(ev, &au.beats, seed.wrapping_add(streams::RHYTHM)),
        SIGNAL_WARM_S,
    );
    let ecg = make_ecg_conducted(
        &ev.params,
        &cond,
        ev.params.hr,
        opts.duration_s,
        opts.ecg_fs,
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
