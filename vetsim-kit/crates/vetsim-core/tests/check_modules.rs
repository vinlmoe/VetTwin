//! Port Rust de `tests/check_modules.py` (critère du jalon M1) : le moteur Rust, lisant les
//! modules JSON, reproduit le Python secondaire à 1e-9 près.
//! Référence : `tests/fixtures/python_reference.json`, produite par `tests/make_m1_fixture.py`.

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::dsl::{Env, Expr, Value};
use vetsim_core::{
    healthy_params, Catalog, DiseaseModule, DrugModule, EvalError, NoNoise, Params, Patient,
    PatientModule,
};

fn kit_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(kit_root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn catalog() -> Catalog {
    let patient = PatientModule::from_json(&read("modules/healthy_dog.json"), "healthy_dog.json")
        .unwrap_or_else(|e| panic!("{e}"));
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
    .map(|d| {
        let f = format!("modules/diseases/{d}.json");
        DiseaseModule::from_json(&read(&f), &f).unwrap_or_else(|e| panic!("{e}"))
    })
    .collect();
    let drugs = [
        "atropine",
        "betabloquant",
        "furosemide",
        "opioide",
        "pimobendan",
    ]
    .iter()
    .map(|d| {
        let f = format!("modules/drugs/{d}.json");
        DrugModule::from_json(&read(&f), &f).unwrap_or_else(|e| panic!("{e}"))
    })
    .collect();
    Catalog::new(patient, diseases, drugs).unwrap()
}

fn fixture() -> Json {
    serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference.json",
    ))
    .unwrap()
}

/// `math.isclose(a, b, rel_tol=1e-9, abs_tol=1e-12)`, comme check_modules.py.
fn isclose(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= f64::max(1e-9 * f64::max(a.abs(), b.abs()), 1e-12)
}

fn assert_params(label: &str, got: &Params, exp: &serde_json::Map<String, Json>) {
    let mut bad = Vec::new();
    for (k, v) in exp {
        let e = v.as_f64().unwrap();
        match got.get(k) {
            None => bad.push(format!("{k}: absent du Rust")),
            Some(g) if !isclose(g, e) => bad.push(format!("{k}: Rust {g} / Python {e}")),
            _ => {}
        }
    }
    for n in Params::NAMES {
        if !exp.contains_key(*n) {
            bad.push(format!("{n}: absent du Python"));
        }
    }
    assert!(
        bad.is_empty(),
        "{label} : {} écarts\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn all_modules_load() {
    let c = catalog();
    assert_eq!(c.diseases.len(), 9); // + hypertension pulmonaire pré-capillaire (24/11/2026)
    assert_eq!(c.drugs.len(), 5);
    assert_eq!(c.patient.params.len(), Params::NAMES.len());
}

#[test]
fn healthy_patient_matches_python() {
    let c = catalog();
    let fx = fixture();
    let healthy = fx["healthy"].as_object().unwrap();
    for (bw, exp) in healthy {
        let bw: f64 = bw.parse().unwrap();
        let p = healthy_params(&c.patient, bw).unwrap();
        assert_params(
            &format!("patient sain {bw} kg"),
            &p,
            exp.as_object().unwrap(),
        );
    }
    // Les trois poids du critère M1 sont bien couverts.
    for bw in ["5.0", "20.0", "40.0"] {
        assert!(healthy.contains_key(bw));
    }
}

#[test]
fn disease_modifiers_match_python() {
    let c = catalog();
    let fx = fixture();
    let mut n = 0;
    for entry in fx["disease_mods"].as_array().unwrap() {
        let id = entry["disease"].as_str().unwrap();
        let s = entry["severity"].as_f64().unwrap();
        let case = c.disease(id).unwrap().case(s, &mut NoNoise).unwrap();
        let label = format!("{id} s={s}");
        assert_eq!(
            case.stage.as_deref(),
            entry["stage"].as_str(),
            "{label} : stade"
        );
        let exp = entry["mods"].as_array().unwrap();
        assert_eq!(
            case.modifiers.len(),
            exp.len(),
            "{label} : nombre de modificateurs"
        );
        for (m, e) in case.modifiers.iter().zip(exp) {
            let (k, op, v) = (
                e[0].as_str().unwrap(),
                e[1].as_str().unwrap(),
                e[2].as_f64().unwrap(),
            );
            assert_eq!((m.param, m.op.as_str()), (k, op), "{label} : ordre/cible");
            assert!(
                isclose(m.value, v),
                "{label} {k} : Rust {} / Python {v}",
                m.value
            );
        }
        for (flag, val) in entry["flags"].as_object().unwrap() {
            let got = case.flags.iter().find(|(f, _)| f == flag).map(|(_, b)| *b);
            assert_eq!(got, val.as_bool(), "{label} : drapeau {flag}");
        }
        n += 1;
    }
    assert_eq!(n, 81); // 9 maladies × 9 sévérités
}

#[test]
fn applied_params_match_python() {
    let c = catalog();
    let fx = fixture();
    for entry in fx["applied"].as_array().unwrap() {
        let bw = entry["bw"].as_f64().unwrap();
        let mut pt = Patient::healthy(&c.patient, bw).unwrap();
        let mut label = format!("{bw} kg");
        for d in entry["diseases"].as_array().unwrap() {
            let (id, s) = (d[0].as_str().unwrap(), d[1].as_f64().unwrap());
            pt.add_disease(c.disease(id).unwrap(), s, &mut NoNoise)
                .unwrap();
            label.push_str(&format!(" + {id}:{s}"));
        }
        assert_params(&label, &pt.params, entry["params"].as_object().unwrap());
    }
}

#[test]
fn dsl_matches_reference_interpreter() {
    let fx = fixture();
    let env_json = fx["dsl_env"].as_object().unwrap();
    let mut env = Env::new();
    let scope: Vec<&str> = env_json.keys().map(String::as_str).collect();
    for (k, v) in env_json {
        env.set_num(k, v.as_f64().unwrap());
    }
    for case in fx["dsl"].as_array().unwrap() {
        let expr = &case["expr"];
        let mut issues = Vec::new();
        let parsed = Expr::parse(expr, "", &scope, &mut issues);
        match case.get("error").and_then(Json::as_str) {
            // Variable inconnue : refusée dès le chargement (plus tôt que Python).
            Some("KeyError") => assert!(
                parsed.is_none() && issues[0].message.contains("variable inconnue"),
                "{expr} : devait être refusé au chargement"
            ),
            Some(err) => {
                let r = parsed.expect("parse").eval(&env);
                let ok = match err {
                    "ZeroDivisionError" => matches!(r, Err(EvalError::DivisionByZero(_))),
                    "ValueError" => matches!(r, Err(EvalError::LogDomain(_))),
                    _ => false,
                };
                assert!(ok, "{expr} : Python lève {err}, Rust donne {r:?}");
            }
            None => {
                let got = parsed.expect("parse").eval(&env).unwrap();
                let exp = &case["value"];
                match (got, exp) {
                    (Value::Bool(g), Json::Bool(e)) => assert_eq!(g, *e, "{expr}"),
                    (Value::Num(g), e) if e.is_number() => {
                        assert!(isclose(g, e.as_f64().unwrap()), "{expr} : {g} / {e}")
                    }
                    (g, e) => panic!("{expr} : type différent, Rust {g:?} / Python {e}"),
                }
            }
        }
    }
}
