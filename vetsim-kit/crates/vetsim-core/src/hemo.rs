//! Boucle cardiovasculaire à 6 compartiments et élastances variables
//! (miroir de `physio.simulate_hemo`, MODEL_SPEC §2).
//!
//! États : volumes `[VG, VD, aorte, veines systémiques, artère pulmonaire, veines pulmonaires/OG]`
//! (mL). Les ventricules suivent une élastance activée par un demi-cosinus (systole `ts`,
//! relaxation `tr`), avec une relation télédiastolique exponentielle ; les vaisseaux sont des
//! compliances linéaires ; les valves sont des diodes lisses avec une conductance de fuite
//! (régurgitation).

use crate::error::Error;
use crate::ode::{solve, Options};
use crate::params::Params;

/// Activation ventriculaire `e(t)` ∈ [0, 1] : demi-cosinus montant sur `[0, ts)`,
/// descendant sur `[ts, ts + tr)`, nulle en diastole.
pub fn activation(t: f64, period: f64, ts: f64, tr: f64) -> f64 {
    let tc = t % period;
    if tc < ts {
        0.5 * (1.0 - (std::f64::consts::PI * tc / ts).cos())
    } else if tc < ts + tr {
        0.5 * (1.0 + (std::f64::consts::PI * (tc - ts) / tr).cos())
    } else {
        0.0
    }
}

/// Pression ventriculaire : `e·Ees·(V − V0) + (1 − e)·a·(exp(clip(b(V − V0), −20, 8)) − 1)`.
pub fn ventricular_pressure(v: f64, e: f64, ees: f64, v0: f64, a: f64, b: f64) -> f64 {
    let x = (b * (v - v0)).clamp(-20.0, 8.0);
    e * ees * (v - v0) + (1.0 - e) * a * (x.exp() - 1.0)
}

/// Débit valvulaire (diode lisse) : `σ = ½(1 + tanh(Δp/0,3))`,
/// `Q = σ·Δp/r + (1 − σ)·g·Δp` (`g` = conductance de fuite rétrograde).
pub fn valve_flow(dp: f64, r_forward: f64, g_back: f64) -> f64 {
    let s = 0.5 * (1.0 + (dp / 0.3).tanh());
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
        }
    }
}

/// Pressions et débits pour les volumes `v` à l'instant `t`.
pub fn flows(v: &[f64; 6], t: f64, p: &Params, tm: &Timing) -> (Pressures, Flows, f64) {
    let [lv, rv, ao, sv, pa, pv] = *v;
    let e = activation(t, tm.period, tm.ts, tm.tr);
    let pr = Pressures {
        lv: ventricular_pressure(lv, e, p.ees_lv, p.v0_lv, p.a_lv, p.b_lv),
        rv: ventricular_pressure(rv, e, p.ees_rv, p.v0_rv, p.a_rv, p.b_rv),
        ao: (ao - p.v0_ao) / p.c_ao,
        sv: (sv - p.v0_sv) / p.c_sv,
        pa: (pa - p.v0_pa) / p.c_pa,
        pv: (pv - p.v0_pv) / p.c_pv,
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
        p.v0_lv + (1.0 + 8.0 / p.a_lv).ln() / p.b_lv,
        p.v0_rv + (1.0 + 4.0 / p.a_rv).ln() / p.b_rv,
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
    if !(p.hr.is_finite() && p.hr > 0.0) {
        return Err(Error::InvalidInput(format!("FC {} /min", p.hr)));
    }
    let tm = Timing::from_hr(p.hr);
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
    };
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
