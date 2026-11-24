//! Calage conjoint de la compliance aortique, de l'impédance caractéristique et de l'inertance.
//!
//! `cargo run --release -p vetsim-core --example fit_arterial -- c z l` : coefficients à 20 kg de
//! `c_ao` (mL/mmHg), `z_ao` (mmHg·s/mL) et `l_ao` (mmHg·s²/mL), avec leurs allométries
//! (s, s^(−2/3), s^(−1/3)). Affiche, à 5, 20 et 40 kg : PAM, PAS, PAD, pression pulsée, débit,
//! FE, vitesse aortique maximale (débit maximal / aire de l'anneau, Brown 2003).
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
    pm["params"]["c_ao"] = json!({"mul": [c, "s"]});
    pm["params"]["z_ao"] = json!({"mul": [z, {"pow": ["s", -2.0 / 3.0]}]});
    pm["params"]["l_ao"] = json!({"mul": [l, {"pow": ["s", -1.0 / 3.0]}]});
    let patient = PatientModule::from_json(&pm.to_string(), "p").unwrap();
    let mut out = Vec::new();
    for bw in [5.0, 20.0, 40.0] {
        let pt = Patient::healthy(&patient, bw).unwrap();
        match evaluate(&pt.params) {
            Ok(ev) => {
                let s = &ev.hemo.summary;
                let qmax = ev.hemo.waveforms.q_ao.iter().copied().fold(0.0, f64::max);
                let d = 0.795 * vetsim_core::math::pow(bw, 1.0 / 3.0);
                let area = std::f64::consts::PI * d * d / 4.0;
                out.push(json!({"bw": bw, "map": s.map, "sap": s.sap, "dap": s.dap,
                    "pp": s.sap - s.dap, "co": s.co, "ef": s.ef, "vao": qmax / area, "hr": ev.params.hr}));
            }
            Err(e) => out.push(json!({"bw": bw, "err": e.to_string()})),
        }
    }
    println!("{}", json!({"c": c, "z": z, "l": l, "res": out}));
}
