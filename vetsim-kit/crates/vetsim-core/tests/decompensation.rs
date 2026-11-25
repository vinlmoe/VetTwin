//! Décompensation mécanistique : termes toxiques des médicaments, couplages (acidémie, hypoxie
//! tissulaire, ischémie coronaire, hypokaliémie) et viabilité.
//! Référence : `tests/fixtures/python_reference_tox.json` (`tests/make_tox_fixture.py`).
//! Classement (issue, mécanismes, cause du décès) : identique. Grandeurs : tolérances du golden
//! (3 % ou 0,02 en absolu ; pH 0,005), le solveur hémodynamique n'étant pas le même (LSODA / DP5).

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::{
    apply_drug_effects, evaluate, Catalog, DiseaseModule, DrugModule, Error, NoNoise, Patient,
    PatientModule,
};

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
        "pulmonary_hypertension",
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

/// Issue d'un scénario : (état, mécanismes ou cause, grandeurs, facteurs de couplage).
type Run = (String, Vec<String>, Vec<(&'static str, f64)>, [f64; 3]);

fn run(c: &Catalog, case: &Json) -> Run {
    let mut pt = Patient::healthy(&c.patient, case["bw"].as_f64().unwrap()).unwrap();
    if let Some(d) = case["disease"].as_array() {
        pt.add_disease(
            c.disease(d[0].as_str().unwrap()).unwrap(),
            d[1].as_f64().unwrap(),
            &mut NoNoise,
        )
        .unwrap();
    }
    let conc: Vec<(String, f64)> = case["conc"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
        .collect();
    let q = apply_drug_effects(
        &pt.params,
        &c.drugs,
        &conc,
        0.0,
        case["U"].as_f64().unwrap(),
    )
    .unwrap();
    match evaluate(&q) {
        Ok(e) => {
            let state = if e.outcome.decompensated {
                "decompensated"
            } else {
                "stable"
            };
            let mech = e
                .outcome
                .mechanisms
                .iter()
                .map(|m| m.id.to_string())
                .collect();
            let m = &e.metabolic;
            let vals = vec![
                ("map", e.hemo.summary.map),
                ("co", e.hemo.summary.co),
                ("hr", e.params.hr),
                ("ph", m.art.ph),
                ("pco2", m.art.pco2),
                ("pao2", m.art.pao2),
                ("po2_v", m.ven.po2),
                ("lactate", m.lactate),
                ("k", m.chem.k),
            ];
            let f = [e.decomp.f_contract, e.decomp.f_rsys, e.decomp.f_vent];
            (state.into(), mech, vals, f)
        }
        Err(Error::NonViable { cause, .. }) => {
            ("dead".into(), vec![cause.into()], vec![], [0.0; 3])
        }
        Err(e) => panic!("{} : {e}", case["id"]),
    }
}

fn close(key: &str, a: f64, b: f64) -> bool {
    let atol = if key.starts_with("ph") { 0.005 } else { 0.02 };
    (a - b).abs() <= f64::max(0.03 * f64::max(a.abs(), b.abs()), atol)
}

#[test]
fn decompensation_matches_python() {
    let c = catalog();
    let fx: Json = serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_tox.json",
    ))
    .unwrap();
    let mut bad = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let (state, mech, vals, f) = run(&c, case);
        if state != case["state"].as_str().unwrap() {
            bad.push(format!("{id} : état {state} / Python {}", case["state"]));
            continue;
        }
        if state == "dead" {
            if mech[0] != case["cause"].as_str().unwrap() {
                bad.push(format!(
                    "{id} : cause {} / Python {}",
                    mech[0], case["cause"]
                ));
            }
            continue;
        }
        let exp: Vec<String> = case["mechanisms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap().to_string())
            .collect();
        if mech != exp {
            bad.push(format!("{id} : mécanismes {mech:?} / Python {exp:?}"));
        }
        for (k, v) in vals {
            let e = case["values"][k].as_f64().unwrap();
            if !close(k, v, e) {
                bad.push(format!("{id} : {k} Rust {v} / Python {e}"));
            }
        }
        for (i, k) in ["f_contract", "f_rsys", "f_vent"].iter().enumerate() {
            let e = case[*k].as_f64().unwrap();
            if (f[i] - e).abs() > 0.03 {
                bad.push(format!("{id} : {k} Rust {} / Python {e}", f[i]));
            }
        }
    }
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

/// Contrôle d'échec : c'est le terme toxique qui prolonge la relation dose-réponse au-delà du
/// plateau thérapeutique. Opioïde massif chez le chien vigile (18/11/2026) : PaCO2 > 60 mmHg et
/// dépression par acidémie, sans décès (Bailey 1987) ; sans terme toxique, plateau thérapeutique.
#[test]
fn toxicity_terms_extend_dose_response() {
    let mut c = catalog();
    let healthy = Patient::healthy(&c.patient, 20.0).unwrap();
    let conc = vec![("opioide".to_string(), 30.0)];
    let q = apply_drug_effects(&healthy.params, &c.drugs, &conc, 0.0, 0.0).unwrap();
    let e = evaluate(&q).expect("chien vigile : pas de décès par opioïde seul");
    assert!(
        e.metabolic.art.pco2 > 60.0,
        "PaCO2 {}",
        e.metabolic.art.pco2
    );
    assert!(e
        .outcome
        .mechanisms
        .iter()
        .any(|m| m.id == "acidemia_depression"));
    for d in &mut c.drugs {
        d.pd.toxicity.clear();
    }
    let q = apply_drug_effects(&healthy.params, &c.drugs, &conc, 0.0, 0.0).unwrap();
    let e = evaluate(&q).unwrap();
    assert!(
        e.metabolic.art.pco2 < 46.0,
        "PaCO2 {}",
        e.metabolic.art.pco2
    );
    assert!(!e
        .outcome
        .mechanisms
        .iter()
        .any(|m| m.id == "acidemia_depression"));
}

/// Contrôle d'échec : un seuil de couplage déplacé change le classement (mutation détectée).
#[test]
fn coupling_threshold_mutation_is_detected() {
    let c = catalog();
    let mut p = Patient::healthy(&c.patient, 20.0).unwrap().params;
    assert!(!evaluate(&p).unwrap().outcome.decompensated);
    p.acid_ph0 = 7.45; // seuil au-dessus du pH du chien sain (7,38) : acidémie « détectée »
    let e = evaluate(&p).unwrap();
    assert!(e
        .outcome
        .mechanisms
        .iter()
        .any(|m| m.id == "acidemia_depression"));
    assert!(e.decomp.f_contract < 1.0);
}
