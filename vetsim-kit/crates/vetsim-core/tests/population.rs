//! Jalon M5 : population virtuelle (graine explicite) et banc de calibration.
//! La comparaison statistique avec `calibration/calibration_run_python.json` est faite par
//! `python tests/compare_calibration.py --cmd target/release/vetsim-cli`.

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::calibrate::{disease_checks, healthy_cohort, parse_targets, pk_checks};
use vetsim_core::{
    Catalog, DiseaseModule, DrugModule, Error, NoNoise, Patient, PatientModule, SimRng,
};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn catalog() -> Catalog {
    let patient = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let diseases = [
        "mmvd",
        "ckd",
        "addison",
        "pneumonia",
        "atrial_fibrillation",
        "av_block",
        "ventricular_premature",
        "sick_sinus",
        "pulmonary_hypertension",
    ]
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

#[test]
fn same_seed_same_patient_and_case() {
    let c = catalog();
    let mmvd = c.disease("mmvd").unwrap();
    let make = |seed| {
        let mut rng = SimRng::new(seed);
        let mut pt = Patient::with_variability(&c.patient, 20.0, &mut rng).unwrap();
        pt.add_disease_staged(mmvd, None, &mut rng).unwrap();
        pt
    };
    assert_eq!(make(7), make(7));
    assert_ne!(make(7).params, make(8).params);
    // Les paramètres sans variabilité ne bougent pas.
    let healthy = Patient::healthy(&c.patient, 20.0).unwrap().params;
    let varied = Patient::with_variability(&c.patient, 20.0, &mut SimRng::new(3))
        .unwrap()
        .params;
    assert_eq!(varied.map_set, healthy.map_set);
    assert_ne!(varied.hr, healthy.hr);
}

#[test]
fn variability_has_mean_one_and_module_cv() {
    let c = catalog();
    let base = Patient::healthy(&c.patient, 20.0).unwrap().params;
    let n = 4000;
    let mut rng = SimRng::new(1);
    let ratios: Vec<f64> = (0..n)
        .map(|_| {
            Patient::with_variability(&c.patient, 20.0, &mut rng)
                .unwrap()
                .params
                .alt
                / base.alt
        })
        .collect();
    let m = ratios.iter().sum::<f64>() / n as f64;
    let cv = (ratios.iter().map(|r| (r - m) * (r - m)).sum::<f64>() / n as f64).sqrt() / m;
    // alt : sd lognormal 0,25 dans healthy_dog.json → CV = √(e^{σ²} − 1) ≈ 0,254
    let expected_cv = (vetsim_core::math::exp(0.25 * 0.25) - 1.0).sqrt();
    assert!((m - 1.0).abs() < 0.015, "moyenne {m}");
    assert!(
        (cv - expected_cv).abs() < 0.015,
        "CV {cv} (attendu {expected_cv})"
    );
}

#[test]
fn stage_sampling_follows_bounds_and_prior() {
    let c = catalog();
    let ckd = c.disease("ckd").unwrap();
    let mut rng = SimRng::new(11);
    let n = 20_000;
    let mut counts = vec![0usize; ckd.stages.len()];
    for _ in 0..n {
        let case = ckd.sample(None, &mut rng).unwrap();
        let i = ckd
            .stages
            .iter()
            .position(|s| Some(&s.name) == case.stage.as_ref())
            .unwrap();
        let st = &ckd.stages[i];
        assert!(st.lo <= case.severity && case.severity < st.hi);
        counts[i] += 1;
    }
    let total: f64 = ckd.stages.iter().map(|s| s.prior).sum();
    for (st, k) in ckd.stages.iter().zip(&counts) {
        let f = *k as f64 / n as f64;
        assert!(
            (f - st.prior / total).abs() < 0.012,
            "{} : {f} contre {}",
            st.name,
            st.prior
        );
    }
    // Stade imposé ; stade inconnu refusé ; mode sans bruit = milieu du stade.
    let case = ckd.sample(Some("IRIS3"), &mut rng).unwrap();
    assert!((0.5..0.75).contains(&case.severity));
    assert!(matches!(
        ckd.sample(Some("IRIS9"), &mut rng),
        Err(Error::UnknownStage { .. })
    ));
    let case = ckd.sample(Some("IRIS2"), &mut NoNoise).unwrap();
    assert_eq!(case.severity, 0.375);
    assert_eq!(
        ckd.sample(None, &mut NoNoise).unwrap().stage.as_deref(),
        Some("IRIS2")
    );
}

#[test]
fn pk_checks_match_python_table() {
    let c = catalog();
    let py: Json = serde_json::from_str(&read("calibration/calibration_run_python.json")).unwrap();
    let rows = pk_checks(&c).unwrap();
    let ref_rows = py["pk"].as_array().unwrap();
    assert_eq!(rows.len(), ref_rows.len());
    for (r, q) in rows.iter().zip(ref_rows) {
        assert_eq!(r.module, q["module"].as_str().unwrap());
        assert_eq!(r.var, q["var"].as_str().unwrap());
        assert_eq!(r.target, q["target"].as_f64().unwrap(), "{} : cible", r.var);
        let e = q["model_mean"].as_f64().unwrap();
        assert!(
            ((r.model - e) / e).abs() < 1e-4,
            "{} {} : Rust {} / Python {e}",
            r.module,
            r.var,
            r.model
        );
    }
}

#[test]
fn bench_is_deterministic_and_targets_load() {
    let c = catalog();
    let targets = parse_targets(&read("calibration/targets.json"), "targets.json").unwrap();
    assert_eq!(targets.len(), 21);
    let a = healthy_cohort(&c, &targets, 3).unwrap();
    let b = healthy_cohort(&c, &targets, 3).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        disease_checks(&c, 1, 5).unwrap(),
        disease_checks(&c, 1, 5).unwrap()
    );
    // Libellé inconnu refusé avec son chemin.
    let bad = r#"{"healthy": [{"module": "X", "var": "Glycémie", "target": 5, "tol": 1}]}"#;
    let err = parse_targets(bad, "t").unwrap_err().to_string();
    assert!(err.contains("/healthy/0/var"), "{err}");
}
