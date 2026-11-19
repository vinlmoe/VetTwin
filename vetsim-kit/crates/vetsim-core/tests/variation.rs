//! Trajectoires Monte-Carlo d'un même animal (variation.rs) : propriétés.

use std::path::PathBuf;
use vetsim_core::{
    evaluate, sample_trajectory, Catalog, DiseaseModule, Dose, DrugModule, Patient, PatientModule,
    Route, Scale, SimRng, VariationConfig,
};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn catalog() -> Catalog {
    let patient = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let diseases = ["mmvd", "ckd"]
        .iter()
        .map(|d| DiseaseModule::from_json(&read(&format!("modules/diseases/{d}.json")), d).unwrap())
        .collect();
    let drugs = ["furosemide", "pimobendan"]
        .iter()
        .map(|d| DrugModule::from_json(&read(&format!("modules/drugs/{d}.json")), d).unwrap())
        .collect();
    Catalog::new(patient, diseases, drugs).unwrap()
}

fn config() -> VariationConfig {
    VariationConfig::from_json(&read("modules/variation.json"), "variation").unwrap()
}

const OFF: Scale = Scale {
    natural: 0.0,
    disease: 0.0,
    drug: 0.0,
    measurement: 0.0,
};

/// Animal avec variabilité interindividuelle et MRC tirée (tirages non triviaux).
fn ckd_dog(c: &Catalog, seed: u64) -> Patient {
    let mut rng = SimRng::new(seed);
    let mut pt = Patient::with_variability(&c.patient, 20.0, &mut rng).unwrap();
    pt.add_disease(c.disease("ckd").unwrap(), 0.5, &mut rng)
        .unwrap();
    pt
}

#[test]
fn module_loads() {
    let cfg = config();
    assert!(cfg.natural.iter().any(|t| t.param == "hr"));
    assert!(cfg.measurement.iter().any(|(k, _)| k == "creat"));
}

/// Sources coupées : la trajectoire reproduit l'animal à chaque instant, au bit près
/// (les tirages du cas sont rejoués à la même sévérité).
#[test]
fn zero_amplitude_reproduces_the_animal() {
    let c = catalog();
    let pt = ckd_dog(&c, 11);
    let cfg = config().scaled(OFF);
    let mods = [c.disease("ckd").unwrap()];
    let t: Vec<f64> = (0..10).map(|i| i as f64 * 12.0).collect();
    let tr = sample_trajectory(&pt, &mods, &c.drugs, &[], &t, &cfg, &mut SimRng::new(1)).unwrap();
    for p in &tr.params {
        assert_eq!(p, &pt.params);
    }
    assert!(tr.severity[0].iter().all(|s| *s == 0.5));
}

#[test]
fn same_seed_same_trajectory() {
    let c = catalog();
    let pt = ckd_dog(&c, 4);
    let mods = [c.disease("ckd").unwrap()];
    let t = [0.0, 1.0, 6.0, 24.0];
    let doses = [Dose {
        t_h: 0.0,
        drug: "furosemide".into(),
        mg_per_kg: 2.0,
        route: Route::Iv,
    }];
    let cfg = config();
    let a = sample_trajectory(&pt, &mods, &c.drugs, &doses, &t, &cfg, &mut SimRng::new(9)).unwrap();
    let b = sample_trajectory(&pt, &mods, &c.drugs, &doses, &t, &cfg, &mut SimRng::new(9)).unwrap();
    let d =
        sample_trajectory(&pt, &mods, &c.drugs, &doses, &t, &cfg, &mut SimRng::new(10)).unwrap();
    assert_eq!(a, b);
    assert_ne!(a.params, d.params);
    assert_ne!(a.conc, d.conc, "variabilité PK");
}

/// Fluctuation naturelle : les paramètres varient d'un instant à l'autre, leur moyenne sur de
/// nombreuses réalisations reste celle de l'animal (multiplicateur de moyenne 1), et deux
/// instants proches sont plus corrélés que deux instants éloignés (Ornstein-Uhlenbeck).
#[test]
fn natural_fluctuation_is_mean_preserving_and_autocorrelated() {
    let c = catalog();
    let pt = Patient::healthy(&c.patient, 20.0).unwrap();
    let cfg = config();
    let tau = cfg.natural.iter().find(|t| t.param == "hr").unwrap().tau_h;
    let t = [0.0, 0.1 * tau, 5.0 * tau];
    let n = 4000;
    let mut rng = SimRng::new(5);
    let (mut sum, mut near, mut far, mut var) = (0.0, 0.0, 0.0, 0.0);
    let hr0 = pt.params.hr;
    for _ in 0..n {
        let tr = sample_trajectory(&pt, &[], &c.drugs, &[], &t, &cfg, &mut rng).unwrap();
        let x: Vec<f64> = tr
            .params
            .iter()
            .map(|p| vetsim_core::math::ln(p.hr / hr0))
            .collect();
        sum += tr.params[0].hr;
        var += x[0] * x[0];
        near += x[0] * x[1];
        far += x[0] * x[2];
    }
    let mean = sum / n as f64;
    assert!((mean / hr0 - 1.0).abs() < 0.005, "moyenne {mean} vs {hr0}");
    assert!(near / var > 0.8, "corrélation à 0,1 τ : {}", near / var);
    assert!((far / var).abs() < 0.1, "corrélation à 5 τ : {}", far / var);
}

/// Progression : la sévérité suit la pente donnée ; la créatinine monte avec elle.
#[test]
fn progression_raises_severity_and_creatinine() {
    let c = catalog();
    let pt = ckd_dog(&c, 2);
    let mods = [c.disease("ckd").unwrap()];
    let cfg = config()
        .scaled(OFF)
        .with_progression(&[("ckd".to_string(), 0.01)]);
    let t = [0.0, 24.0 * 10.0, 24.0 * 30.0];
    let tr = sample_trajectory(&pt, &mods, &c.drugs, &[], &t, &cfg, &mut SimRng::new(0)).unwrap();
    let s = &tr.severity[0];
    assert!(
        (s[1] - 0.6).abs() < 1e-12 && (s[2] - 0.8).abs() < 1e-12,
        "{s:?}"
    );
    let creat: Vec<f64> = tr
        .params
        .iter()
        .map(|p| evaluate(p).unwrap().metabolic.chem.creat)
        .collect();
    assert!(creat[0] < creat[1] && creat[1] < creat[2], "{creat:?}");
}

#[test]
fn measurement_noise_is_mean_one() {
    let cfg = config();
    let mut rng = SimRng::new(8);
    let n = 50_000;
    let m: f64 = (0..n)
        .map(|_| cfg.measure("creat", 100.0, &mut rng))
        .sum::<f64>()
        / n as f64;
    assert!((m - 100.0).abs() < 0.1, "{m}");
    assert_eq!(cfg.measure("inconnu", 3.0, &mut rng), 3.0);
}
