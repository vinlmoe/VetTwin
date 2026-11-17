//! Auscultation battement par battement : port de `pcg_beats.make_pcg_beats`.
//! Les tirages de NumPy sont rejoués (`Replay`) : comparaison valeur par valeur.
//! Fixture : `tests/fixtures/python_reference_pcg_beats.json` (`tests/make_pcg_beats_fixture.py`).

use serde_json::Value as Json;
use std::path::PathBuf;

use vetsim_core::pcg::{synthesize_pcg_beats, AuscultationConfig, PcgBeat, PcgOptions};
use vetsim_core::rng::Replay;
use vetsim_core::signals::PcgInputs;

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
        "crates/vetsim-core/tests/fixtures/python_reference_pcg_beats.json",
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

fn beats_of(case: &Json, la_mean: f64) -> Vec<PcgBeat> {
    case["beats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| PcgBeat {
            t: b["start"].as_f64().unwrap(),
            h: PcgInputs {
                t: floats(&b["t"]),
                q_ao: floats(&b["Q_ao"]),
                q_mi: floats(&b["Q_mi"]),
                p_lv: floats(&b["P_lv"]),
                p_ao: floats(&b["P_ao"]),
                p_pv: floats(&b["P_pv"]),
                period: b["T"].as_f64().unwrap(),
                ts: b["ts"].as_f64().unwrap(),
                la_mean,
            },
        })
        .collect()
}

#[test]
fn per_beat_synthesis_matches_python_replay() {
    let cfg = config();
    let fx = fixture();
    for case in fx["cases"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let la = case["la_mean"].as_f64().unwrap();
        let noise = b64_f64(case["noise_b64"].as_str().unwrap());
        assert_eq!(noise.len(), case["n_noise"].as_u64().unwrap() as usize);
        let mut rep = Replay::new(noise.clone());
        let o = PcgOptions {
            fs: case["fs"].as_f64(),
            duration_s: case["dur"].as_f64(),
            resp_hz: case["resp_hz"].as_f64(),
            site: None,
        };
        let r = synthesize_pcg_beats(&beats_of(case, la), la, &cfg, &o, &mut rep).unwrap();
        assert_eq!(rep.consumed(), noise.len(), "{label} : tirages consommés");
        let exp = b64_f64(case["sig_b64"].as_str().unwrap());
        assert_eq!(r.samples.len(), exp.len(), "{label} : longueur");
        let worst = r
            .samples
            .iter()
            .zip(&exp)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(worst < 1e-9, "{label} : écart maximal {worst}");
        let f = &case["features"];
        assert!(
            close(r.features.s1, f["s1"].as_f64().unwrap(), 1e-12),
            "{label} s1"
        );
        assert!(
            close(r.features.s2, f["s2"].as_f64().unwrap(), 1e-12),
            "{label} s2"
        );
        assert_eq!(
            u64::from(r.features.grade),
            f["grade"].as_u64().unwrap(),
            "{label} grade"
        );
        assert!(
            close(r.t_ac, f["t_ac"].as_f64().unwrap(), 1e-12),
            "{label} t_ac"
        );
    }
}

/// Contrôle d'échec : avec une hémodynamique identique pour tous les battements (celle du
/// premier), l'intensité de B1 ne varie plus ; en FA elle doit varier avec le battement.
#[test]
fn s1_varies_with_beat_hemodynamics_in_af() {
    let cfg = config();
    let fx = fixture();
    let case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"].as_str().unwrap().starts_with("FA"))
        .unwrap();
    let la = case["la_mean"].as_f64().unwrap();
    let beats = beats_of(case, la);
    let s1 = |bs: &[PcgBeat]| -> Vec<f64> {
        bs.iter()
            .map(|b| {
                let one = [b.clone()];
                let o = PcgOptions {
                    duration_s: Some(1.0 + b.t),
                    ..PcgOptions::default()
                };
                let mut z = vetsim_core::rng::SimRng::new(0);
                synthesize_pcg_beats(&one, la, &cfg, &o, &mut z)
                    .unwrap()
                    .features
                    .s1
            })
            .collect()
    };
    let v = s1(&beats);
    let spread = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - v.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(spread > 0.01, "B1 en FA : {v:?}");
    let same: Vec<PcgBeat> = beats
        .iter()
        .map(|b| PcgBeat {
            t: b.t,
            h: beats[0].h.clone(),
        })
        .collect();
    let w = s1(&same);
    let spread0 = w.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - w.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(spread0 < 1e-12, "{w:?}");
}

fn close(a: f64, b: f64, rtol: f64) -> bool {
    a == b || (a - b).abs() <= rtol * f64::max(a.abs(), b.abs()).max(1e-12)
}
