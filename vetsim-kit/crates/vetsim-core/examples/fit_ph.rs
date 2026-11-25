//! Calage du module « hypertension pulmonaire pré-capillaire ».
//!
//! `cargo run --release -p vetsim-core --example fit_ph -- aR p atr rv` : effets
//! r_pul × m, m = 1 + aR·s^p ; c_pa × m^(ln 0,52/ln 4,7) et z_pa × m^(ln 2,4/ln 4,7) (rapports de
//! l'état mesuré par Yoshida et al. 2021, embolie expérimentale, PMC8692872 : RVP ×4,7,
//! compliance ×0,52, vitesse d'onde ×2,4) ; a_tr + atr·s (cm²) ; ees_rv × (1 + rv·s). Sortie par sévérité : PAP, PVC, gradient VD–OD (≈ gradient d'IT), RVP (UW), compliance
//! VES/PP, TA, TE, TA/TE, débit, PAM, FC, fraction régurgitée tricuspide.
use serde_json::json;
use std::path::PathBuf;
use vetsim_core::{evaluate, Patient, PatientModule};

fn read(rel: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel),
    )
    .unwrap()
}

fn main() {
    let a: Vec<f64> = std::env::args()
        .skip(1)
        .map(|x| x.parse().unwrap())
        .collect();
    let (ar, p, atr, rv) = (a[0], a[1], a[2], a[3]);
    // compliance et impédance suivent la résistance comme dans l'état mesuré par Yoshida 2021
    // (RVP ×4,7 ; compliance ×0,52 ; vitesse d'onde ×2,4)
    let (ec, ez) = (
        vetsim_core::math::ln(0.52) / vetsim_core::math::ln(4.7),
        vetsim_core::math::ln(2.4) / vetsim_core::math::ln(4.7),
    );
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let base = Patient::healthy(&pm, 20.0).unwrap().params;
    let mut out = Vec::new();
    for s in [0.0f64, 0.1, 0.25, 0.4, 0.55, 0.7, 0.85, 1.0] {
        let mut q = base.clone();
        let m = 1.0 + ar * vetsim_core::math::pow(s, p);
        q.r_pul *= m;
        q.c_pa *= vetsim_core::math::pow(m, ec);
        q.z_pa *= vetsim_core::math::pow(m, ez);
        q.a_tr += atr * s;
        q.ees_rv *= 1.0 + rv * s;
        match evaluate(&q) {
            Ok(e) => {
                let w = &e.hemo.waveforms;
                let h = &e.hemo.summary;
                let dt = w.t[1] - w.t[0];
                let qmax = w.q_pu.iter().copied().fold(0.0, f64::max);
                let thr = 0.02 * qmax;
                let n = w.q_pu.len();
                let i0 = (1..n)
                    .find(|&i| w.q_pu[i - 1] <= thr && w.q_pu[i] > thr)
                    .unwrap_or(0);
                let (mut ip, mut i1) = (i0, i0);
                while i1 < n && w.q_pu[i1] > thr {
                    if w.q_pu[i1] > w.q_pu[ip] {
                        ip = i1;
                    }
                    i1 += 1;
                }
                let mx = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max);
                let mn = |v: &[f64]| v.iter().copied().fold(f64::MAX, f64::min);
                let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
                let paps = mx(&w.p_pa);
                let papd = mn(&w.p_pa);
                let papm = mean(&w.p_pa);
                let rap = mean(&w.p_sv);
                let grad = w
                    .p_rv
                    .iter()
                    .zip(&w.p_sv)
                    .map(|(v, r)| v - r)
                    .fold(f64::MIN, f64::max);
                let sv = h.co * 1000.0 / e.params.hr;
                let tr_back: f64 = -w.q_tr.iter().map(|x| x.min(0.0)).sum::<f64>() * dt;
                let tr_fwd: f64 = w.q_tr.iter().map(|x| x.max(0.0)).sum::<f64>() * dt;
                out.push(json!({"s": s, "paps": paps, "papd": papd, "papm": papm, "rap": rap, "trpg": grad,
                    "pvr_wu": (papm - h.la_mean) / h.co, "pac": sv / (paps - papd),
                    "at": (ip - i0) as f64 * dt * 1000.0, "et": (i1 - i0) as f64 * dt * 1000.0,
                    "at_et": (ip - i0) as f64 / (i1 - i0).max(1) as f64,
                    "co": h.co, "map": h.map, "hr": e.params.hr, "rf_tr": tr_back / tr_fwd.max(1e-9)}));
            }
            Err(err) => out.push(json!({"s": s, "err": err.to_string()})),
        }
    }
    println!(
        "{}",
        json!({"ar": ar, "p": p, "atr": atr, "rv": rv, "res": out})
    );
}
