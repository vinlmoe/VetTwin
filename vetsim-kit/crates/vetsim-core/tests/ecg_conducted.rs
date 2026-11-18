//! ECG construit depuis la conduction (`signals::make_ecg_conducted`) comparé à la référence
//! Python : même séquence de conduction (reconstruite depuis la fixture), bruit rejoué (`Replay`).
//! Fixture : `tests/fixtures/python_reference_ecg_conducted.json`
//! (`tests/make_ecg_conducted_fixture.py`). Contrôle d'échec : `shape_mutation_is_detected`.

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::rhythm::{AtrialEvent, Conduction, Origin, VentricularEvent};
use vetsim_core::rng::Replay;
use vetsim_core::signals::make_ecg_conducted;
use vetsim_core::{Params, Patient, PatientModule};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn floats(j: &Json) -> Vec<f64> {
    j.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn close(a: f64, b: f64, rtol: f64) -> bool {
    a == b || (a - b).abs() <= rtol * f64::max(a.abs(), b.abs()).max(1e-12)
}

fn origin(s: &str) -> Origin {
    match s {
        "sinus" => Origin::Sinus,
        "junctional" => Origin::Junctional,
        "ventricular_escape" => Origin::VentricularEscape,
        "ectopic" => Origin::Ectopic,
        o => panic!("origine inconnue {o}"),
    }
}

fn case_inputs(pm: &PatientModule, case: &Json) -> (Params, Conduction) {
    let mut p = Patient::healthy(pm, 20.0).unwrap().params;
    for (k, v) in case["over"].as_object().unwrap() {
        *p.get_mut(k).unwrap() = v.as_f64().unwrap();
    }
    let cond = Conduction {
        atrial: case["atrial"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| AtrialEvent {
                t: a[0].as_f64().unwrap(),
                organized: a[1].as_bool().unwrap(),
            })
            .collect(),
        ventricular: case["ventricular"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| VentricularEvent {
                t: b[0].as_f64().unwrap(),
                origin: origin(b[1].as_str().unwrap()),
                t_a: b[2].as_f64(),
                pr: b[3].as_f64(),
                wide: b[4].as_bool().unwrap(),
            })
            .collect(),
    };
    (p, cond)
}

/// Écarts entre le tracé Rust et la fixture (1 valeur sur `stride`, somme, somme des |x|).
fn compare(fx: &Json, mutate: impl Fn(&mut Params)) -> Vec<String> {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let stride = fx["stride"].as_u64().unwrap() as usize;
    let mut bad = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let (mut p, cond) = case_inputs(&pm, case);
        mutate(&mut p);
        let mut rp = Replay::new(floats(&case["noise"]));
        let e = make_ecg_conducted(&p, &cond, case["hr"].as_f64().unwrap(), 8.0, 500.0, &mut rp);
        let f = &case["features"];
        let fe = e.features;
        for (k, g) in [
            ("pr", fe.pr),
            ("qrs", fe.qrs),
            ("qt", fe.qt),
            ("p_amp", fe.p_amp),
            ("t_amp", fe.t_amp),
            ("r_amp", fe.r_amp),
            ("t_w", fe.t_w),
            ("p_w", fe.p_w),
        ] {
            if !close(g, f[k].as_f64().unwrap(), 1e-12) {
                bad.push(format!("{id} {k} : Rust {g} / Python {}", f[k]));
            }
        }
        let s = &case["sig"];
        if e.v.len() != s["n"].as_u64().unwrap() as usize {
            bad.push(format!("{id} : longueur {}", e.v.len()));
            continue;
        }
        for (i, x) in floats(&s["every"]).iter().enumerate() {
            let g = e.v[i * stride];
            if !close(g, *x, 1e-9) && (g - x).abs() > 1e-12 {
                bad.push(format!("{id}[{}] : Rust {g} / Python {x}", i * stride));
                break;
            }
        }
        let sum: f64 = e.v.iter().sum();
        let sa: f64 = e.v.iter().map(|x| x.abs()).sum();
        for (n, g, x) in [
            ("somme", sum, s["sum"].as_f64().unwrap()),
            ("somme |x|", sa, s["sum_abs"].as_f64().unwrap()),
        ] {
            if !close(g, x, 1e-9) && (g - x).abs() > 1e-9 {
                bad.push(format!("{id} {n} : Rust {g} / Python {x}"));
            }
        }
    }
    bad
}

fn fixture() -> Json {
    serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_ecg_conducted.json",
    ))
    .unwrap()
}

#[test]
fn ecg_from_conduction_matches_python() {
    let fx = fixture();
    assert_eq!(fx["cases"].as_array().unwrap().len(), 8);
    let bad = compare(&fx, |_| {});
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn shape_mutation_is_detected() {
    // Contrôle d'échec : un QRS 5 % plus large ou un cycle de FA différent doit être rejeté.
    let fx = fixture();
    assert!(!compare(&fx, |p| p.qrs *= 1.05).is_empty());
    let bad = compare(&fx, |p| p.af_cycle *= 1.1);
    assert!(
        bad.iter().any(|l| l.starts_with("fa")),
        "ondes f insensibles au cycle"
    );
    assert!(bad.iter().all(|l| l.starts_with("fa")), "{bad:?}");
}
