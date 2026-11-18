//! Refus des modules invalides au chargement (chemin JSON, champ, raison) et erreurs typées
//! à l'exécution.

use serde_json::{json, Value as Json};
use std::path::PathBuf;
use vetsim_core::{
    Catalog, DiseaseModule, DrugModule, Error, EvalError, LoadError, NoNoise, Patient,
    PatientModule,
};

fn read(rel: &str) -> Json {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn issues(r: Result<impl std::fmt::Debug, LoadError>) -> Vec<(String, String)> {
    match r {
        Err(LoadError::Invalid { issues, .. }) => {
            issues.into_iter().map(|i| (i.path, i.message)).collect()
        }
        other => panic!("refus attendu, obtenu {other:?}"),
    }
}

fn disease_issues(j: &Json) -> Vec<(String, String)> {
    issues(DiseaseModule::from_json(&j.to_string(), "test"))
}

fn has(issues: &[(String, String)], path: &str, needle: &str) -> bool {
    issues.iter().any(|(p, m)| p == path && m.contains(needle))
}

#[test]
fn disease_schema_violations_are_located() {
    let mut j = read("modules/diseases/mmvd.json");
    j["effects"][0]["param"] = json!("g_mitral");
    j["effects"][1]["op"] = json!("pow");
    j["effects"][2]["factor"] = json!(2);
    j["evidence"] = json!("E");
    j["schema_version"] = json!("2.0.0");
    j["let"]["late"] = json!({"max": [0, {"div": [{"sub": ["sev", 0.8]}, 0.2]}]});
    let is = disease_issues(&j);
    assert!(
        has(&is, "/effects/0/param", "paramètre inconnu « g_mitral »"),
        "{is:?}"
    );
    assert!(has(&is, "/effects/1/op", "« pow » invalide"), "{is:?}");
    assert!(has(&is, "/effects/2/factor", "non autorisé"), "{is:?}");
    assert!(has(&is, "/evidence", "« E »"), "{is:?}");
    assert!(has(&is, "/schema_version", "2.0.0"), "{is:?}");
    assert!(
        has(
            &is,
            "/let/late/max/1/div/0/sub/0",
            "variable inconnue « sev »"
        ),
        "{is:?}"
    );
    // Tous les problèmes sont rapportés ensemble.
    assert_eq!(is.len(), 6, "{is:?}");
}

#[test]
fn disease_scope_and_severity_rules() {
    // Un let ne peut pas utiliser un let défini après lui.
    let mut j = read("modules/diseases/mmvd.json");
    j["let"] = json!({"a": {"mul": ["b", 2]}, "b": "s"});
    assert!(has(
        &disease_issues(&j),
        "/let/a/mul/0",
        "variable inconnue « b »"
    ));

    // Un effet de maladie ne peut pas lire les variables d'un médicament.
    let mut j = read("modules/diseases/pneumonia.json");
    j["effects"][0]["expr"] = json!({"mul": ["E", 2]});
    assert!(has(&disease_issues(&j), "/effects/0/expr/mul/0", "« E »"));

    // Bornes de stade et prior.
    let mut j = read("modules/diseases/ckd.json");
    j["severity"]["stages"]["IRIS2"] = json!([0.5, 0.25]);
    j["severity"]["prior"]["IRIS5"] = json!(0.1);
    j["severity"]["prior"]
        .as_object_mut()
        .unwrap()
        .remove("IRIS1");
    let is = disease_issues(&j);
    assert!(has(&is, "/severity/stages/IRIS2", "bornes"), "{is:?}");
    assert!(has(&is, "/severity/prior/IRIS5", "inconnu"), "{is:?}");
    assert!(
        has(&is, "/severity/prior", "« IRIS1 » sans probabilité"),
        "{is:?}"
    );

    // Nom de tirage réservé, distribution inconnue, cible sur un stade inexistant.
    let mut j = read("modules/diseases/addison.json");
    j["draws"]["s"] = json!({"dist": "lognormal_mean1", "sd": 0.1});
    j["draws"]["z_k"] = json!({"dist": "uniform", "lo": 0});
    j["targets"][0]["stage"] = json!("terminal");
    let is = disease_issues(&j);
    assert!(has(&is, "/draws/s", "déjà utilisé"), "{is:?}");
    assert!(has(&is, "/draws/z_k/dist", "« uniform »"), "{is:?}");
    assert!(has(&is, "/targets/0/stage", "« terminal »"), "{is:?}");
}

#[test]
fn drug_rules() {
    let mut j = read("modules/drugs/pimobendan.json");
    j["pk"]["f"] = json!(1.4);
    j["pk"].as_object_mut().unwrap().remove("vc_L_kg");
    j["pd"]["effects"][0]["expr"] = json!({"mul": ["D", 2]});
    let is = issues(DrugModule::from_json(&j.to_string(), "t"));
    assert!(has(&is, "/pk/f", "hors du domaine"), "{is:?}");
    assert!(has(&is, "/pk", "« vc_L_kg » manquant"), "{is:?}");
    // D n'existe que pour un diurétique.
    assert!(has(&is, "/pd/effects/0/expr/mul/0", "« D »"), "{is:?}");

    let mut j = read("modules/drugs/furosemide.json");
    j["pd"].as_object_mut().unwrap().remove("tau_refill_h");
    let is = issues(DrugModule::from_json(&j.to_string(), "t"));
    assert!(has(&is, "/pd", "« tau_refill_h » manquant"), "{is:?}");
}

#[test]
fn patient_rules() {
    let mut j = read("modules/healthy_dog.json");
    j["params"].as_object_mut().unwrap().remove("hb");
    j["params"]["hr"] = json!({"mul": [87, {"pow": ["sev", -0.25]}]});
    j["params"]["heart_rate"] = json!(80);
    j["interindividual_variability"]["k"]["sd"] = json!(-1);
    let is = issues(PatientModule::from_json(&j.to_string(), "t"));
    assert!(has(&is, "/params", "« hb » non défini"), "{is:?}");
    assert!(has(&is, "/params/hr/mul/1/pow/0", "« sev »"), "{is:?}");
    assert!(
        has(&is, "/params/heart_rate", "paramètre inconnu"),
        "{is:?}"
    );
    assert!(
        has(&is, "/interindividual_variability/k/sd", "hors du domaine"),
        "{is:?}"
    );
}

#[test]
fn messages_are_readable() {
    let err = DiseaseModule::from_json("{\"id\": 3", "modules/diseases/x.json").unwrap_err();
    assert!(matches!(err, LoadError::Json { .. }));
    assert!(err
        .to_string()
        .starts_with("modules/diseases/x.json : JSON illisible"));

    let mut j = read("modules/diseases/mmvd.json");
    j["effects"][1]["op"] = json!("pow");
    let msg = DiseaseModule::from_json(&j.to_string(), "modules/diseases/mmvd.json")
        .unwrap_err()
        .to_string();
    assert_eq!(
        msg,
        "modules/diseases/mmvd.json : module invalide\n  \
         - /effects/1/op : opération « pow » invalide (attendu mul ou add)"
    );
}

#[test]
fn duplicate_ids_are_refused() {
    let p = PatientModule::from_json(&read("modules/healthy_dog.json").to_string(), "p").unwrap();
    let d = DiseaseModule::from_json(&read("modules/diseases/ckd.json").to_string(), "d").unwrap();
    let err = Catalog::new(p, vec![d.clone(), d], vec![]).unwrap_err();
    assert!(matches!(
        err,
        LoadError::Duplicate {
            kind: "disease",
            ..
        }
    ));
}

#[test]
fn runtime_errors_are_typed() {
    let p = PatientModule::from_json(&read("modules/healthy_dog.json").to_string(), "p").unwrap();
    assert_eq!(
        Patient::healthy(&p, 0.0).unwrap_err(),
        Error::InvalidBodyWeight(0.0)
    );
    assert!(Patient::healthy(&p, f64::NAN).is_err());

    let ckd =
        DiseaseModule::from_json(&read("modules/diseases/ckd.json").to_string(), "d").unwrap();
    assert_eq!(
        ckd.case(1.5, &mut NoNoise).unwrap_err(),
        Error::InvalidSeverity(1.5)
    );

    // Division par zéro à l'évaluation : erreur localisée, jamais de NaN.
    let mut j = read("modules/diseases/pneumonia.json");
    j["effects"][0]["expr"] = json!({"div": [1, {"sub": ["s", 0.5]}]});
    let m = DiseaseModule::from_json(&j.to_string(), "d").unwrap();
    match m.case(0.5, &mut NoNoise).unwrap_err() {
        Error::Eval {
            module,
            location,
            source,
        } => {
            assert_eq!(module, "pneumonia");
            assert_eq!(location, "/effects/0/expr");
            assert_eq!(source, EvalError::DivisionByZero("div"));
        }
        other => panic!("{other:?}"),
    }

    // Probabilité de Bernoulli hors de [0, 1].
    let mut j = read("modules/diseases/ckd.json");
    j["draws"]["hyper"]["p"] = json!({"add": [0.9, "s"]});
    let m = DiseaseModule::from_json(&j.to_string(), "d").unwrap();
    assert!(matches!(
        m.case(0.5, &mut NoNoise).unwrap_err(),
        Error::InvalidProbability { .. }
    ));
}

#[test]
fn clamp_and_when_are_applied() {
    let p = PatientModule::from_json(&read("modules/healthy_dog.json").to_string(), "p").unwrap();
    let mut j = read("modules/diseases/pneumonia.json");
    j["effects"] = json!([
        {"param": "vagal_block", "op": "add", "expr": 3, "clamp": [0, 1]},
        {"param": "hr", "op": "mul", "expr": 2, "when": {"gt": ["s", 0.5]}},
        {"param": "hr", "op": "add", "expr": 1}
    ]);
    let m = DiseaseModule::from_json(&j.to_string(), "d").unwrap();
    let mut pt = Patient::healthy(&p, 20.0).unwrap();
    pt.add_disease(&m, 0.4, &mut NoNoise).unwrap();
    assert_eq!(pt.params.vagal_block, 1.0);
    assert_eq!(pt.params.hr, 88.0); // when faux : seul l'effet additif s'applique
    let mut pt = Patient::healthy(&p, 20.0).unwrap();
    pt.add_disease(&m, 0.6, &mut NoNoise).unwrap();
    assert_eq!(pt.params.hr, 175.0); // ordre respecté : 87 × 2 + 1
}
