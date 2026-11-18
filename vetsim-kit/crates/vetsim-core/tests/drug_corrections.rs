//! Corrections du 18/11/2026 (docs/VERIFICATION_MALADIES_MEDICAMENTS.tex) : contrôles
//! comportementaux sur le moteur Rust normatif, chacun tiré d'une source et, quand c'est
//! possible, doublé d'un contrôle d'échec (le test doit échouer si la correction est retirée).
//! Usage pédagogique uniquement.

use std::path::PathBuf;
use vetsim_core::{
    apply_drug_effects, evaluate, timecourse, Catalog, DiseaseModule, Dose, DrugModule, Error,
    NoNoise, Params, Patient, PatientModule, Route,
};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn catalog() -> Catalog {
    let patient = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let diseases = ["mmvd", "ckd", "addison", "atrial_fibrillation"]
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

fn patient(c: &Catalog, disease: Option<(&str, f64)>) -> Params {
    let mut pt = Patient::healthy(&c.patient, 20.0).unwrap();
    if let Some((d, s)) = disease {
        pt.add_disease(c.disease(d).unwrap(), s, &mut NoNoise)
            .unwrap();
    }
    pt.params
}

fn iv(drug: &str, mg_per_kg: f64) -> Vec<Dose> {
    vec![Dose {
        t_h: 0.0,
        drug: drug.into(),
        mg_per_kg,
        route: Route::Iv,
    }]
}

fn drugged(c: &Catalog, p: &Params, drug: &str, conc: f64) -> Params {
    apply_drug_effects(p, &c.drugs, &[(drug.to_string(), conc)], 0.0, 0.0).unwrap()
}

/// Électrolytes (paramètres) après une dose de furosémide, aux instants `t`.
fn furosemide_ions(c: &Catalog, p: &Params, mg_per_kg: f64, t: &[f64]) -> Vec<(f64, f64, f64)> {
    let tc = timecourse(p, &c.drugs, &iv("furosemide", mg_per_kg), 24.0, t).unwrap();
    (0..tc.t.len())
        .map(|i| {
            let q = apply_drug_effects(p, &c.drugs, &[], tc.d[i], tc.u[i]).unwrap();
            (q.k, q.na, q.cl)
        })
        .collect()
}

#[test]
fn furosemide_losses_stay_physiological_even_at_massive_dose() {
    // Correction 1 : pertes proportionnelles à la concentration ; K⁺ ne peut plus être négatif.
    let c = catalog();
    let p = patient(&c, None);
    for (k, na, cl) in furosemide_ions(&c, &p, 200.0, &[0.0, 2.0, 8.0, 12.0, 24.0]) {
        assert!(k > 1.5 && na > 120.0 && cl > 70.0, "K {k} Na {na} Cl {cl}");
    }
}

#[test]
fn furosemide_2mg_kg_iv_matches_koh_2021() {
    // Koh 2021 (texte intégral, Tableau 4) : K 4,48 → 4,15 à 8 h (−7,4 %), Na sans variation
    // significative, Cl 110,66 → 106,36 (−3,9 %).
    let c = catalog();
    let p = patient(&c, None);
    let (k, na, cl) = furosemide_ions(&c, &p, 2.0, &[8.0])[0];
    let (dk, dna, dcl) = (k / p.k - 1.0, na / p.na - 1.0, cl / p.cl - 1.0);
    assert!((-0.12..=-0.03).contains(&dk), "ΔK/K {dk}");
    assert!(dna > -0.02, "ΔNa/Na {dna}");
    assert!((-0.07..=-0.01).contains(&dcl), "ΔCl/Cl {dcl}");
}

#[test]
fn furosemide_no_hyper_response_in_ckd() {
    // Correction 5 : effet piloté par l'excrétion urinaire (Miyazaki 1990). En MRC, la
    // concentration plasmatique s'accumule (×15 à 12 h) ; avant la correction, la diurèse
    // effective était plus que doublée (K⁺ 1,2 mmol/L). Écart restant documenté : la source
    // décrit une réponse diminuée, le modèle donne encore +21 % (effet saturé, élimination lente).
    let c = catalog();
    let healthy = patient(&c, None);
    let ckd = patient(&c, Some(("ckd", 0.625)));
    let u = |p: &Params| {
        timecourse(p, &c.drugs, &iv("furosemide", 2.0), 12.0, &[12.0])
            .unwrap()
            .u[0]
    };
    let (uh, uc) = (u(&healthy), u(&ckd));
    assert!(
        uc <= uh * 1.25,
        "diurèse effective MRC {uc} contre sain {uh}"
    );
    // Contrôle d'échec : avec l'ancien pilotage plasmatique, l'hyperréponse revient.
    let mut plasma = c.drugs.clone();
    for m in &mut plasma {
        if let vetsim_core::modules::PdKind::Diuretic { urinary_drive, .. } = &mut m.pd.kind {
            *urinary_drive = false;
        }
    }
    let up = timecourse(&ckd, &plasma, &iv("furosemide", 2.0), 12.0, &[12.0])
        .unwrap()
        .u[0];
    assert!(up > uh * 1.25, "pilotage plasmatique : {up} contre {uh}");
}

#[test]
fn opioid_conscious_dog_no_apnea_and_preserved_pressure() {
    // Correction 2 : chien vigile. Méthadone 1 mg/kg IV : FC −32 % (Mills 2025), PAM conservée
    // (Maiante 2009, Garofalo 2012). Fentanyl massif : ni apnée ni PaCO2 > 67 (Bailey 1987).
    let c = catalog();
    let p = patient(&c, None);
    let base = evaluate(&p).unwrap();
    let ther = evaluate(&drugged(&c, &p, "opioide", 0.1)).unwrap();
    let dhr = ther.params.hr / base.params.hr - 1.0;
    let dmap = ther.hemo.summary.map / base.hemo.summary.map - 1.0;
    assert!((-0.45..=-0.20).contains(&dhr), "ΔFC {dhr}");
    assert!(dmap > -0.10, "ΔPAM {dmap}");
    assert!(
        ther.metabolic.art.pco2 < 46.0,
        "PaCO2 {}",
        ther.metabolic.art.pco2
    );
    for conc in [1.0, 10.0, 30.0] {
        let e = evaluate(&drugged(&c, &p, "opioide", conc))
            .unwrap_or_else(|err| panic!("opioïde {conc} mg/L : {err}"));
        assert!(
            e.metabolic.art.pco2 <= 67.0,
            "PaCO2 {}",
            e.metabolic.art.pco2
        );
    }
}

#[test]
fn sustained_map_below_40_is_not_viable() {
    // Correction 4 : PAM maintenue sous 40 mmHg = choc irréversible (Morales 1999, Granot 1985).
    let c = catalog();
    let p = drugged(
        &c,
        &patient(&c, Some(("addison", 0.85))),
        "pimobendan",
        2.18,
    );
    match evaluate(&p) {
        Err(Error::NonViable { cause, .. }) => assert_eq!(cause, "irreversible_shock"),
        other => panic!(
            "attendu : choc irréversible, obtenu {:?}",
            other.map(|e| e.hemo.summary.map)
        ),
    }
    // Contrôle d'échec : sans le critère, le même patient « survit » avec une PAM < 40.
    let mut q = p.clone();
    q.map_arrest = 0.0;
    let e = evaluate(&q).expect("viable sans le critère de PAM");
    assert!(e.hemo.summary.map < 40.0, "PAM {}", e.hemo.summary.map);
}

#[test]
fn beta_blocker_slows_ventricular_rate_in_atrial_fibrillation() {
    // Correction 6 : effet dromotrope du sympathique sur le nœud AV (dénervation : 144,5 → 121,5
    // /min, PMC7872775). Contrôle d'échec : sans av_symp, aucun ralentissement.
    let c = catalog();
    let af = patient(&c, Some(("atrial_fibrillation", 0.8)));
    let hr = |p: &Params| evaluate(p).unwrap().params.hr;
    let (h0, h1) = (hr(&af), hr(&drugged(&c, &af, "betabloquant", 10.0)));
    assert!(h1 < 0.92 * h0, "FC ventriculaire {h0} → {h1}");
    let mut off = af.clone();
    off.av_symp = 0.0;
    let (g0, g1) = (hr(&off), hr(&drugged(&c, &off, "betabloquant", 10.0)));
    assert!(g1 > 0.97 * g0, "sans av_symp : {g0} → {g1}");
}
