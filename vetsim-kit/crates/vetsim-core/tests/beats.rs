//! Hémodynamique battement par battement : comparaison avec `physio.simulate_hemo_beats`.
//! Référence : `tests/fixtures/python_reference_beats.json` (`tests/make_beats_fixture.py`).
//! Mêmes séquences de conduction (tirages rejoués) ; solveurs différents (LSODA / DP5) :
//! tolérance 1 % ou 0,05 en absolu sur les moyennes, 2 % ou 0,5 mL battement par battement.

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::hemo::simulate_hemo_beats;
use vetsim_core::rhythm::simulate_conduction;
use vetsim_core::{Patient, PatientModule};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn beats_match_python() {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let fx: Json = serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_beats.json",
    ))
    .unwrap();
    let (t_end, warm) = (fx["t_end"].as_f64().unwrap(), fx["warm"].as_f64().unwrap());
    let mut bad = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut p = Patient::healthy(&pm, 20.0).unwrap().params;
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
        let per = case["sinus_period"].as_f64().unwrap();
        let cond = simulate_conduction(&p, 0.6, per, t_end, &mut rand, None);
        let r = simulate_hemo_beats(&p, &cond, t_end, warm, per).unwrap();
        let s = &r.summary;
        let got = [
            ("map", s.map),
            ("sap", s.sap),
            ("dap", s.dap),
            ("co", s.co),
            ("edv", s.edv),
            ("esv", s.esv),
            ("sv", s.sv),
            ("la_mean", s.la_mean),
            ("pap_mean", s.pap_mean),
            ("cvp", s.cvp),
            ("lv_edp", s.lv_edp),
            ("reg_fraction", s.reg_fraction),
            ("mitral_e", s.mitral_e),
            ("mitral_a", s.mitral_a),
            ("hr_v", r.hr_v),
        ];
        for (k, v) in got {
            let e = case["summary"][k].as_f64().unwrap();
            if (v - e).abs() > f64::max(0.01 * e.abs(), 0.05) {
                bad.push(format!("{id} : {k} Rust {v:.5} / Python {e:.5}"));
            }
        }
        let sv_py: Vec<f64> = case["beat_sv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        if sv_py.len() != r.beats.len() {
            bad.push(format!(
                "{id} : {} battements / Python {}",
                r.beats.len(),
                sv_py.len()
            ));
        } else {
            for (b, e) in r.beats.iter().zip(&sv_py) {
                if (b.sv - e).abs() > f64::max(0.02 * e, 0.5) {
                    bad.push(format!(
                        "{id} : VES battement {:.3} s Rust {:.3} / Python {e:.3}",
                        b.t, b.sv
                    ));
                    break;
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

/// Contrôle d'échec : sans contraction atriale, la FA ne fait plus perdre la part atriale du
/// débit (le test de comparaison échouerait si la contraction atriale était ignorée).
#[test]
fn atrial_contraction_matters_in_sinus_but_not_in_af() {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let mut p = Patient::healthy(&pm, 20.0).unwrap().params;
    p.k_la = 1.45;
    p.k_ra = 0.29;
    let per = 60.0 / 130.0;
    let cond = simulate_conduction(&p, 0.6, per, 25.0, &mut || 0.0, None);
    let with = simulate_hemo_beats(&p, &cond, 25.0, 10.0, per)
        .unwrap()
        .summary
        .co;
    let mut q = p.clone();
    q.k_la = 0.0;
    q.k_ra = 0.0;
    let without = simulate_hemo_beats(&q, &cond, 25.0, 10.0, per)
        .unwrap()
        .summary
        .co;
    assert!(
        with > 1.05 * without,
        "contribution atriale {with} / {without}"
    );
}
