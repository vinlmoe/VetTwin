//! Message `renal_course` de l'API JSON (rein dynamique, `vetsim_core::renal`).

use serde_json::{json, Value as Json};
use std::path::PathBuf;
use vetsim_api::Engine;

fn load(rel: &str) -> Json {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}
fn msg(e: &mut Engine, m: Json) -> Json {
    serde_json::from_str(&e.message(&m.to_string())).unwrap()
}

#[test]
fn renal_course_round_trip() {
    let mut e = Engine::new();
    let bundle = json!({
        "patient": load("modules/healthy_dog.json"),
        "diseases": [], "drugs": [load("modules/drugs/furosemide.json")],
        "renal": load("modules/renal/renal_model.json"),
    });
    let r: Json = serde_json::from_str(&e.load_modules(&bundle.to_string())).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["loaded"]["renal"], true);
    let pid: Json =
        serde_json::from_str(&e.new_patient(r#"{"bw": 12, "variability": false}"#)).unwrap();
    let pid = pid["patient_id"].as_str().unwrap().to_string();
    let out = msg(
        &mut e,
        json!({"op": "renal_course", "patient": pid, "options": {
            "doses": [{"t_h": 0, "drug": "furosemide", "mg_per_kg": 2, "route": "iv"}],
            "t_end_h": 8, "every_h": 2 }}),
    );
    assert_eq!(out["ok"], true, "{out}");
    let t: Vec<f64> = serde_json::from_value(out["t"].clone()).unwrap();
    assert_eq!(t, vec![0.0, 2.0, 4.0, 6.0, 8.0]);
    let u = out["urine_cum_mL_kg"].as_array().unwrap();
    assert!(u.last().unwrap().as_f64().unwrap() > 10.0, "{u:?}");
    let k = out["k"].as_array().unwrap();
    assert!(k[1].as_f64().unwrap() < k[0].as_f64().unwrap());
    assert!(out["map"][4].as_f64().is_some());
    assert!(out["death_t_h"].is_null());
    assert_eq!(out["validated"], true);
    assert!(out["warning"].is_null());
    // natriurie de base bien inférieure à la natrémie (apport alimentaire / diurèse)
    let una = out["una_mmol_L"].as_array().unwrap();
    assert!(una[4].as_f64().unwrap() < 100.0, "{una:?}");
    // sans module rénal : erreur lisible
    let mut e2 = Engine::new();
    let b2 = json!({ "patient": load("modules/healthy_dog.json"), "diseases": [], "drugs": [] });
    e2.load_modules(&b2.to_string());
    let pid2: Json =
        serde_json::from_str(&e2.new_patient(r#"{"bw": 12, "variability": false}"#)).unwrap();
    let r2 = msg(
        &mut e2,
        json!({"op": "renal_course", "patient": pid2["patient_id"], "options": {}}),
    );
    assert_eq!(r2["ok"], false);
}
