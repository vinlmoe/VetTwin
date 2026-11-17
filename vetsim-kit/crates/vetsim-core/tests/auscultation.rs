//! Module auscultation, étape (a) : port fidèle de `signals_pcg_v5.make_pcg`.
//! Les tirages de NumPy sont rejoués (`Replay`) : comparaison valeur par valeur.
//! Fixture : `tests/fixtures/python_reference_pcg.json` (`tests/make_pcg_fixture.py`).

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::dsp::butter_bandpass;
use vetsim_core::pcg::{synthesize_pcg, AuscultationConfig, PcgOptions};
use vetsim_core::rng::Replay;
use vetsim_core::signals::{sosfilt, PcgInputs};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn config() -> AuscultationConfig {
    AuscultationConfig::from_json(&read("modules/auscultation.json"), "auscultation.json")
        .unwrap_or_else(|e| panic!("{e}"))
}

fn fixture() -> Json {
    serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_pcg.json",
    ))
    .unwrap()
}

fn floats(j: &Json) -> Vec<f64> {
    j.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

/// Décodage base64 standard (tests uniquement).
fn b64_f64(s: &str) -> Vec<f64> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut val = [0u8; 256];
    for (i, c) in A.iter().enumerate() {
        val[*c as usize] = i as u8;
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let v: Vec<u32> = chunk.iter().map(|c| u32::from(val[*c as usize])).collect();
        let n = (v[0] << 18)
            | (v.get(1).copied().unwrap_or(0) << 12)
            | (v.get(2).copied().unwrap_or(0) << 6)
            | v.get(3).copied().unwrap_or(0);
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    out.chunks(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn close(a: f64, b: f64, rtol: f64) -> bool {
    a == b || (a - b).abs() <= rtol * f64::max(a.abs(), b.abs()).max(1e-12)
}

#[test]
fn configuration_loads_with_provenance() {
    let c = config();
    assert_eq!(c.fs, 4000.0);
    assert_eq!(c.components.len(), 3);
    assert!(c.site("base").is_some() && c.site("apex").is_some());
    assert_eq!(c.site("apex").unwrap().evidence, "D");
    // provenance obligatoire
    let mut j: Json = serde_json::from_str(&read("modules/auscultation.json")).unwrap();
    j["s1"].as_object_mut().unwrap().remove("source");
    j["murmur"]["band_hz"] = serde_json::json!([250, 60]);
    let err = AuscultationConfig::from_json(&j.to_string(), "t")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("/s1/source") && err.contains("/murmur/band_hz"),
        "{err}"
    );
}

#[test]
fn butterworth_design_matches_scipy() {
    let fx = fixture();
    for case in fx["impulse"].as_array().unwrap() {
        let band = floats(&case["band"]);
        let sos = butter_bandpass(
            case["order"].as_u64().unwrap() as usize,
            band[0],
            band[1],
            case["fs"].as_f64().unwrap(),
        )
        .unwrap();
        let mut imp = vec![0.0; 800];
        imp[0] = 1.0;
        let got = sosfilt(&sos, &imp);
        let exp = floats(&case["h"]);
        let worst = got
            .iter()
            .zip(&exp)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(worst < 1e-12, "{case:?} : écart {worst:e}");
    }
}

#[test]
fn replay_matches_python_reference() {
    let cfg = config();
    let fx = fixture();
    let stride = fx["stride"].as_u64().unwrap() as usize;
    let mut bad = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
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
            ts: h["ts"].as_f64().unwrap(),
            la_mean: h["la_mean"].as_f64().unwrap(),
        };
        let beats = case["beats"].as_array().map(|_| floats(&case["beats"]));
        let noise = b64_f64(case["noise_b64"].as_str().unwrap());
        assert_eq!(noise.len() as u64, case["n_noise"].as_u64().unwrap());
        let mut rp = Replay::new(noise.clone());
        let opts = PcgOptions {
            fs: case["fs"].as_f64(),
            duration_s: case["dur"].as_f64(),
            resp_hz: case["resp_hz"].as_f64(),
            site: None,
        };
        let r = synthesize_pcg(&inp, beats.as_deref(), &cfg, &opts, &mut rp).unwrap();
        if rp.consumed() != noise.len() {
            bad.push(format!(
                "{label} : {} tirages consommés / {}",
                rp.consumed(),
                noise.len()
            ));
        }
        let s = &case["sig"];
        if r.samples.len() as u64 != s["n"].as_u64().unwrap() {
            bad.push(format!(
                "{label} : {} échantillons / {}",
                r.samples.len(),
                s["n"]
            ));
            continue;
        }
        let mut worst: f64 = 0.0;
        for (i, e) in floats(&s["every"]).iter().enumerate() {
            let g = r.samples[i * stride];
            worst = worst.max((g - e).abs());
        }
        if worst > 1e-9 {
            bad.push(format!(
                "{label} : écart max sur les échantillons {worst:e}"
            ));
        }
        let sum: f64 = r.samples.iter().sum();
        let sum_abs: f64 = r.samples.iter().map(|x| x.abs()).sum();
        for (k, g) in [("somme", sum), ("somme |x|", sum_abs)] {
            let e = s[if k == "somme" { "sum" } else { "sum_abs" }]
                .as_f64()
                .unwrap();
            if !close(g, e, 1e-9) && (g - e).abs() > 1e-7 {
                bad.push(format!("{label} {k} : Rust {g} / Python {e}"));
            }
        }
        let f = &case["features"];
        let fe = r.features;
        let round = |x: f64, k: f64| (x * k).round() / k;
        let checks = [
            ("s1", round(fe.s1, 100.0) == f["s1"].as_f64().unwrap()),
            ("s2", round(fe.s2, 100.0) == f["s2"].as_f64().unwrap()),
            ("s3", fe.s3 == f["s3"].as_bool().unwrap()),
            (
                "débit",
                round(fe.murmur_peak_flow, 10.0) == f["murmur_peak_flow"].as_f64().unwrap(),
            ),
            (
                "souffle",
                fe.murmur.label_fr() == f["murmur"].as_str().unwrap(),
            ),
            ("grade", u64::from(fe.grade) == f["grade"].as_u64().unwrap()),
        ];
        for (k, ok) in checks {
            if !ok {
                bad.push(format!("{label} {k} : Rust {fe:?} / Python {f}"));
            }
        }
        eprintln!(
            "  {label:<34} {} battements, écart max {worst:.1e}",
            r.beats.len()
        );
    }
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}
