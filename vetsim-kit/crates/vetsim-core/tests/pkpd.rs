//! Jalon M4 : PK/PD, effets des médicaments, diurèse.
//! Référence stricte : `tests/fixtures/python_reference_m4.json` (`tests/make_m4_fixture.py`).
//! La conformité golden (cas `pk_*` et `*_<médicament>`) est vérifiée par
//! `python tests/conformance.py --cmd target/release/vetsim-cli`.

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::{
    apply_drug_effects, timecourse, Catalog, DiseaseModule, Dose, DrugModule, Error, NoNoise,
    Patient, PatientModule, Route,
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

fn fixture() -> Json {
    serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_m4.json",
    ))
    .unwrap()
}

fn patient(c: &Catalog, case: &Json) -> Patient {
    let mut pt = Patient::healthy(&c.patient, case["bw"].as_f64().unwrap()).unwrap();
    for d in case["diseases"].as_array().unwrap() {
        let m = c.disease(d[0].as_str().unwrap()).unwrap();
        pt.add_disease(m, d[1].as_f64().unwrap(), &mut NoNoise)
            .unwrap();
    }
    pt
}

fn isclose(a: f64, b: f64, rtol: f64, atol: f64) -> bool {
    a == b || (a - b).abs() <= f64::max(rtol * f64::max(a.abs(), b.abs()), atol)
}

#[test]
fn drug_effects_match_python_exactly() {
    let c = catalog();
    let fx = fixture();
    let mut bad = Vec::new();
    let mut n = 0;
    for case in fx["effects"].as_array().unwrap() {
        let pt = patient(&c, case);
        let conc: Vec<(String, f64)> = case["conc"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
            .collect();
        let (d, u) = (case["D"].as_f64().unwrap(), case["U"].as_f64().unwrap());
        let q = apply_drug_effects(&pt.params, &c.drugs, &conc, d, u).unwrap();
        for (k, v) in case["params"].as_object().unwrap() {
            let (got, e) = (q.get(k).unwrap(), v.as_f64().unwrap());
            if !isclose(got, e, 1e-12, 1e-15) {
                bad.push(format!("{conc:?} D={d} U={u} {k}: Rust {got} / Python {e}"));
            }
            n += 1;
        }
    }
    eprintln!("M4 effets : {}/{n} paramètres à 1e-12", n - bad.len());
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn timecourses_match_python() {
    let c = catalog();
    let fx = fixture();
    let (rtol, atol) = (1e-6, 1e-9);
    let mut bad = Vec::new();
    let (mut n, mut worst) = (0, (0.0f64, String::new()));
    for case in fx["timecourses"].as_array().unwrap() {
        let pt = patient(&c, case);
        let doses: Vec<Dose> = case["doses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| Dose {
                t_h: d[0].as_f64().unwrap(),
                drug: d[1].as_str().unwrap().to_string(),
                mg_per_kg: d[2].as_f64().unwrap(),
                route: if d[3] == "iv" { Route::Iv } else { Route::Po },
            })
            .collect();
        let t_eval: Vec<f64> = case["t_eval"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_f64().unwrap())
            .collect();
        let tc = timecourse(
            &pt.params,
            &c.drugs,
            &doses,
            case["t_end"].as_f64().unwrap(),
            &t_eval,
        )
        .unwrap();
        let label = format!("{} kg {:?}", case["bw"], case["doses"]);
        for (key, exp) in case["expected"].as_object().unwrap() {
            let got: &[f64] = match key.as_str() {
                "t" => &tc.t,
                "D" => &tc.d,
                "U" => &tc.u,
                drug => tc.conc_of(drug).unwrap(),
            };
            let exp = exp.as_array().unwrap();
            assert_eq!(got.len(), exp.len(), "{label} {key} : longueur");
            for (i, (g, e)) in got.iter().zip(exp).enumerate() {
                let e = e.as_f64().unwrap();
                n += 1;
                let rel = (g - e).abs() / e.abs().max(atol);
                if rel > worst.0 {
                    worst = (rel, format!("{label} {key}[{i}]"));
                }
                if !isclose(*g, e, rtol, atol) {
                    bad.push(format!(
                        "{label} {key}[{i}] t={}: Rust {g} / Python {e}",
                        tc.t[i]
                    ));
                }
            }
        }
    }
    eprintln!(
        "M4 cinétiques : {}/{n} valeurs (rel {rtol}, abs {atol}) ; écart relatif max {:.1e} ({})",
        n - bad.len(),
        worst.0,
        worst.1
    );
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn pk_and_pd_errors_are_typed() {
    let c = catalog();
    let pt = Patient::healthy(&c.patient, 20.0).unwrap();
    let dose = |drug: &str, mg: f64, t: f64| Dose {
        t_h: t,
        drug: drug.into(),
        mg_per_kg: mg,
        route: Route::Iv,
    };
    assert_eq!(
        timecourse(
            &pt.params,
            &c.drugs,
            &[dose("aspirine", 1.0, 0.0)],
            1.0,
            &[0.0]
        )
        .unwrap_err(),
        Error::UnknownDrug("aspirine".into())
    );
    assert!(matches!(
        timecourse(
            &pt.params,
            &c.drugs,
            &[dose("furosemide", -1.0, 0.0)],
            1.0,
            &[0.0]
        ),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        timecourse(
            &pt.params,
            &c.drugs,
            &[dose("furosemide", 1.0, f64::NAN)],
            1.0,
            &[0.0]
        ),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        apply_drug_effects(&pt.params, &c.drugs, &[("x".into(), 1.0)], 0.0, 0.0).unwrap_err(),
        Error::UnknownDrug("x".into())
    );
    // D/U sans module diurétique.
    let no_diuretic: Vec<DrugModule> = c
        .drugs
        .iter()
        .filter(|d| d.id != "furosemide")
        .cloned()
        .collect();
    assert!(matches!(
        apply_drug_effects(&pt.params, &no_diuretic, &[], 100.0, 0.0),
        Err(Error::InvalidInput(_))
    ));
    // Sans médicament ni D/U : paramètres inchangés.
    assert_eq!(
        apply_drug_effects(&pt.params, &c.drugs, &[], 0.0, 0.0).unwrap(),
        pt.params
    );
}
