//! Jalon M6 (partie déterministe) : autonome, VFC, ECG et PCG comparés valeur par valeur à la
//! référence Python, en rejouant les tirages de NumPy (`Replay`).
//! Fixture : `tests/fixtures/python_reference_m6.json` (`tests/make_m6_fixture.py`).

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::autonomic::{hrv_metrics, simulate_autonomic, AutonomicInputs, AutonomicOptions};
use vetsim_core::rng::Replay;
use vetsim_core::signals::{make_ecg, make_pcg, sosfilt, PcgInputs, PCG_BANDPASS_SOS};
use vetsim_core::{Catalog, DiseaseModule, DrugModule, Params, Patient, PatientModule};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn fixture() -> Json {
    serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_m6.json",
    ))
    .unwrap()
}

fn base_params() -> Params {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let _ = (
        DiseaseModule::from_json,
        DrugModule::from_json,
        Catalog::new,
    );
    Patient::healthy(&pm, 20.0).unwrap().params
}

fn params_from(j: &Json) -> Params {
    let mut p = base_params();
    for (k, v) in j.as_object().unwrap() {
        *p.get_mut(k).unwrap() = v.as_f64().unwrap();
    }
    p
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

/// Compare une série à son résumé de fixture (1 valeur sur `stride`, somme, somme des |x|).
fn check_series(
    label: &str,
    got: &[f64],
    exp: &Json,
    stride: usize,
    rtol: f64,
    bad: &mut Vec<String>,
) {
    let n = exp["n"].as_u64().unwrap() as usize;
    if got.len() != n {
        bad.push(format!("{label} : longueur {} / {n}", got.len()));
        return;
    }
    for (i, e) in floats(&exp["every"]).iter().enumerate() {
        let g = got[i * stride];
        if !close(g, *e, rtol) && (g - e).abs() > 1e-12 {
            bad.push(format!("{label}[{}] : Rust {g} / Python {e}", i * stride));
            break;
        }
    }
    let s: f64 = got.iter().sum();
    let sa: f64 = got.iter().map(|x| x.abs()).sum();
    for (name, g, e) in [
        ("somme", s, exp["sum"].as_f64().unwrap()),
        ("somme |x|", sa, exp["sum_abs"].as_f64().unwrap()),
    ] {
        if !close(g, e, rtol.max(1e-9)) && (g - e).abs() > 1e-9 {
            bad.push(format!("{label} {name} : Rust {g} / Python {e}"));
        }
    }
}

#[test]
fn butterworth_matches_scipy_impulse_response() {
    let fx = fixture();
    let mut imp = vec![0.0; 400];
    imp[0] = 1.0;
    let got = sosfilt(&PCG_BANDPASS_SOS, &imp);
    for (i, (g, e)) in got.iter().zip(floats(&fx["butter_impulse"])).enumerate() {
        assert!((g - e).abs() < 1e-15, "h[{i}] : {g} / {e}");
    }
}

#[test]
fn autonomic_and_hrv_replay_match_python() {
    let fx = fixture();
    let stride = fx["stride"].as_u64().unwrap() as usize;
    let mut bad = Vec::new();
    for case in fx["autonomic"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let i = &case["inputs"];
        let inp = AutonomicInputs {
            hr: i["hr"].as_f64().unwrap(),
            s_eff: i["s_eff"].as_f64().unwrap(),
            v_eff: i["v_eff"].as_f64().unwrap(),
            map: i["map"].as_f64().unwrap(),
            rr: i["rr"].as_f64().unwrap(),
            va: i["va"].as_f64().unwrap(),
            params: params_from(&i["params"]),
        };
        let opts = AutonomicOptions {
            duration_s: case["dur"].as_f64().unwrap(),
            ..AutonomicOptions::default()
        };
        let noise = floats(&case["noise"]);
        let mut rp = Replay::new(noise.clone());
        let out = simulate_autonomic(&inp, &opts, &mut rp);
        assert_eq!(
            rp.consumed(),
            noise.len(),
            "{label} : nombre de tirages consommés"
        );
        let beats = floats(&case["beats"]);
        if out.beats.len() != beats.len() {
            bad.push(format!(
                "{label} : {} battements / {}",
                out.beats.len(),
                beats.len()
            ));
            continue;
        }
        let worst = out
            .beats
            .iter()
            .zip(&beats)
            .map(|(g, e)| (g - e).abs())
            .fold(0.0, f64::max);
        if worst > 1e-9 {
            bad.push(format!("{label} : instants R, écart max {worst:e} s"));
        }
        check_series(
            &format!("{label} hr"),
            &out.hr,
            &case["hr"],
            stride,
            1e-9,
            &mut bad,
        );
        check_series(
            &format!("{label} s"),
            &out.s,
            &case["s"],
            stride,
            1e-9,
            &mut bad,
        );
        check_series(
            &format!("{label} v"),
            &out.v,
            &case["v"],
            stride,
            1e-9,
            &mut bad,
        );
        // VFC calculée sur les instants R **de la référence** (isole hrv_metrics).
        let h = hrv_metrics(&beats, case["fr"].as_f64().unwrap()).unwrap();
        let e = &case["hrv"];
        for (k, g) in [
            ("fc_moy", h.fc_moy),
            ("sdnn_ms", h.sdnn_ms),
            ("rmssd_ms", h.rmssd_ms),
            ("lf", h.lf),
            ("hf", h.hf),
            ("lf_hf", h.lf_hf),
            ("rsa_bpm", h.rsa_bpm),
            ("f_peak", h.f_peak),
        ] {
            let ev = e[k].as_f64().unwrap();
            if !close(g, ev, 1e-9) {
                bad.push(format!("{label} VFC {k} : Rust {g} / Python {ev}"));
            }
        }
        eprintln!(
            "  {label:<12} {} battements, écart max des instants R {worst:.1e} s",
            beats.len()
        );
    }
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn ecg_replay_matches_python() {
    let fx = fixture();
    let stride = fx["stride"].as_u64().unwrap() as usize;
    let mut bad = Vec::new();
    for case in fx["ecg"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let p = params_from(&case["params"]);
        let rt = case["r_times"].as_array().map(|_| floats(&case["r_times"]));
        let mut rp = Replay::new(floats(&case["noise"]));
        let e = make_ecg(
            &p,
            case["hr"].as_f64().unwrap(),
            case["rr"].as_f64().unwrap(),
            8.0,
            500.0,
            rt.as_deref(),
            &mut rp,
        );
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
                bad.push(format!("{label} {k} : Rust {g} / Python {}", f[k]));
            }
        }
        check_series(
            &format!("ECG {label}"),
            &e.v,
            &case["sig"],
            stride,
            1e-9,
            &mut bad,
        );
    }
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn pcg_replay_matches_python() {
    let fx = fixture();
    let stride = fx["stride"].as_u64().unwrap() as usize;
    let mut bad = Vec::new();
    for case in fx["pcg"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let h = &case["hemo"];
        let inp = PcgInputs {
            t: floats(&h["t"]),
            q_ao: floats(&h["Q_ao"]),
            q_mi: floats(&h["Q_mi"]),
            p_lv: floats(&h["P_lv"]),
            p_ao: floats(&h["P_ao"]),
            p_pv: floats(&h["P_pv"]),
            period: h["T"].as_f64().unwrap(),
            ts: 0.37 * h["T"].as_f64().unwrap().sqrt(),
            la_mean: h["la_mean"].as_f64().unwrap(),
        };
        let mut rp = Replay::new(floats(&case["noise"]));
        let pcg = make_pcg(&inp, 4, &mut rp);
        let f = &case["features"];
        let pf = pcg.features;
        let round = |x: f64, d: i32| {
            let k = if d == 1 { 10.0 } else { 100.0 };
            (x * k).round() / k
        };
        let checks = [
            (
                "s1",
                (round(pf.s1, 2) - f["s1"].as_f64().unwrap()).abs() < 1e-9,
            ),
            (
                "s2",
                (round(pf.s2, 2) - f["s2"].as_f64().unwrap()).abs() < 1e-9,
            ),
            ("s3", pf.s3 == f["s3"].as_bool().unwrap()),
            (
                "murmur_peak_flow",
                (round(pf.murmur_peak_flow, 1) - f["murmur_peak_flow"].as_f64().unwrap()).abs()
                    < 1e-9,
            ),
            (
                "murmur",
                pf.murmur.label_fr() == f["murmur"].as_str().unwrap(),
            ),
            ("grade", u64::from(pf.grade) == f["grade"].as_u64().unwrap()),
        ];
        for (k, ok) in checks {
            if !ok {
                bad.push(format!("{label} {k} : Rust {pf:?} / Python {f}"));
            }
        }
        check_series(
            &format!("PCG {label}"),
            &pcg.v,
            &case["sig"],
            stride,
            1e-9,
            &mut bad,
        );
    }
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}
