//! Module auscultation, étape (b) : propriétés exigées par PROMPT_AUSCULTATION.md.
//! Les critères spectraux et B1/B2 sont mesurés par `pcg_metrics.py` dans
//! `tests/pcg_acceptance.py` (rapport : docs/PCG_CONFORMITE.md).

use std::path::PathBuf;
use vetsim_core::autonomic::{simulate_autonomic, AutonomicOptions};
use vetsim_core::pcg::{patient_pcg, AuscultationConfig, PcgOptions};
use vetsim_core::rng::SimRng;
use vetsim_core::signals::{streams, Murmur};
use vetsim_core::{evaluate, DiseaseModule, Evaluation, NoNoise, Patient, PatientModule};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn cfg() -> AuscultationConfig {
    AuscultationConfig::from_json(&read("modules/auscultation.json"), "a").unwrap()
}

fn dog(hr: Option<f64>, mmvd: Option<f64>) -> Evaluation {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let mut pt = Patient::healthy(&pm, 20.0).unwrap();
    if let Some(s) = mmvd {
        let m = DiseaseModule::from_json(&read("modules/diseases/mmvd.json"), "m").unwrap();
        pt.add_disease(&m, s, &mut NoNoise).unwrap();
    }
    if let Some(h) = hr {
        pt.params.hr = h;
    }
    evaluate(&pt.params).unwrap()
}

#[test]
fn signal_is_finite_normalised_and_exact_length() {
    let c = cfg();
    for hr in [72.0, 90.0] {
        let ev = dog(Some(hr), None);
        for seed in 0..20 {
            let r = patient_pcg(&ev, &c, &PcgOptions::default(), seed).unwrap();
            assert_eq!(r.samples.len(), 32_000, "8 s × 4000 Hz");
            assert!(r.samples.iter().all(|x| x.is_finite()), "NaN ou infini");
            let peak = r.samples.iter().map(|x| x.abs()).fold(0.0, f64::max);
            assert!((peak - 1.0).abs() < 1e-12, "crête {peak}");
            // pas de saturation : une seule crête à pleine échelle après quantification 16 bits
            let full = r
                .samples
                .iter()
                .filter(|x| (x.abs() * 32767.0).round() >= 32767.0)
                .count();
            assert!(full <= 2, "{full} échantillons à pleine échelle");
        }
    }
    // autre fréquence d'échantillonnage et durée
    let r = patient_pcg(
        &dog(None, None),
        &c,
        &PcgOptions {
            fs: Some(8000.0),
            duration_s: Some(5.0),
            ..PcgOptions::default()
        },
        3,
    )
    .unwrap();
    assert_eq!(r.samples.len(), 40_000);
}

#[test]
fn deterministic_for_a_seed_and_seed_dependent() {
    let c = cfg();
    let ev = dog(None, None);
    let a = patient_pcg(&ev, &c, &PcgOptions::default(), 7).unwrap();
    let b = patient_pcg(&ev, &c, &PcgOptions::default(), 7).unwrap();
    let d = patient_pcg(&ev, &c, &PcgOptions::default(), 8).unwrap();
    assert_eq!(a, b);
    assert_ne!(a.samples, d.samples);
}

#[test]
fn beats_are_those_of_the_autonomic_model() {
    // L'écart-type des RR est celui du module autonome : le module audio ne le modifie pas.
    let c = cfg();
    let ev = dog(Some(90.0), None);
    let au = simulate_autonomic(
        &(&ev).into(),
        &AutonomicOptions {
            duration_s: 120.0,
            ..AutonomicOptions::default()
        },
        &mut SimRng::for_stream(5, streams::AUTONOMIC),
    );
    let expected: Vec<f64> = au
        .beats
        .iter()
        .copied()
        .filter(|b| *b < 8.0 - c.end_margin_s)
        .collect();
    let r = patient_pcg(&ev, &c, &PcgOptions::default(), 5).unwrap();
    assert_eq!(r.beats, expected);
}

#[test]
fn mild_mmvd_has_a_visible_holosystolic_murmur_and_s1_not_below_healthy() {
    let c = cfg();
    let healthy = patient_pcg(&dog(None, None), &c, &PcgOptions::default(), 1).unwrap();
    let mmvd = patient_pcg(&dog(None, Some(0.3)), &c, &PcgOptions::default(), 1).unwrap();
    assert_eq!(healthy.features.murmur, Murmur::None);
    assert_eq!(healthy.features.grade, 0);
    assert_eq!(mmvd.features.murmur, Murmur::HolosystolicLeftApical);
    assert!(mmvd.features.grade >= 2, "grade {}", mmvd.features.grade);
    assert!(
        mmvd.features.s1 >= healthy.features.s1,
        "B1 {} < {}",
        mmvd.features.s1,
        healthy.features.s1
    );
    // « visible » : en systole, l'énergie du signal entre B1 et B2 dépasse nettement celle du sain
    let systolic_rms = |r: &vetsim_core::pcg::PcgResult| {
        let mut acc = (0.0, 0usize);
        for b in &r.beats {
            let (i0, i1) = (
                ((b + r.t_mc + 0.06) * r.fs) as usize,
                ((b + r.t_ac - 0.02) * r.fs) as usize,
            );
            for x in &r.samples[i0.min(r.samples.len())..i1.min(r.samples.len())] {
                acc.0 += x * x;
                acc.1 += 1;
            }
        }
        (acc.0 / acc.1 as f64).sqrt()
    };
    let (h, m) = (systolic_rms(&healthy), systolic_rms(&mmvd));
    assert!(m > 3.0 * h, "énergie systolique MMVD {m} / sain {h}");
}

#[test]
fn apex_site_inverts_s1_s2_and_unknown_site_is_refused() {
    let c = cfg();
    let ev = dog(None, None);
    let base = patient_pcg(&ev, &c, &PcgOptions::default(), 1)
        .unwrap()
        .features;
    let apex = patient_pcg(
        &ev,
        &c,
        &PcgOptions {
            site: Some("apex".into()),
            ..PcgOptions::default()
        },
        1,
    )
    .unwrap()
    .features;
    assert!(
        base.s2 > base.s1 && apex.s1 > apex.s2,
        "base {base:?} / apex {apex:?}"
    );
    assert!(patient_pcg(
        &ev,
        &c,
        &PcgOptions {
            site: Some("lune".into()),
            ..PcgOptions::default()
        },
        1
    )
    .is_err());
}
