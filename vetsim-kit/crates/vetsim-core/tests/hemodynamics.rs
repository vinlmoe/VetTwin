//! Jalon M3 : hémodynamique ODE (Dormand-Prince) + boucle baroréflexe, sortie complète d'`eval`.
//!
//! 1. Golden : cas `eval` sans médicament (`healthy_*`, `mmvd_*`, `ckd_*`, `addison_*`,
//!    `pneumonia_*`), tolérances de `tests/golden/cases.json`.
//! 2. Hors golden : `tests/fixtures/python_reference_m3.json` (CLI de référence ; crise
//!    addisonienne, MMVD stade D, combinaisons, 2,5 et 63 kg), mêmes tolérances.
//! 3. Robustesse : balayage de sévérités et de poids, sorties finies.

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::{evaluate, Catalog, DiseaseModule, DrugModule, NoNoise, Patient, PatientModule};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn catalog() -> Catalog {
    let patient = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let diseases = [
        "mmvd",
        "ckd",
        "addison",
        "pneumonia",
        "atrial_fibrillation",
        "av_block",
        "ventricular_premature",
        "sick_sinus",
    ]
    .iter()
    .map(|d| DiseaseModule::from_json(&read(&format!("modules/diseases/{d}.json")), d).unwrap())
    .collect();
    let drugs = [
        "atropine",
        "betabloquant",
        "furosemide",
        "opioide",
        "pimobendan",
    ]
    .iter()
    .map(|d| DrugModule::from_json(&read(&format!("modules/drugs/{d}.json")), d).unwrap())
    .collect();
    Catalog::new(patient, diseases, drugs).unwrap()
}

/// Patient décrit par des arguments du CLI (`--bw`, `--disease id:s`).
fn patient_from_args(c: &Catalog, args: &[&str]) -> Patient {
    let mut bw = 20.0;
    let mut diseases = Vec::new();
    for w in args.windows(2) {
        match w[0] {
            "--bw" => bw = w[1].parse().unwrap(),
            "--disease" => {
                let (id, s) = w[1].split_once(':').unwrap();
                diseases.push((id.to_string(), s.parse::<f64>().unwrap()));
            }
            _ => {}
        }
    }
    let mut pt = Patient::healthy(&c.patient, bw).unwrap();
    for (id, s) in diseases {
        pt.add_disease(c.disease(&id).unwrap(), s, &mut NoNoise)
            .unwrap();
    }
    pt
}

struct Tol {
    json: Json,
}

impl Tol {
    fn load() -> (Self, Json) {
        let g: Json = serde_json::from_str(&read("tests/golden/cases.json")).unwrap();
        (
            Self {
                json: g["tolerances"].clone(),
            },
            g,
        )
    }

    /// Écart rapporté à l'écart toléré (`math.isclose`) : conforme si ≤ 1.
    fn ratio(&self, key: &str, got: f64, exp: f64) -> f64 {
        let t = &self.json["per_key"][key];
        let rt = t["rtol"]
            .as_f64()
            .unwrap_or(self.json["default_rtol"].as_f64().unwrap());
        let at = t["atol"]
            .as_f64()
            .unwrap_or(self.json["default_atol"].as_f64().unwrap());
        let allow = f64::max(rt * f64::max(got.abs(), exp.abs()), at);
        (got - exp).abs() / allow
    }
}

/// Marge du critère d'arrêt du baroréflexe sous laquelle la référence est jugée « sur le fil ».
const KNIFE_EDGE: f64 = 1e-3;

fn check(
    label: &str,
    args: &[&str],
    exp: &Json,
    c: &Catalog,
    tol: &Tol,
    report: &mut Vec<String>,
) -> f64 {
    let pt = patient_from_args(c, args);
    let ev = evaluate(&pt.params).unwrap_or_else(|e| panic!("{label}: {e}"));
    check_eval(label, &ev, exp, tol, report)
}

fn check_eval(
    label: &str,
    ev: &vetsim_core::Evaluation,
    exp: &Json,
    tol: &Tol,
    report: &mut Vec<String>,
) -> f64 {
    let flat = ev.to_flat();
    assert_eq!(
        flat.len(),
        exp.as_object().unwrap().len(),
        "{label} : nombre de clés"
    );
    let mut worst: f64 = 0.0;
    for (k, got) in flat {
        let e = exp[k]
            .as_f64()
            .unwrap_or_else(|| panic!("{label} : clé {k} absente du golden"));
        let r = tol.ratio(k, got, e);
        worst = worst.max(r);
        if r > 1.0 {
            report.push(format!(
                "{label} {k}: Rust {got} / référence {e} ({r:.2} × tolérance)"
            ));
        }
    }
    worst
}

#[test]
fn golden_eval_cases_without_drugs() {
    let c = catalog();
    let (tol, g) = Tol::load();
    let mut bad = Vec::new();
    let (mut n, mut worst) = (0, 0.0f64);
    for case in g["cases"].as_array().unwrap() {
        let args: Vec<&str> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        if args[0] != "eval" || args.iter().any(|a| ["--conc", "--D", "--U"].contains(a)) {
            continue;
        }
        let label = case["id"].as_str().unwrap();
        worst = worst.max(check(label, &args, &case["expected"], &c, &tol, &mut bad));
        n += 1;
    }
    eprintln!("M3 golden : {n} cas, écart max = {worst:.4} × tolérance");
    assert_eq!(n, 27); // 15 + 4 troubles du rythme × 3 sévérités
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn out_of_golden_cases_match_reference_cli() {
    let c = catalog();
    let (tol, _) = Tol::load();
    let fx: Json = serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_m3.json",
    ))
    .unwrap();
    let mut bad = Vec::new();
    let mut worst = 0.0f64;
    let mut knife_edge = 0;
    for case in fx["cases"].as_array().unwrap() {
        let args: Vec<&str> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        let label = args[1..].join(" ");
        let pt = patient_from_args(&c, &args);
        let ev = evaluate(&pt.params).unwrap();
        let py_passes = case["baro_passes"].as_u64().unwrap() as usize;
        let margin = case["baro_margin"].as_f64().unwrap();
        let mut local = Vec::new();
        let r = check_eval(&label, &ev, &case["expected"], &tol, &mut local);
        let note = format!(
            "passages baroréflexe Rust {} / Python {py_passes}, marge Python {margin:.1e}",
            ev.baro_iterations
        );
        eprintln!("  {label:<55} {r:.4} × tolérance ({note})");
        if !local.is_empty() && margin < KNIFE_EDGE && ev.baro_iterations != py_passes {
            // Discontinuité de la référence : critère d'arrêt |d_t − d| < 0,04 franchi d'un côté
            // différent. Écart documenté (docs/SOLVER.md), non masqué : on l'affiche.
            for l in &local {
                eprintln!("    ÉCART CONNU (critère d'arrêt sur le fil) : {l}");
            }
            knife_edge += 1;
            continue;
        }
        bad.extend(local);
        worst = worst.max(r);
    }
    eprintln!(
        "M3 hors golden : écart max = {worst:.4} × tolérance ; {knife_edge} cas sur le fil du critère d'arrêt"
    );
    assert!(
        knife_edge <= 1,
        "trop de cas « sur le fil » : vérifier le modèle"
    );
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn severity_and_weight_sweep_stays_finite() {
    // Décès attendus (critère « dette d'O2 », VO2 > DO2) : MMVD sévère chez le chien de 3 kg.
    // La passe simple donnait déjà un état non physique (ER 1,56 à 2,93, PvO2 au plancher de 1 mmHg).
    // Depuis la normalisation allométrique des fuites valvulaires ((bw/20)^0,75, 18/11/2026), aucun
    // décès : à 3 kg, la MMVD 1,0 donne POG 36 mmHg et PaO2 30 mmHg, comme à 20 kg. Auparavant, la
    // fuite absolue tuait les MMVD 0,8 à 1,0 du chien de 3 kg (Python identique).
    let expected_dead: [(f64, &str, i32); 0] = [];
    let mut dead = Vec::new();
    let c = catalog();
    for bw in [3.0, 20.0, 60.0] {
        for d in ["mmvd", "ckd", "addison", "pneumonia"] {
            for i in 0..=10 {
                let s = f64::from(i) / 10.0;
                let mut pt = Patient::healthy(&c.patient, bw).unwrap();
                pt.add_disease(c.disease(d).unwrap(), s, &mut NoNoise)
                    .unwrap();
                let ev = match evaluate(&pt.params) {
                    Ok(ev) => ev,
                    Err(vetsim_core::error::Error::NonViable { cause, last }) => {
                        let last =
                            last.unwrap_or_else(|| panic!("{bw} kg {d}:{s} : aucune passe viable"));
                        assert!(
                            cause == "oxygen_debt" && last.metabolic.er >= 1.0,
                            "{bw} kg {d}:{s} : {cause}"
                        );
                        dead.push((bw, d, i));
                        continue;
                    }
                    Err(e) => panic!("{bw} kg {d}:{s} : {e}"),
                };
                for (k, v) in ev.to_flat() {
                    assert!(v.is_finite(), "{bw} kg {d}:{s} : {k} = {v}");
                }
                assert!(
                    ev.hemo.summary.converged,
                    "{bw} kg {d}:{s} : régime non périodique"
                );
            }
        }
    }
    assert_eq!(dead, expected_dead, "décès inattendus ou manquants");
}
