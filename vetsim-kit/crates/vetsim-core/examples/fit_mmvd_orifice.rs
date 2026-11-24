//! Calage de l'orifice régurgitant mitral (loi d'orifice) du module MMVD.
//!
//! `cargo run --release -p vetsim-core --example fit_mmvd_orifice -- A1 p k`
//! Remplace l'effet `a_mr` du module par `A1·s^p·z_g` et `mr_k` par `k`, puis affiche, pour
//! chaque sévérité, l'orifice effectif retenu (cm², 20 kg), la fraction régurgitée, la POG
//! moyenne, la FC, ou la cause de non-viabilité.
use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::{
    apply_drug_effects, evaluate, timecourse, Catalog, DiseaseModule, Dose, DrugModule, NoNoise,
    Patient, PatientModule, Route,
};

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
    let (a1, pe, k) = (a[0], a[1], a[2]);
    let amax = a.get(3).copied().unwrap_or(0.0);
    let mut pm: Json = serde_json::from_str(&read("modules/healthy_dog.json")).unwrap();
    pm["params"]["mr_k"] = serde_json::json!(k);
    pm["params"]["mr_amax"] = serde_json::json!(amax);
    let mut mm: Json = serde_json::from_str(&read("modules/diseases/mmvd.json")).unwrap();
    for e in mm["effects"].as_array_mut().unwrap() {
        if e["param"] == "a_mr" {
            e["expr"] = serde_json::json!({"mul": [a1, {"pow": ["s", pe]}, "z_g"]});
        }
    }
    let patient = PatientModule::from_json(&pm.to_string(), "p").unwrap();
    let d = DiseaseModule::from_json(&mm.to_string(), "mmvd").unwrap();
    let fu = DrugModule::from_json(&read("modules/drugs/furosemide.json"), "furosemide").unwrap();
    let c = Catalog::new(patient, vec![d], vec![fu]).unwrap();
    let mut out = Vec::new();
    for s in [0.175, 0.45, 0.725, 0.9, 1.0] {
        let mut pt = Patient::healthy(&c.patient, 20.0).unwrap();
        pt.add_disease(c.disease("mmvd").unwrap(), s, &mut NoNoise)
            .unwrap();
        match evaluate(&pt.params) {
            Ok(ev) => out.push(serde_json::json!({"s": s, "eroa": ev.params.a_mr,
                "rf": ev.hemo.summary.reg_fraction, "lap": ev.hemo.summary.la_mean,
                "hr": ev.params.hr, "edv": ev.hemo.summary.edv})),
            Err(e) => out.push(serde_json::json!({"s": s, "err": e.to_string()})),
        }
    }
    // Suzuki 2013 : furosémide 0,17 mg/kg/h × 6 h, IM expérimentale (s = 0,78) : POG −27 %
    let mut pt = Patient::healthy(&c.patient, 20.0).unwrap();
    pt.add_disease(c.disease("mmvd").unwrap(), 0.78, &mut NoNoise)
        .unwrap();
    let doses: Vec<Dose> = (0..12)
        .map(|i| Dose {
            t_h: f64::from(i) / 2.0,
            drug: "furosemide".into(),
            mg_per_kg: 0.085,
            route: Route::Iv,
        })
        .collect();
    let furo = (|| {
        let la0 = evaluate(&pt.params).ok()?.hemo.summary.la_mean;
        let tc = timecourse(&pt.params, &c.drugs, &doses, 5.9, &[5.9]).ok()?;
        let conc: Vec<(String, f64)> = tc.conc.iter().map(|(d, v)| (d.clone(), v[0])).collect();
        let q = apply_drug_effects(&pt.params, &c.drugs, &conc, tc.d[0], tc.u[0]).ok()?;
        Some(evaluate(&q).ok()?.hemo.summary.la_mean / la0)
    })();
    println!(
        "{}",
        serde_json::json!({"a1": a1, "p": pe, "k": k, "amax": amax, "furo": furo, "res": out})
    );
}
