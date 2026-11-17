//! Jalon M2 : gaz du sang, acide-base de Stewart, Hill/Bohr, rein, lactate (en algébrique).
//!
//! 1. Critère de la feuille de route : sur le golden, sorties gaz et chimie conformes (tolérances
//!    de `tests/golden/cases.json`) quand on injecte l'hémodynamique de référence (`co`, `map`,
//!    `la_mean`) — cas `eval` sans médicament.
//! 2. Test strict : `tests/fixtures/python_reference_m2.json` (physio.evaluate avec hémodynamique
//!    imposée, 125 cas dont profils extrêmes) à rel 1e-9.

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::{
    metabolic_state, Catalog, DiseaseModule, DrugModule, HemoInputs, NoNoise, Patient,
    PatientModule,
};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn catalog() -> Catalog {
    let patient = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let diseases = ["mmvd", "ckd", "addison", "pneumonia"]
        .iter()
        .map(|d| DiseaseModule::from_json(&read(&format!("modules/diseases/{d}.json")), d).unwrap())
        .collect();
    let drugs = [
        "atropine",
        "betabloquant",
        "furosemide",
        "opioide",
        "pimobendan",
    ]
    .iter()
    .map(|d| DrugModule::from_json(&read(&format!("modules/drugs/{d}.json")), d).unwrap())
    .collect();
    Catalog::new(patient, diseases, drugs).unwrap()
}

fn patient(c: &Catalog, bw: f64, diseases: &[(String, f64)]) -> Patient {
    let mut pt = Patient::healthy(&c.patient, bw).unwrap();
    for (id, s) in diseases {
        pt.add_disease(c.disease(id).unwrap(), *s, &mut NoNoise)
            .unwrap();
    }
    pt
}

/// `math.isclose`.
fn isclose(a: f64, b: f64, rtol: f64, atol: f64) -> bool {
    a == b || (a - b).abs() <= f64::max(rtol * f64::max(a.abs(), b.abs()), atol)
}

#[test]
fn golden_gas_and_chemistry_with_reference_hemodynamics() {
    let c = catalog();
    let g: Json = serde_json::from_str(&read("tests/golden/cases.json")).unwrap();
    let tol = &g["tolerances"];
    let (drt, dat) = (
        tol["default_rtol"].as_f64().unwrap(),
        tol["default_atol"].as_f64().unwrap(),
    );
    let (mut total, mut bad, mut cases) = (0, Vec::new(), 0);
    let mut worst: (f64, String) = (0.0, String::new());
    for case in g["cases"].as_array().unwrap() {
        let args: Vec<&str> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        // Cas avec médicament : PD au jalon M4.
        if args[0] != "eval" || args.iter().any(|a| ["--conc", "--D", "--U"].contains(a)) {
            continue;
        }
        let mut bw = 20.0;
        let mut diseases = Vec::new();
        for w in args.windows(2) {
            match w[0] {
                "--bw" => bw = w[1].parse().unwrap(),
                "--disease" => {
                    let (id, s) = w[1].split_once(':').unwrap();
                    diseases.push((id.to_string(), s.parse().unwrap()));
                }
                _ => {}
            }
        }
        let exp = &case["expected"];
        let h = HemoInputs {
            co: exp["co"].as_f64().unwrap(),
            map: exp["map"].as_f64().unwrap(),
            la_mean: exp["la_mean"].as_f64().unwrap(),
        };
        let pt = patient(&c, bw, &diseases);
        let m = metabolic_state(&pt.params, &h).unwrap();
        for (k, got) in m.to_flat() {
            let e = exp[k].as_f64().unwrap();
            let t = &tol["per_key"][k];
            let rt = t["rtol"].as_f64().unwrap_or(drt);
            let at = t["atol"].as_f64().unwrap_or(dat);
            total += 1;
            if !isclose(got, e, rt, at) {
                bad.push(format!("{} {k}: Rust {got} / golden {e}", case["id"]));
            }
            let rel = (got - e).abs() / e.abs().max(1e-300);
            if rel > worst.0 {
                worst = (rel, format!("{} {k}", case["id"]));
            }
        }
        cases += 1;
    }
    eprintln!(
        "M2 golden : {}/{total} valeurs conformes sur {cases} cas ; plus grand écart relatif {:.2e} ({})",
        total - bad.len(),
        worst.0,
        worst.1
    );
    assert_eq!(cases, 15);
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn metabolic_state_matches_python_reference_strictly() {
    let c = catalog();
    let fx: Json = serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_m2.json",
    ))
    .unwrap();
    let (mut total, mut bad) = (0, Vec::new());
    let mut worst: (f64, String) = (0.0, String::new());
    for case in fx["cases"].as_array().unwrap() {
        let bw = case["bw"].as_f64().unwrap();
        let diseases: Vec<(String, f64)> = case["diseases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| (d[0].as_str().unwrap().to_string(), d[1].as_f64().unwrap()))
            .collect();
        let mut pt = patient(&c, bw, &diseases);
        for (k, v) in case["overrides"].as_object().unwrap() {
            *pt.params.get_mut(k).unwrap() = v.as_f64().unwrap();
        }
        let hm = &case["hemo"];
        let h = HemoInputs {
            co: hm["co"].as_f64().unwrap(),
            map: hm["map"].as_f64().unwrap(),
            la_mean: hm["la_mean"].as_f64().unwrap(),
        };
        let label = format!(
            "{bw} kg {:?} {} {}",
            diseases, case["overrides"], case["hemo_profile"]
        );
        let m = metabolic_state(&pt.params, &h).unwrap();
        let exp = &case["expected"];
        for (k, got) in m.to_flat() {
            let e = exp[k].as_f64().unwrap();
            total += 1;
            if !isclose(got, e, 1e-9, 1e-12) {
                bad.push(format!("{label} {k}: Rust {got} / Python {e}"));
            }
            let rel = (got - e).abs() / e.abs().max(1e-300);
            if rel > worst.0 {
                worst = (rel, format!("{label} {k}"));
            }
        }
    }
    eprintln!(
        "M2 strict : {}/{total} valeurs à 1e-9 ; plus grand écart relatif {:.2e} ({})",
        total - bad.len(),
        worst.0,
        worst.1
    );
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn invalid_hemodynamics_are_refused() {
    let c = catalog();
    let pt = patient(&c, 20.0, &[]);
    for h in [
        HemoInputs {
            co: 0.0,
            map: 100.0,
            la_mean: 6.0,
        },
        HemoInputs {
            co: f64::NAN,
            map: 100.0,
            la_mean: 6.0,
        },
        HemoInputs {
            co: 3.0,
            map: f64::INFINITY,
            la_mean: 6.0,
        },
    ] {
        assert!(matches!(
            metabolic_state(&pt.params, &h),
            Err(vetsim_core::Error::InvalidInput(_))
        ));
    }
}
