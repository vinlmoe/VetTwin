//! Messages `anesthesia_*` de l'API JSON (docs/ANESTHESIE.tex, § API).

use serde_json::{json, Value as Json};
use std::path::PathBuf;
use vetsim_api::Engine;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn load(rel: &str) -> Json {
    serde_json::from_str(&std::fs::read_to_string(root().join(rel)).unwrap()).unwrap()
}
fn all(rel: &str) -> Vec<Json> {
    let mut v: Vec<_> = std::fs::read_dir(root().join(rel))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    v.iter()
        .map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap())
        .collect()
}
fn msg(e: &mut Engine, m: Json) -> Json {
    serde_json::from_str(&e.message(&m.to_string())).unwrap()
}

#[test]
fn anesthesia_messages_round_trip() {
    let mut e = Engine::new();
    let bundle = json!({
        "patient": load("modules/healthy_dog.json"),
        "diseases": [], "drugs": [],
        "anesthesia": {
            "model": load("modules/anesthesia/anesthesia_model.json"),
            "agents": all("modules/anesthesia/agents"),
            "inhalants": all("modules/anesthesia/inhalants"),
            "procedures": all("modules/anesthesia/procedures"),
        }
    });
    let r: Json = serde_json::from_str(&e.load_modules(&bundle.to_string())).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["loaded"]["anesthesia"], true);
    let l = msg(&mut e, json!({"op": "anesthesia_list"}));
    assert!(l["agents"].as_array().unwrap().len() >= 10);
    assert_eq!(l["procedures"].as_array().unwrap().len(), 2);
    let pid: Json =
        serde_json::from_str(&e.new_patient(r#"{"bw": 20, "variability": false}"#)).unwrap();
    let pid = pid["patient_id"].as_str().unwrap().to_string();
    let st = msg(
        &mut e,
        json!({"op": "anesthesia_start", "patient": pid, "options": {"seed": 2, "procedure": "castration"}}),
    );
    assert_eq!(st["ok"], true, "{st}");
    let sid = st["session"].as_str().unwrap().to_string();
    assert_eq!(st["snapshot"]["depth"]["plane"], "awake");
    // intubation refusée chez le chien vigile (déglutition présente)
    let r = msg(
        &mut e,
        json!({"op": "anesthesia_event", "options": {"session": sid, "event": {"type": "intubate"}}}),
    );
    assert_eq!(r["ok"], false);
    let r = msg(
        &mut e,
        json!({"op": "anesthesia_event", "options": {"session": sid, "event": {"type": "bolus", "agent": "propofol", "mg_per_kg": 5.2, "route": "iv"}}}),
    );
    assert_eq!(r["ok"], true, "{r}");
    msg(
        &mut e,
        json!({"op": "anesthesia_step", "options": {"session": sid, "dt_s": 5, "n": 18}}),
    );
    for ev in [
        json!({"type": "intubate"}),
        json!({"type": "fresh_gas", "l_min": 2}),
        json!({"type": "vaporizer", "agent": "sevoflurane", "pct": 3}),
    ] {
        let r = msg(
            &mut e,
            json!({"op": "anesthesia_event", "options": {"session": sid, "event": ev}}),
        );
        assert_eq!(r["ok"], true, "{r}");
    }
    let s = msg(
        &mut e,
        json!({"op": "anesthesia_step", "options": {"session": sid, "dt_s": 5, "n": 12, "all": true, "waves_s": 2}}),
    );
    assert_eq!(s["ok"], true, "{s}");
    assert_eq!(s["snapshots"].as_array().unwrap().len(), 12);
    assert!(s["ecg"].as_array().unwrap().len() > 100);
    assert!(s["pressures"]["ao"].as_array().is_some());
    let last = s["snapshots"].as_array().unwrap().last().unwrap();
    assert_ne!(last["depth"]["plane"], "awake");
    assert!(last["vitals"]["hr"].as_f64().unwrap() > 0.0);
    let bad = msg(
        &mut e,
        json!({"op": "anesthesia_event", "options": {"session": sid, "event": {"type": "teleport"}}}),
    );
    assert_eq!(bad["ok"], false);
    let rec = msg(
        &mut e,
        json!({"op": "anesthesia_record", "options": {"session": sid}}),
    );
    assert_eq!(rec["ok"], true);
    assert!(rec["log"].as_array().unwrap().len() >= 4);
    assert!(rec["milestones"]["intubation"].is_number());
}

/// Auscultation en temps réel pendant l'anesthésie : le son accompagne les courbes du moniteur,
/// sur les mêmes battements que l'ECG ; refusé (et non ignoré) sans configuration auscultation.
#[test]
fn anesthesia_step_streams_heart_sounds_with_the_monitor() {
    let bundle = |with_ausc: bool| {
        let mut b = json!({
            "patient": load("modules/healthy_dog.json"),
            "diseases": [], "drugs": [],
            "anesthesia": {
                "model": load("modules/anesthesia/anesthesia_model.json"),
                "agents": all("modules/anesthesia/agents"),
                "inhalants": all("modules/anesthesia/inhalants"),
                "procedures": all("modules/anesthesia/procedures"),
            }
        });
        if with_ausc {
            b["auscultation"] = load("modules/auscultation.json");
        }
        b
    };
    let start = |e: &mut Engine| {
        let p: Json = serde_json::from_str(&e.new_patient(r#"{"bw": 20}"#)).unwrap();
        let pid = p["patient_id"].as_str().unwrap().to_string();
        let r = msg(
            e,
            json!({"op": "anesthesia_start", "patient": pid, "options": {"seed": 3}}),
        );
        r["session"].as_str().unwrap().to_string()
    };
    // sans configuration : erreur explicite
    let mut e0 = Engine::new();
    let _ = e0.load_modules(&bundle(false).to_string());
    let sid0 = start(&mut e0);
    let r0 = msg(
        &mut e0,
        json!({"op": "anesthesia_step", "options": {"session": sid0, "waves_s": 6, "pcg": true}}),
    );
    assert_eq!(r0["ok"], false, "{r0}");
    // avec configuration : son présent, battements = ondes R de l'ECG du même segment
    let mut e = Engine::new();
    let r: Json = serde_json::from_str(&e.load_modules(&bundle(true).to_string())).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    let sid = start(&mut e);
    let s = msg(
        &mut e,
        json!({"op": "anesthesia_step", "options": {"session": sid, "dt_s": 5, "n": 2, "waves_s": 6, "seed": 1, "pcg": {"site": "apex"}}}),
    );
    assert_eq!(s["ok"], true, "{s}");
    let pcg = &s["pcg"];
    assert_eq!(pcg["site"], "apex");
    assert!(pcg["wav_base64"].as_str().unwrap().len() > 10_000);
    let ecg: Vec<f64> = s["ecg"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    let mx = ecg.iter().copied().fold(f64::MIN, f64::max);
    let mut r_times = Vec::new();
    for i in 1..ecg.len() - 1 {
        let far = r_times
            .last()
            .is_none_or(|&t: &f64| i as f64 / 250.0 - t > 0.2);
        if ecg[i] > 0.5 * mx && ecg[i] >= ecg[i - 1] && ecg[i] > ecg[i + 1] && far {
            r_times.push(i as f64 / 250.0);
        }
    }
    let beats: Vec<f64> = pcg["beats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    assert!(
        beats.len().abs_diff(r_times.len()) <= 1 && beats.len() >= 4,
        "battements du son {beats:?}, ondes R de l'ECG {r_times:?}"
    );
    // synchronisme : chaque battement du son a une onde R à moins de 0,12 s
    for b in &beats {
        assert!(
            r_times.iter().any(|r| (r - b).abs() < 0.12),
            "battement {b} s sans onde R voisine : {r_times:?}"
        );
    }
    // sans demande : pas de son
    let s2 = msg(
        &mut e,
        json!({"op": "anesthesia_step", "options": {"session": sid, "waves_s": 6}}),
    );
    assert!(s2.get("pcg").is_none() || s2["pcg"].is_null());
}
