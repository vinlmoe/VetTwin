//! Boucle cardiovasculaire à 6 compartiments et élastances variables
//! (miroir de `physio.simulate_hemo`, MODEL_SPEC §2).
//!
//! États : volumes `[VG, VD, aorte, veines systémiques, artère pulmonaire, veines pulmonaires/OG]`
//! (mL). Les ventricules suivent une élastance activée par un demi-cosinus (systole `ts`,
//! relaxation `tr`), avec une relation télédiastolique exponentielle ; les vaisseaux sont des
//! compliances linéaires ; les valves sont des diodes lisses avec une conductance de fuite
//! (régurgitation).

use crate::error::Error;
use crate::math;
use crate::ode::{solve, Options};
use crate::params::Params;

/// Activation ventriculaire `e(t)` ∈ [0, 1] : demi-cosinus montant sur `[0, ts)`,
/// descendant sur `[ts, ts + tr)`, nulle en diastole.
pub fn activation(t: f64, period: f64, ts: f64, tr: f64) -> f64 {
    let tc = t % period;
    if tc < ts {
        0.5 * (1.0 - math::cos(std::f64::consts::PI * tc / ts))
    } else if tc < ts + tr {
        0.5 * (1.0 + math::cos(std::f64::consts::PI * (tc - ts) / tr))
    } else {
        0.0
    }
}

/// Pression ventriculaire : `e·Ees·(V − V0) + (1 − e)·a·(exp(clip(b(V − V0), −20, 8)) − 1)`.
pub fn ventricular_pressure(v: f64, e: f64, ees: f64, v0: f64, a: f64, b: f64) -> f64 {
    let x = (b * (v - v0)).clamp(-20.0, 8.0);
    e * ees * (v - v0) + (1.0 - e) * a * (math::exp(x) - 1.0)
}

/// Débit valvulaire (diode lisse) : `σ = ½(1 + tanh(Δp/0,3))`,
/// `Q = σ·Δp/r + (1 − σ)·g·Δp` (`g` = conductance de fuite rétrograde).
pub fn valve_flow(dp: f64, r_forward: f64, g_back: f64) -> f64 {
    let s = 0.5 * (1.0 + math::tanh(dp / 0.3));
    s * dp / r_forward + (1.0 - s) * g_back * dp
}

/// Pressions (mmHg) des 6 compartiments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pressures {
    /// VG.
    pub lv: f64,
    /// VD.
    pub rv: f64,
    /// Aorte.
    pub ao: f64,
    /// Veines systémiques.
    pub sv: f64,
    /// Artère pulmonaire.
    pub pa: f64,
    /// Veines pulmonaires / OG.
    pub pv: f64,
}

/// Débits (mL/s).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flows {
    /// Mitral (OG → VG ; négatif = régurgitation).
    pub mi: f64,
    /// Aortique (VG → aorte).
    pub ao: f64,
    /// Tricuspide.
    pub tr: f64,
    /// Pulmonaire (valve).
    pub pu: f64,
    /// Circulation systémique.
    pub sys: f64,
    /// Circulation pulmonaire.
    pub pul: f64,
}

/// Rythme : période et durées de systole et de relaxation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// Période `T = 60/FC` (s).
    pub period: f64,
    /// Durée de systole `ts = 0,37·√T` (s).
    pub ts: f64,
    /// Durée de relaxation `tr = 0,4·ts` (s).
    pub tr: f64,
    /// Délai oreillette → ventricule (s) ; `None` : paramètre `pr`.
    pub pr: Option<f64>,
}

impl Timing {
    /// Rythme pour une FC (/min).
    pub fn from_hr(hr: f64) -> Self {
        let period = 60.0 / hr;
        let ts = 0.37 * period.sqrt();
        Self {
            period,
            ts,
            tr: 0.4 * ts,
            pr: None,
        }
    }
}

/// Activation atriale : débute `pr` avant l'activation ventriculaire du même battement.
pub fn atrial_activation(t: f64, p: &Params, tm: &Timing) -> f64 {
    activation(t + tm.pr.unwrap_or(p.pr), tm.period, p.ta_atr, p.tra_atr)
}

/// Pressions et débits pour les volumes `v` à l'instant `t`.
///
/// Oreillettes contractiles : élastance active proportionnelle à la pression passive des
/// compartiments `pv` (OG + veines pulmonaires) et `sv` (OD + veines systémiques),
/// `P = (1 + k·e_a(t))·P_passive` ; `k = 0` : oreillettes passives (modèle d'origine).
pub fn flows(v: &[f64; 6], t: f64, p: &Params, tm: &Timing) -> (Pressures, Flows, f64) {
    let [lv, rv, ao, sv, pa, pv] = *v;
    let e = activation(t, tm.period, tm.ts, tm.tr);
    let ea = atrial_activation(t, p, tm);
    let pr = Pressures {
        lv: ventricular_pressure(lv, e, p.ees_lv, p.v0_lv, p.a_lv, p.b_lv),
        rv: ventricular_pressure(rv, e, p.ees_rv, p.v0_rv, p.a_rv, p.b_rv),
        ao: (ao - p.v0_ao) / p.c_ao,
        sv: (1.0 + p.k_ra * ea) * (sv - p.v0_sv) / p.c_sv,
        pa: (pa - p.v0_pa) / p.c_pa,
        pv: (1.0 + p.k_la * ea) * (pv - p.v0_pv) / p.c_pv,
    };
    let rf = p.r_valve;
    let q = Flows {
        mi: valve_flow(pr.pv - pr.lv, rf, p.g_mr),
        ao: valve_flow(pr.lv - pr.ao, rf, p.g_ar),
        tr: valve_flow(pr.sv - pr.rv, rf, p.g_tr),
        pu: valve_flow(pr.rv - pr.pa, rf, p.g_pr),
        sys: (pr.ao - pr.sv) / p.r_sys,
        pul: (pr.pa - pr.pv) / p.r_pul,
    };
    (pr, q, e)
}

/// Volumes initiaux (mL) : ventricules sur leur relation télédiastolique (8 et 4 mmHg),
/// vaisseaux à des pressions de départ usuelles ; la volémie (`bvf`, sur 85 mL/kg) et
/// `vol_shift` agissent sur le réservoir veineux systémique.
pub fn initial_volumes(p: &Params) -> [f64; 6] {
    let vtot = 85.0 * p.bw;
    [
        p.v0_lv + math::ln(1.0 + 8.0 / p.a_lv) / p.b_lv,
        p.v0_rv + math::ln(1.0 + 4.0 / p.a_rv) / p.b_rv,
        p.v0_ao + p.c_ao * 100.0,
        p.v0_sv + p.c_sv * 4.0 + (p.bvf - 1.0) * vtot + p.vol_shift,
        p.v0_pa + p.c_pa * 14.0,
        p.v0_pv + p.c_pv * 6.0,
    ]
}

/// Indicateurs hémodynamiques du dernier battement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HemoSummary {
    /// PAM (mmHg).
    pub map: f64,
    /// PAS (mmHg).
    pub sap: f64,
    /// PAD (mmHg).
    pub dap: f64,
    /// Débit cardiaque (L/min).
    pub co: f64,
    /// Volume télédiastolique du VG (mL).
    pub edv: f64,
    /// Volume télésystolique du VG (mL).
    pub esv: f64,
    /// Fraction d'éjection.
    pub ef: f64,
    /// Volume d'éjection total (mL).
    pub sv: f64,
    /// Fraction de régurgitation (mitrale + aortique).
    pub reg_fraction: f64,
    /// POG moyenne (mmHg).
    pub la_mean: f64,
    /// POG maximale (mmHg).
    pub la_max: f64,
    /// PAP moyenne (mmHg).
    pub pap_mean: f64,
    /// PVC (mmHg).
    pub cvp: f64,
    /// Pression télédiastolique du VG (mmHg).
    pub lv_edp: f64,
    /// Régime périodique atteint (volume d'éjection stable à 3 % entre les 2 derniers battements).
    pub converged: bool,
    /// Pic du débit mitral protodiastolique (onde E, hors activation atriale ; mL/s).
    pub mitral_e: f64,
    /// Pic du débit mitral pendant l'activation atriale (onde A ; mL/s).
    pub mitral_a: f64,
}

/// Séries du dernier battement (échantillonnage `dt`), temps relatif au début du battement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Waveforms {
    /// Temps (s).
    pub t: Vec<f64>,
    /// Pressions VG, VD, aorte, veines systémiques, AP, OG (mmHg).
    pub p_lv: Vec<f64>,
    /// Pression VD.
    pub p_rv: Vec<f64>,
    /// Pression aortique.
    pub p_ao: Vec<f64>,
    /// Pression veineuse systémique.
    pub p_sv: Vec<f64>,
    /// Pression artérielle pulmonaire.
    pub p_pa: Vec<f64>,
    /// Pression OG.
    pub p_pv: Vec<f64>,
    /// Débit mitral (mL/s).
    pub q_mi: Vec<f64>,
    /// Débit aortique.
    pub q_ao: Vec<f64>,
    /// Débit tricuspide.
    pub q_tr: Vec<f64>,
    /// Débit pulmonaire valvulaire.
    pub q_pu: Vec<f64>,
    /// Débit systémique.
    pub q_sys: Vec<f64>,
    /// Débit pulmonaire.
    pub q_pul: Vec<f64>,
    /// Volume VG (mL).
    pub v_lv: Vec<f64>,
}

/// Résultat d'une simulation hémodynamique.
#[derive(Debug, Clone, PartialEq)]
pub struct HemoResult {
    /// Rythme utilisé.
    pub timing: Timing,
    /// Indicateurs.
    pub summary: HemoSummary,
    /// Séries du dernier battement.
    pub waveforms: Waveforms,
}

/// Nombre de battements simulés (régime périodique).
pub const N_BEATS: usize = 11;
/// Pas d'échantillonnage des indicateurs (s).
pub const DT: f64 = 0.002;

/// Simule `n_beats` battements et calcule les indicateurs sur le dernier.
pub fn simulate_hemo(p: &Params, n_beats: usize) -> Result<HemoResult, Error> {
    simulate_hemo_pr(p, n_beats, None)
}

/// Comme [`simulate_hemo`], avec le délai oreillette → ventricule de la conduction.
pub fn simulate_hemo_pr(p: &Params, n_beats: usize, pr: Option<f64>) -> Result<HemoResult, Error> {
    if !(p.hr.is_finite() && p.hr > 0.0) {
        return Err(Error::InvalidInput(format!("FC {} /min", p.hr)));
    }
    let mut tm = Timing::from_hr(p.hr);
    tm.pr = pr;
    let t_end = n_beats as f64 * tm.period;
    // Échantillons comme np.arange(t_end − 2T, t_end, dt) (y compris le calcul de numpy :
    // longueur ceil((stop − start)/dt), valeurs start + i·((start + dt) − start)).
    let start = t_end - 2.0 * tm.period;
    let n = ((t_end - start) / DT).ceil() as usize;
    let delta = (start + DT) - start;
    let t_eval: Vec<f64> = (0..n).map(|i| start + i as f64 * delta).collect();
    let mut samples: Vec<[f64; 6]> = Vec::with_capacity(n);
    let opts = Options {
        rtol: 1e-6,
        atol: 1e-6,
        max_step: 0.004,
        ..Options::default()
    };
    let rhs = |t: f64, v: &[f64; 6]| {
        let (_, q, _) = flows(v, t, p, &tm);
        [
            q.mi - q.ao,
            q.tr - q.pu,
            q.ao - q.sys,
            q.sys - q.tr,
            q.pu - q.pul,
            q.pul - q.mi,
        ]
    };
    let mut next = 0;
    solve(rhs, 0.0, initial_volumes(p), t_end, &opts, |step| {
        // Échantillons de ]t_old, t] (comme scipy : searchsorted côté droit).
        while next < n && t_eval[next] <= step.t {
            samples.push(step.interpolate(t_eval[next]));
            next += 1;
        }
    })
    .map_err(Error::Ode)?;
    if samples.len() != n {
        return Err(Error::InvalidInput(format!(
            "échantillonnage incomplet ({}/{n})",
            samples.len()
        )));
    }
    let nb = (tm.period / DT).round_ties_even() as usize;
    if nb == 0 || 2 * nb > n {
        return Err(Error::InvalidInput(format!(
            "FC {} hors du domaine simulable",
            p.hr
        )));
    }
    let last = n - nb..n;
    let prev = n - 2 * nb..n - nb;
    let mut w = Waveforms::default();
    let t0 = t_eval[last.start];
    for i in last.clone() {
        let (pr, q, _) = flows(&samples[i], t_eval[i], p, &tm);
        w.t.push(t_eval[i] - t0);
        w.p_lv.push(pr.lv);
        w.p_rv.push(pr.rv);
        w.p_ao.push(pr.ao);
        w.p_sv.push(pr.sv);
        w.p_pa.push(pr.pa);
        w.p_pv.push(pr.pv);
        w.q_mi.push(q.mi);
        w.q_ao.push(q.ao);
        w.q_tr.push(q.tr);
        w.q_pu.push(q.pu);
        w.q_sys.push(q.sys);
        w.q_pul.push(q.pul);
        w.v_lv.push(samples[i][0]);
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let max = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let edv = max(&w.v_lv);
    let esv = min(&w.v_lv);
    let sv_tot = edv - esv;
    let v_prev: Vec<f64> = samples[prev].iter().map(|s| s[0]).collect();
    let sv_prev = max(&v_prev) - min(&v_prev);
    let v_reg = (-w.q_mi.iter().map(|x| x.min(0.0)).sum::<f64>()
        - w.q_ao.iter().map(|x| x.min(0.0)).sum::<f64>())
        * DT;
    let summary = HemoSummary {
        map: mean(&w.p_ao),
        sap: max(&w.p_ao),
        dap: min(&w.p_ao),
        co: mean(&w.q_sys) * 60.0 / 1000.0,
        edv,
        esv,
        ef: sv_tot / edv,
        sv: sv_tot,
        reg_fraction: if sv_tot > 0.0 { v_reg / sv_tot } else { 0.0 },
        la_mean: mean(&w.p_pv),
        la_max: max(&w.p_pv),
        pap_mean: mean(&w.p_pa),
        cvp: mean(&w.p_sv),
        lv_edp: w.p_lv[0],
        converged: (sv_tot - sv_prev).abs() / sv_tot.max(1e-6) < 0.03,
        mitral_e: 0.0,
        mitral_a: 0.0,
    };
    // flux mitral : pic E (hors activation atriale) et pic A (pendant), comme la référence
    let (mut pe, mut pa) = (None::<f64>, None::<f64>);
    for (k, &q) in w.q_mi.iter().enumerate() {
        let ea = atrial_activation(t_eval[last.start + k], p, &tm);
        let qf = q.max(0.0);
        let slot = if ea <= 0.0 { &mut pe } else { &mut pa };
        *slot = Some(slot.map_or(qf, |m| m.max(qf)));
    }
    let mut summary = summary;
    summary.mitral_e = pe.unwrap_or(0.0);
    summary.mitral_a = pa.unwrap_or(0.0);
    Ok(HemoResult {
        timing: tm,
        summary,
        waveforms: w,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_profile() {
        let tm = Timing::from_hr(60.0);
        assert_eq!(activation(0.0, tm.period, tm.ts, tm.tr), 0.0);
        assert!((activation(tm.ts, tm.period, tm.ts, tm.tr) - 1.0).abs() < 1e-15);
        assert_eq!(activation(0.9, tm.period, tm.ts, tm.tr), 0.0);
        // périodique
        let a = activation(0.1, tm.period, tm.ts, tm.tr);
        assert!((activation(3.1, tm.period, tm.ts, tm.tr) - a).abs() < 1e-12);
    }

    #[test]
    fn valve_is_a_smooth_diode() {
        assert!((valve_flow(10.0, 0.03, 0.0) - 10.0 / 0.03).abs() < 1e-6);
        assert!(valve_flow(-10.0, 0.03, 0.0).abs() < 1e-20);
        assert!((valve_flow(-10.0, 0.03, 0.5) + 5.0).abs() < 1e-6);
    }
}

// ------------------------------------------------------------------ battement par battement
// Miroir de `physio.simulate_hemo_beats` : la séquence de battements vient de la couche de
// conduction (`rhythm`), chaque battement a son activation propre.

/// Battement ventriculaire : début, durée de systole, durée de relaxation (s).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beat {
    /// Début de l'activation (s).
    pub t: f64,
    /// Systole `0,37·√RR` (RR = intervalle qui précède).
    pub ts: f64,
    /// Relaxation `0,4·ts`.
    pub tr: f64,
}

/// Activation d'un battement isolé, `tau` secondes après son début.
pub fn activation_single(tau: f64, ts: f64, tr: f64) -> f64 {
    if (0.0..ts).contains(&tau) {
        0.5 * (1.0 - math::cos(std::f64::consts::PI * tau / ts))
    } else if tau >= ts && tau < ts + tr {
        0.5 * (1.0 + math::cos(std::f64::consts::PI * (tau - ts) / tr))
    } else {
        0.0
    }
}

/// Battements ventriculaires et activations atriales organisées d'une séquence de conduction.
pub fn beat_schedule(cond: &crate::rhythm::Conduction, period0: f64) -> (Vec<Beat>, Vec<f64>) {
    let tv: Vec<f64> = cond.ventricular.iter().map(|b| b.t).collect();
    let beats = tv
        .iter()
        .enumerate()
        .map(|(i, &t)| {
            let rr = if i > 0 { t - tv[i - 1] } else { period0 };
            let ts = 0.37 * rr.sqrt();
            Beat {
                t,
                ts,
                tr: 0.4 * ts,
            }
        })
        .collect();
    let atr = cond
        .atrial
        .iter()
        .filter(|a| a.organized)
        .map(|a| a.t)
        .collect();
    (beats, atr)
}

/// Activation ventriculaire (max des deux derniers battements) et atriale à l'instant `t`.
fn activations_at(t: f64, beats: &[Beat], atr: &[f64], p: &Params) -> (f64, f64) {
    let i = beats.partition_point(|b| b.t <= t) as isize - 1;
    let mut e: f64 = 0.0;
    for j in [i - 1, i] {
        if j >= 0 && (j as usize) < beats.len() {
            let b = beats[j as usize];
            e = e.max(activation_single(t - b.t, b.ts, b.tr));
        }
    }
    let k = atr.partition_point(|&a| a <= t) as isize - 1;
    let mut ea: f64 = 0.0;
    for j in [k - 1, k] {
        if j >= 0 && (j as usize) < atr.len() {
            ea = ea.max(activation_single(t - atr[j as usize], p.ta_atr, p.tra_atr));
        }
    }
    (e, ea)
}

fn flows_beats(
    v: &[f64; 6],
    t: f64,
    p: &Params,
    beats: &[Beat],
    atr: &[f64],
) -> (Pressures, Flows, f64, f64) {
    let [lv, rv, ao, sv, pa, pv] = *v;
    let (e, ea) = activations_at(t, beats, atr, p);
    let pr = Pressures {
        lv: ventricular_pressure(lv, e, p.ees_lv, p.v0_lv, p.a_lv, p.b_lv),
        rv: ventricular_pressure(rv, e, p.ees_rv, p.v0_rv, p.a_rv, p.b_rv),
        ao: (ao - p.v0_ao) / p.c_ao,
        sv: (1.0 + p.k_ra * ea) * (sv - p.v0_sv) / p.c_sv,
        pa: (pa - p.v0_pa) / p.c_pa,
        pv: (1.0 + p.k_la * ea) * (pv - p.v0_pv) / p.c_pv,
    };
    let rf = p.r_valve;
    let q = Flows {
        mi: valve_flow(pr.pv - pr.lv, rf, p.g_mr),
        ao: valve_flow(pr.lv - pr.ao, rf, p.g_ar),
        tr: valve_flow(pr.sv - pr.rv, rf, p.g_tr),
        pu: valve_flow(pr.rv - pr.pa, rf, p.g_pr),
        sys: (pr.ao - pr.sv) / p.r_sys,
        pul: (pr.pa - pr.pv) / p.r_pul,
    };
    (pr, q, e, ea)
}

/// Indicateurs d'un battement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatMetrics {
    /// Début (s).
    pub t: f64,
    /// Intervalle jusqu'au battement suivant (s).
    pub rr: f64,
    /// Contraction précédente terminée au début du battement.
    pub relaxed: bool,
    /// VTD, VTS, VES (mL).
    pub edv: f64,
    /// VTS (mL).
    pub esv: f64,
    /// VES (mL).
    pub sv: f64,
    /// PAS du battement (mmHg).
    pub sap: f64,
    /// PAD du battement (mmHg).
    pub dap: f64,
    /// Pression VG au début du battement (mmHg).
    pub lv_edp: f64,
    /// Volume régurgité (mL).
    pub v_reg: f64,
    /// Pic E (mL/s).
    pub mitral_e: f64,
    /// Pic A (mL/s).
    pub mitral_a: f64,
}

/// Résultat battement par battement.
#[derive(Debug, Clone, PartialEq)]
pub struct BeatsResult {
    /// Indicateurs moyens (mêmes champs que la simulation périodique).
    pub summary: HemoSummary,
    /// FC ventriculaire moyenne (/min).
    pub hr_v: f64,
    /// Battements complets de la fenêtre.
    pub beats: Vec<BeatMetrics>,
    /// Séries sur la fenêtre `[warm, t_end)` (temps absolu).
    pub waveforms: Waveforms,
}

/// Simule la circulation sur une séquence de battements quelconque ; indicateurs moyennés sur
/// les battements complets après `warm` secondes (miroir de `physio.simulate_hemo_beats`).
pub fn simulate_hemo_beats(
    p: &Params,
    cond: &crate::rhythm::Conduction,
    t_end: f64,
    warm: f64,
    period0: f64,
) -> Result<BeatsResult, Error> {
    let (beats, atr) = beat_schedule(cond, period0);
    let n = ((t_end - warm) / DT).ceil() as usize;
    let delta = (warm + DT) - warm;
    let t_eval: Vec<f64> = (0..n).map(|i| warm + i as f64 * delta).collect();
    let mut samples: Vec<[f64; 6]> = Vec::with_capacity(n);
    let opts = Options {
        rtol: 1e-6,
        atol: 1e-6,
        max_step: 0.004,
        ..Options::default()
    };
    let rhs = |t: f64, v: &[f64; 6]| {
        let (_, q, _, _) = flows_beats(v, t, p, &beats, &atr);
        [
            q.mi - q.ao,
            q.tr - q.pu,
            q.ao - q.sys,
            q.sys - q.tr,
            q.pu - q.pul,
            q.pul - q.mi,
        ]
    };
    let mut next = 0;
    solve(rhs, 0.0, initial_volumes(p), t_end, &opts, |step| {
        while next < n && t_eval[next] <= step.t {
            samples.push(step.interpolate(t_eval[next]));
            next += 1;
        }
    })
    .map_err(Error::Ode)?;
    if samples.len() != n {
        return Err(Error::InvalidInput(format!(
            "échantillonnage incomplet ({}/{n})",
            samples.len()
        )));
    }
    let mut w = Waveforms::default();
    let mut ea_s = Vec::with_capacity(n);
    for (i, s) in samples.iter().enumerate() {
        let (pr, q, _, ea) = flows_beats(s, t_eval[i], p, &beats, &atr);
        w.t.push(t_eval[i]);
        w.p_lv.push(pr.lv);
        w.p_rv.push(pr.rv);
        w.p_ao.push(pr.ao);
        w.p_sv.push(pr.sv);
        w.p_pa.push(pr.pa);
        w.p_pv.push(pr.pv);
        w.q_mi.push(q.mi);
        w.q_ao.push(q.ao);
        w.q_tr.push(q.tr);
        w.q_pu.push(q.pu);
        w.q_sys.push(q.sys);
        w.q_pul.push(q.pul);
        w.v_lv.push(s[0]);
        ea_s.push(ea);
    }
    let starts: Vec<f64> = beats.iter().map(|b| b.t).filter(|&t| t >= warm).collect();
    let relaxed_at = |a: f64| -> bool {
        match beats.iter().position(|b| b.t == a) {
            Some(i) if i > 0 => a - beats[i - 1].t >= beats[i - 1].ts + beats[i - 1].tr,
            _ => true,
        }
    };
    let mut per = Vec::new();
    for win in starts.windows(2) {
        let (a, b) = (win[0], win[1]);
        let idx: Vec<usize> = (0..n).filter(|&i| w.t[i] >= a && w.t[i] < b).collect();
        if idx.len() < 3 {
            continue;
        }
        let fold = |f: &dyn Fn(usize) -> f64, init: f64, op: fn(f64, f64) -> f64| {
            idx.iter().map(|&i| f(i)).fold(init, op)
        };
        let edv = fold(&|i| w.v_lv[i], f64::NEG_INFINITY, f64::max);
        let esv = fold(&|i| w.v_lv[i], f64::INFINITY, f64::min);
        let v_reg = (-idx.iter().map(|&i| w.q_mi[i].min(0.0)).sum::<f64>()
            - idx.iter().map(|&i| w.q_ao[i].min(0.0)).sum::<f64>())
            * DT;
        let peak = |pred: &dyn Fn(f64) -> bool| -> f64 {
            let v: Vec<f64> = idx
                .iter()
                .filter(|&&i| pred(ea_s[i]))
                .map(|&i| w.q_mi[i].max(0.0))
                .collect();
            if v.is_empty() {
                0.0
            } else {
                v.into_iter().fold(f64::NEG_INFINITY, f64::max)
            }
        };
        per.push(BeatMetrics {
            t: a,
            rr: b - a,
            relaxed: relaxed_at(a),
            edv,
            esv,
            sv: edv - esv,
            sap: fold(&|i| w.p_ao[i], f64::NEG_INFINITY, f64::max),
            dap: fold(&|i| w.p_ao[i], f64::INFINITY, f64::min),
            lv_edp: w.p_lv[idx[0]],
            v_reg,
            mitral_e: peak(&|e| e <= 0.0),
            mitral_a: peak(&|e| e > 0.0),
        });
    }
    if per.is_empty() {
        return Err(Error::InvalidInput(
            "aucun battement complet dans la fenêtre".into(),
        ));
    }
    let (s0, s1) = (starts[0], *starts.last().expect("battements"));
    let win: Vec<usize> = (0..n).filter(|&i| w.t[i] >= s0 && w.t[i] < s1).collect();
    let wmean = |v: &[f64]| win.iter().map(|&i| v[i]).sum::<f64>() / win.len() as f64;
    let wmax = |v: &[f64]| win.iter().map(|&i| v[i]).fold(f64::NEG_INFINITY, f64::max);
    let bmean = |f: fn(&BeatMetrics) -> f64| per.iter().map(f).sum::<f64>() / per.len() as f64;
    let sv_tot: f64 = per.iter().map(|b| b.sv).sum();
    let rr_mean = bmean(|b| b.rr);
    let relaxed: Vec<f64> = per.iter().filter(|b| b.relaxed).map(|b| b.lv_edp).collect();
    let summary = HemoSummary {
        map: wmean(&w.p_ao),
        sap: bmean(|b| b.sap),
        dap: bmean(|b| b.dap),
        co: wmean(&w.q_sys) * 60.0 / 1000.0,
        edv: bmean(|b| b.edv),
        esv: bmean(|b| b.esv),
        ef: bmean(|b| b.sv) / bmean(|b| b.edv),
        sv: bmean(|b| b.sv),
        reg_fraction: if sv_tot > 0.0 {
            per.iter().map(|b| b.v_reg).sum::<f64>() / sv_tot
        } else {
            0.0
        },
        la_mean: wmean(&w.p_pv),
        la_max: wmax(&w.p_pv),
        pap_mean: wmean(&w.p_pa),
        cvp: wmean(&w.p_sv),
        lv_edp: if relaxed.is_empty() {
            f64::NAN
        } else {
            relaxed.iter().sum::<f64>() / relaxed.len() as f64
        },
        converged: true,
        mitral_e: bmean(|b| b.mitral_e),
        mitral_a: bmean(|b| b.mitral_a),
    };
    Ok(BeatsResult {
        summary,
        hr_v: 60.0 / rr_mean,
        beats: per,
        waveforms: w,
    })
}

impl BeatsResult {
    /// Forme commune avec la simulation périodique : indicateurs moyens et séries du dernier
    /// battement complet (temps relatif à son début), rythme moyen.
    pub fn into_hemo_result(self, p: &Params) -> HemoResult {
        let last = *self.beats.last().expect("au moins un battement");
        let w = &self.waveforms;
        let idx: Vec<usize> = (0..w.t.len())
            .filter(|&i| w.t[i] >= last.t && w.t[i] < last.t + last.rr)
            .collect();
        let pick = |v: &[f64]| idx.iter().map(|&i| v[i]).collect::<Vec<f64>>();
        let one = Waveforms {
            t: idx.iter().map(|&i| w.t[i] - last.t).collect(),
            p_lv: pick(&w.p_lv),
            p_rv: pick(&w.p_rv),
            p_ao: pick(&w.p_ao),
            p_sv: pick(&w.p_sv),
            p_pa: pick(&w.p_pa),
            p_pv: pick(&w.p_pv),
            q_mi: pick(&w.q_mi),
            q_ao: pick(&w.q_ao),
            q_tr: pick(&w.q_tr),
            q_pu: pick(&w.q_pu),
            q_sys: pick(&w.q_sys),
            q_pul: pick(&w.q_pul),
            v_lv: pick(&w.v_lv),
        };
        let mut timing = Timing::from_hr(self.hr_v);
        timing.pr = Some(p.pr);
        HemoResult {
            timing,
            summary: self.summary,
            waveforms: one,
        }
    }
}
