//! Couche de conduction : rejeu des tirages de la référence Python et comparaison des événements.
//! Référence : `tests/fixtures/python_reference_rhythm.json` (`tests/make_rhythm_fixture.py`).

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::rhythm::{is_trivial, simulate_conduction, summarize, Origin};
use vetsim_core::{Patient, PatientModule};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * f64::max(1.0, f64::max(a.abs(), b.abs()))
}

#[test]
fn conduction_matches_python_replay() {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let fx: Json = serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_rhythm.json",
    ))
    .unwrap();
    let mut bad = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut p = Patient::healthy(&pm, case["bw"].as_f64().unwrap())
            .unwrap()
            .params;
        for (k, v) in case["over"].as_object().unwrap() {
            *p.get_mut(k).unwrap() = v.as_f64().unwrap();
        }
        let draws: Vec<f64> = case["draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        let mut i = 0;
        let mut rand = || {
            let u = draws[i];
            i += 1;
            u
        };
        let (v_eff, per, t_end) = (
            case["v_eff"].as_f64().unwrap(),
            case["sinus_period"].as_f64().unwrap(),
            case["t_end"].as_f64().unwrap(),
        );
        let ev = simulate_conduction(&p, v_eff, per, t_end, &mut rand, None);
        if i != draws.len() {
            bad.push(format!("{id} : {i} tirages consommés / {}", draws.len()));
        }
        let ea = case["atrial"].as_array().unwrap();
        if ea.len() != ev.atrial.len() {
            bad.push(format!(
                "{id} : {} activations A / Python {}",
                ev.atrial.len(),
                ea.len()
            ));
        } else {
            for (a, e) in ev.atrial.iter().zip(ea) {
                if !close(a.t, e[0].as_f64().unwrap()) || a.organized != e[1].as_bool().unwrap() {
                    bad.push(format!("{id} : A {a:?} / {e}"));
                    break;
                }
            }
        }
        let ev_v = case["ventricular"].as_array().unwrap();
        if ev_v.len() != ev.ventricular.len() {
            bad.push(format!(
                "{id} : {} activations V / Python {}",
                ev.ventricular.len(),
                ev_v.len()
            ));
            continue;
        }
        for (b, e) in ev.ventricular.iter().zip(ev_v) {
            let opt = |x: Option<f64>, j: &Json| match (x, j.as_f64()) {
                (None, None) => true,
                (Some(a), Some(b)) => close(a, b),
                _ => false,
            };
            if !close(b.t, e[0].as_f64().unwrap())
                || b.origin.as_str() != e[1].as_str().unwrap()
                || !opt(b.t_a, &e[2])
                || !opt(b.pr, &e[3])
                || b.wide != e[4].as_bool().unwrap()
            {
                bad.push(format!("{id} : V {b:?} / {e}"));
                break;
            }
        }
        if is_trivial(&p, v_eff, per) != case["trivial"].as_bool().unwrap() {
            bad.push(format!("{id} : trivialité différente"));
        }
    }
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

/// Contrôle d'échec : un influx bloqué ne doit pas produire d'activation ventriculaire ; sans
/// foyer d'échappement, le BAV complet donne l'asystolie (le mécanisme décide, pas un réglage).
#[test]
fn complete_block_without_escape_is_asystole() {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let mut p = Patient::healthy(&pm, 20.0).unwrap().params;
    p.hp_conduction = 0.0;
    let ev = simulate_conduction(&p, 0.6, 60.0 / 88.0, 30.0, &mut || 0.5, None);
    let s = summarize(&ev, 5.0, 30.0);
    assert!(s
        .origins
        .iter()
        .all(|(o, _)| *o == Origin::VentricularEscape));
    p.vent_escape_rate = 0.0;
    let ev = simulate_conduction(&p, 0.6, 60.0 / 88.0, 30.0, &mut || 0.5, None);
    assert!(ev.ventricular.is_empty());
}
