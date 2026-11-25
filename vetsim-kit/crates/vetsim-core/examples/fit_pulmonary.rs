//! Calage conjoint du windkessel pulmonaire : compliance `c_pa`, impédance `z_pa`, inertance
//! `l_pa` (coefficients à 20 kg ; allométries s, s^(−2/3), s^(−1/3)).
//!
//! `cargo run --release -p vetsim-core --example fit_pulmonary -- c z l` : PAP systolique,
//! diastolique et moyenne, temps d'accélération et d'éjection du flux pulmonaire (ms), rapport
//! TA/TE, vitesse pulmonaire maximale (m/s, anneau de diamètre 0,795·BW^⅓, Brown 2003), gradient
//! maximal VD − AP, débit et PAM, à 5, 20 et 40 kg.
//!
//! Calage retenu (24/11/2026) : `c_pa` 2,5 (inchangé), `z_pa` 0,02, `l_pa` 0,0013 ; cibles TA
//! 73–79 ms et TA/TE 0,40–0,43 (Schober 2006, Visser 2016, Yuchi 2023), PAP moyenne 14 mmHg
//! (Haskins 2005), PAP systolique 15–25 mmHg. Une perte de Bernoulli à l'anneau pulmonaire a été
//! essayée et écartée : elle raccourcissait l'éjection et élevait la PAP systolique.
use serde_json::{json, Value as Json};
use std::path::PathBuf;
use vetsim_core::{evaluate, Patient, PatientModule};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn main() {
    let a: Vec<f64> = std::env::args()
        .skip(1)
        .map(|x| x.parse().unwrap())
        .collect();
    let (c, z, l) = (a[0], a[1], a[2]);
    let mut pm: Json = serde_json::from_str(&read("modules/healthy_dog.json")).unwrap();
    pm["params"]["c_pa"] = json!({"mul": [c, "s"]});
    pm["params"]["z_pa"] = json!({"mul": [z, {"pow": ["s", -2.0 / 3.0]}]});
    pm["params"]["l_pa"] = json!({"mul": [l, {"pow": ["s", -1.0 / 3.0]}]});
    let patient = PatientModule::from_json(&pm.to_string(), "p").unwrap();
    let mut out = Vec::new();
    for bw in [5.0, 20.0, 40.0] {
        let pt = Patient::healthy(&patient, bw).unwrap();
        match evaluate(&pt.params) {
            Ok(ev) => {
                let w = &ev.hemo.waveforms;
                let dt = w.t[1] - w.t[0];
                let q = &w.q_pu;
                let qmax = q.iter().copied().fold(0.0, f64::max);
                let thr = 0.02 * qmax;
                // début de l'éjection : premier passage au-dessus du seuil ; fin : retour dessous
                let n = q.len();
                let i0 = (1..n).find(|&i| q[i - 1] <= thr && q[i] > thr).unwrap_or(0);
                let mut ip = i0;
                let mut i1 = i0;
                while i1 < n && q[i1] > thr {
                    if q[i1] > q[ip] {
                        ip = i1;
                    }
                    i1 += 1;
                }
                let mx = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max);
                let mn = |v: &[f64]| v.iter().copied().fold(f64::MAX, f64::min);
                let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
                let grad = w
                    .p_rv
                    .iter()
                    .zip(&w.p_pa)
                    .map(|(a, b)| a - b)
                    .fold(f64::MIN, f64::max);
                // anneau de diamètre 0,795·BW^⅓ (Brown 2003)
                let r_ao = 0.795 * vetsim_core::math::pow(bw, 1.0 / 3.0) / 2.0;
                out.push(json!({"bw": bw, "paps": mx(&w.p_pa), "papd": mn(&w.p_pa), "papm": mean(&w.p_pa),
                    "at": (ip - i0) as f64 * dt * 1000.0, "et": (i1 - i0) as f64 * dt * 1000.0,
                    "at_et": (ip - i0) as f64 / (i1 - i0).max(1) as f64,
                    "v_pu": qmax / (std::f64::consts::PI * r_ao * r_ao) / 100.0,
                    "grad_rv_pa": grad, "co": ev.hemo.summary.co, "map": ev.hemo.summary.map}));
            }
            Err(e) => out.push(json!({"bw": bw, "err": e.to_string()})),
        }
    }
    println!("{}", json!({"c": c, "z": z, "l": l, "res": out}));
}
