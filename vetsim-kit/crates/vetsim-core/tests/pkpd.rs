//! Jalon M4 : PK/PD, effets des médicaments, diurèse.
//! Référence stricte : `tests/fixtures/python_reference_m4.json` (`tests/make_m4_fixture.py`).
//! La conformité golden (cas `pk_*` et `*_<médicament>`) est vérifiée par
//! `python tests/conformance.py --cmd target/release/vetsim-cli`.

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::{
    apply_drug_effects, timecourse, Catalog, DiseaseModule, Dose, DrugModule, Error, NoNoise,
    Patient, PatientModule, Route,
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

fn fixture() -> Json {
    serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_m4.json",
    ))
    .unwrap()
}

fn patient(c: &Catalog, case: &Json) -> Patient {
    let mut pt = Patient::healthy(&c.patient, case["bw"].as_f64().unwrap()).unwrap();
    for d in case["diseases"].as_array().unwrap() {
        let m = c.disease(d[0].as_str().unwrap()).unwrap();
        pt.add_disease(m, d[1].as_f64().unwrap(), &mut NoNoise)
            .unwrap();
    }
    pt
}

fn isclose(a: f64, b: f64, rtol: f64, atol: f64) -> bool {
    a == b || (a - b).abs() <= f64::max(rtol * f64::max(a.abs(), b.abs()), atol)
}

#[test]
fn drug_effects_match_python_exactly() {
    let c = catalog();
    let fx = fixture();
    let mut bad = Vec::new();
    let mut n = 0;
    for case in fx["effects"].as_array().unwrap() {
        let pt = patient(&c, case);
        let conc: Vec<(String, f64)> = case["conc"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
            .collect();
        let (d, u) = (case["D"].as_f64().unwrap(), case["U"].as_f64().unwrap());
        let q = apply_drug_effects(&pt.params, &c.drugs, &conc, d, u).unwrap();
        for (k, v) in case["params"].as_object().unwrap() {
            let (got, e) = (q.get(k).unwrap(), v.as_f64().unwrap());
            if !isclose(got, e, 1e-12, 1e-15) {
                bad.push(format!("{conc:?} D={d} U={u} {k}: Rust {got} / Python {e}"));
            }
            n += 1;
        }
    }
    eprintln!("M4 effets : {}/{n} paramètres à 1e-12", n - bad.len());
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn timecourses_match_python() {
    let c = catalog();
    let fx = fixture();
    let (rtol, atol) = (1e-6, 1e-9);
    let mut bad = Vec::new();
    let (mut n, mut worst) = (0, (0.0f64, String::new()));
    for case in fx["timecourses"].as_array().unwrap() {
        let pt = patient(&c, case);
        let doses: Vec<Dose> = case["doses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| Dose {
                t_h: d[0].as_f64().unwrap(),
                drug: d[1].as_str().unwrap().to_string(),
                mg_per_kg: d[2].as_f64().unwrap(),
                route: if d[3] == "iv" { Route::Iv } else { Route::Po },
            })
            .collect();
        let t_eval: Vec<f64> = case["t_eval"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_f64().unwrap())
            .collect();
        let tc = timecourse(
            &pt.params,
            &c.drugs,
            &doses,
            case["t_end"].as_f64().unwrap(),
            &t_eval,
        )
        .unwrap();
        let label = format!("{} kg {:?}", case["bw"], case["doses"]);
        for (key, exp) in case["expected"].as_object().unwrap() {
            let got: &[f64] = match key.as_str() {
                "t" => &tc.t,
                "D" => &tc.d,
                "U" => &tc.u,
                drug => tc.conc_of(drug).unwrap(),
            };
            let exp = exp.as_array().unwrap();
            assert_eq!(got.len(), exp.len(), "{label} {key} : longueur");
            for (i, (g, e)) in got.iter().zip(exp).enumerate() {
                let e = e.as_f64().unwrap();
                n += 1;
                let rel = (g - e).abs() / e.abs().max(atol);
                if rel > worst.0 {
                    worst = (rel, format!("{label} {key}[{i}]"));
                }
                if !isclose(*g, e, rtol, atol) {
                    bad.push(format!(
                        "{label} {key}[{i}] t={}: Rust {g} / Python {e}",
                        tc.t[i]
                    ));
                }
            }
        }
    }
    eprintln!(
        "M4 cinétiques : {}/{n} valeurs (rel {rtol}, abs {atol}) ; écart relatif max {:.1e} ({})",
        n - bad.len(),
        worst.0,
        worst.1
    );
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn pk_and_pd_errors_are_typed() {
    let c = catalog();
    let pt = Patient::healthy(&c.patient, 20.0).unwrap();
    let dose = |drug: &str, mg: f64, t: f64| Dose {
        t_h: t,
        drug: drug.into(),
        mg_per_kg: mg,
        route: Route::Iv,
    };
    assert_eq!(
        timecourse(
            &pt.params,
            &c.drugs,
            &[dose("aspirine", 1.0, 0.0)],
            1.0,
            &[0.0]
        )
        .unwrap_err(),
        Error::UnknownDrug("aspirine".into())
    );
    assert!(matches!(
        timecourse(
            &pt.params,
            &c.drugs,
            &[dose("furosemide", -1.0, 0.0)],
            1.0,
            &[0.0]
        ),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        timecourse(
            &pt.params,
            &c.drugs,
            &[dose("furosemide", 1.0, f64::NAN)],
            1.0,
            &[0.0]
        ),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        apply_drug_effects(&pt.params, &c.drugs, &[("x".into(), 1.0)], 0.0, 0.0).unwrap_err(),
        Error::UnknownDrug("x".into())
    );
    // D/U sans module diurétique.
    let no_diuretic: Vec<DrugModule> = c
        .drugs
        .iter()
        .filter(|d| d.id != "furosemide")
        .cloned()
        .collect();
    assert!(matches!(
        apply_drug_effects(&pt.params, &no_diuretic, &[], 100.0, 0.0),
        Err(Error::InvalidInput(_))
    ));
    // Sans médicament ni D/U : paramètres inchangés.
    assert_eq!(
        apply_drug_effects(&pt.params, &c.drugs, &[], 0.0, 0.0).unwrap(),
        pt.params
    );
}

/// Voie IM/SC et compartiment effet (`ke0_per_h`) : module synthétique dérivé de l'atropine.
/// - IM : la concentration plasmatique monte (pic retardé), puis décroît ; sans `ka_im_per_h`
///   la voie est refusée.
/// - `ke0` : après un bolus IV, `Ce` part de 0 et rejoint `C` ; l'effet `E` suit `Ce` ; un terme
///   `site: plasma` suit `C` immédiatement.
#[test]
fn intramuscular_route_and_effect_compartment() {
    let c = catalog();
    let pt = Patient::healthy(&c.patient, 20.0).unwrap();
    let mut v: Json = serde_json::from_str(&read("modules/drugs/atropine.json")).unwrap();
    v["pk"]["ka_im_per_h"] = Json::from(6.0);
    v["pk"]["f_im"] = Json::from(0.9);
    v["pd"]["ke0_per_h"] = Json::from(20.0);
    v["pd"]["toxicity"] = serde_json::json!([{
        "id": "plasma_term", "emax": 1.0, "ec50_mg_L": 0.001, "hill": 1.0, "site": "plasma",
        "effects": [{"param": "vagal_add", "op": "add", "expr": "T"}]
    }]);
    let m = DrugModule::from_json(&v.to_string(), "atropine").unwrap();
    let drugs = vec![m];
    let dose = |route| Dose {
        t_h: 0.0,
        drug: "atropine".into(),
        mg_per_kg: 0.04,
        route,
    };
    let t: Vec<f64> = vec![0.0, 0.01, 0.05, 0.25, 1.0];
    // IM : pic retardé
    let tc = timecourse(&pt.params, &drugs, &[dose(Route::Im)], 1.0, &t).unwrap();
    let c_im = tc.conc_of("atropine").unwrap();
    assert_eq!(c_im[0], 0.0);
    assert!(c_im[2] > c_im[1] && c_im[3] > c_im[2], "{c_im:?}");
    // IV : C maximal d'emblée, Ce retardé puis rejoint C
    let tc = timecourse(&pt.params, &drugs, &[dose(Route::Iv)], 1.0, &t).unwrap();
    let (cp, ce) = (&tc.conc[0].1, &tc.conc_e[0].1);
    assert!(cp[0] > 0.0 && ce[0] == 0.0, "C {cp:?} Ce {ce:?}");
    assert!(ce[1] < 0.5 * cp[1], "équilibration non instantanée");
    assert!((ce[4] - cp[4]).abs() < 0.1 * cp[4], "équilibre à 1 h : {} / {}", ce[4], cp[4]);
    // E suit Ce (blocage vagal), le terme plasma suit C
    let early = vetsim_core::apply_drug_effects_sites(
        &pt.params,
        &drugs,
        &[("atropine".into(), cp[0])],
        &[("atropine".into(), ce[0])],
        0.0,
        0.0,
    )
    .unwrap();
    assert_eq!(early.vagal_block, pt.params.vagal_block);
    assert!(early.vagal_add > pt.params.vagal_add + 0.9);
    // sans voie IM décrite : refus
    let plain = DrugModule::from_json(&read("modules/drugs/opioide.json"), "opioide").unwrap();
    let im = Dose {
        drug: "opioide".into(),
        ..dose(Route::Im)
    };
    assert!(timecourse(&pt.params, &[plain], &[im], 1.0, &[1.0]).is_err());
}
