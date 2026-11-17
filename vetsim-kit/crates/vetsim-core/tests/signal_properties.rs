//! Jalon M6 (partie statistique et qualitative), critères de docs/ROADMAP.md :
//! - cibles `hrv` dans ±1 ET publié, moyenne sur ≥ 20 graines ;
//! - propriétés qualitatives ECG/PCG : souffle présent ou absent, grade monotone avec la
//!   sévérité MMVD, T ample si K > 5,5, P absente si K > 7,8, B3 en surcharge de l'OG.

use std::path::PathBuf;
use vetsim_core::calibrate::{hrv_cohort, parse_hrv_targets};
use vetsim_core::signals::{patient_signals, Murmur, SignalOptions};
use vetsim_core::{
    evaluate, Catalog, DiseaseModule, DrugModule, Evaluation, NoNoise, Patient, PatientModule,
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

fn eval_with(c: &Catalog, disease: Option<(&str, f64)>) -> Evaluation {
    let mut pt = Patient::healthy(&c.patient, 20.0).unwrap();
    if let Some((d, s)) = disease {
        pt.add_disease(c.disease(d).unwrap(), s, &mut NoNoise)
            .unwrap();
    }
    evaluate(&pt.params).unwrap()
}

#[test]
fn hrv_targets_within_one_sd_over_20_seeds() {
    let c = catalog();
    let targets = parse_hrv_targets(&read("calibration/targets.json"), "targets.json").unwrap();
    let rows = hrv_cohort(&c, &targets, 20).unwrap();
    for r in &rows {
        eprintln!(
            "  {:<9} cible {:7.2} ± {:6.2}  modèle {:7.2} (ET {:.2})",
            r.target.var, r.target.target, r.target.tol, r.model_mean, r.model_sd
        );
        assert!(r.ok, "{} hors de ±1 ET : {}", r.target.var, r.model_mean);
        assert_eq!(r.n, 20);
    }
    assert_eq!(rows.len(), 4);
}

#[test]
fn signals_are_reproducible_and_seed_dependent() {
    let c = catalog();
    let ev = eval_with(&c, None);
    let o = SignalOptions::default();
    let a = patient_signals(&ev, &o, 9);
    assert_eq!(a, patient_signals(&ev, &o, 9));
    assert_ne!(a.ecg.v, patient_signals(&ev, &o, 10).ecg.v);
    assert_eq!(a.ecg.v.len(), 4000);
    assert!(a.ecg.r_times.iter().all(|t| *t < 8.0));
}

#[test]
fn ecg_r_peaks_follow_r_times() {
    let c = catalog();
    let ev = eval_with(&c, None);
    let s = patient_signals(&ev, &SignalOptions::default(), 4);
    let e = &s.ecg;
    for &tr in e.r_times.iter().filter(|t| **t > 0.1 && **t < 7.9) {
        // maximum local dans ±20 ms de chaque instant R, à moins de 4 ms près
        let (i0, i1) = (
            ((tr - 0.02) * 500.0) as usize,
            ((tr + 0.02) * 500.0) as usize,
        );
        let imax = (i0..i1).max_by(|a, b| e.v[*a].total_cmp(&e.v[*b])).unwrap();
        assert!(
            (e.t[imax] - tr).abs() < 0.004,
            "R à {tr}, pic à {}",
            e.t[imax]
        );
        assert!(e.v[imax] > 1.4, "amplitude de R {}", e.v[imax]);
    }
    // Les ondes R de l'ECG sont les activations ventriculaires de la conduction des décharges
    // sinusales de l'autonome : décharge + PR (rythme sinusal 1:1), sur [0, 8 s).
    assert!(!e.r_times.is_empty() && e.r_times.iter().all(|t| *t < 8.0));
    for (r, b) in e.r_times.iter().zip(&s.autonomic.beats) {
        assert!(
            (0.08..0.2).contains(&(r - b)),
            "R {r} / décharge sinusale {b}"
        );
    }
    // FC moyenne sur les 120 s de l'autonome proche de la FC de régime permanent (la RSA du
    // chien sain est ample : sur 8 s, la moyenne n'est pas stable).
    let fc = s.autonomic.hrv.unwrap().fc_moy;
    assert!(
        (fc - ev.params.hr).abs() / ev.params.hr < 0.10,
        "FC {fc} / {}",
        ev.params.hr
    );
}

#[test]
fn murmur_absent_when_healthy_and_grade_monotone_with_mmvd() {
    let c = catalog();
    let healthy = patient_signals(&eval_with(&c, None), &SignalOptions::default(), 1);
    assert_eq!(healthy.pcg.features.murmur, Murmur::None);
    assert_eq!(healthy.pcg.features.grade, 0);
    assert!(!healthy.pcg.features.s3);
    let mut prev = 0;
    let mut grades = Vec::new();
    for s in [0.1, 0.3, 0.45, 0.6, 0.75, 0.9, 1.0] {
        let f = patient_signals(
            &eval_with(&c, Some(("mmvd", s))),
            &SignalOptions::default(),
            1,
        )
        .pcg
        .features;
        grades.push((s, f.grade, f.murmur_peak_flow));
        assert!(f.grade >= prev, "grade non monotone : {grades:?}");
        prev = f.grade;
        if s >= 0.3 {
            assert_eq!(f.murmur, Murmur::HolosystolicLeftApical, "s = {s}");
        }
    }
    eprintln!("  grades MMVD (s, grade, pic de débit régurgitant) : {grades:?}");
    assert!(prev >= 5, "stade D : grade {prev}");
    // B3 en surcharge de l'OG (MMVD stade D : POG > 15 mmHg)
    let d = eval_with(&c, Some(("mmvd", 0.9)));
    assert!(d.hemo.summary.la_mean > 15.0);
    assert!(
        patient_signals(&d, &SignalOptions::default(), 1)
            .pcg
            .features
            .s3
    );
}

#[test]
fn hyperkalaemia_changes_the_ecg() {
    let c = catalog();
    let h = patient_signals(&eval_with(&c, None), &SignalOptions::default(), 2)
        .ecg
        .features;
    let crisis = eval_with(&c, Some(("addison", 0.9)));
    assert!(crisis.params.k > 5.5, "K = {}", crisis.params.k);
    let a = patient_signals(&crisis, &SignalOptions::default(), 2)
        .ecg
        .features;
    assert!(
        a.t_amp > h.t_amp && a.t_w < h.t_w,
        "T ample et pointue : {a:?} / {h:?}"
    );
    assert!(a.p_amp < h.p_amp, "P aplatie");
    assert!(a.qt < h.qt, "QT raccourci : {} / {}", a.qt, h.qt);
    // K ≥ k_atrial_standstill : arrêt atrial décidé par la conduction (ni onde P ni contraction),
    // conduction sino-ventriculaire conservée
    let mut pt = Patient::healthy(&c.patient, 20.0).unwrap();
    pt.params.k = 9.0;
    let cond =
        vetsim_core::rhythm::simulate_conduction(&pt.params, 0.6, 0.68, 10.0, &mut || 0.0, None);
    assert!(cond.atrial.is_empty());
    assert!(cond.ventricular.len() > 10);
    let p = vetsim_core::signals::ecg_features(&pt.params);
    assert!(p.qrs > h.qrs);
}
