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
    // décrit une réponse diminuée, le modèle donne encore ≈ +25 % de U effectif (effet saturé,
    // élimination lente).
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
        uc <= uh * 1.3,
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
    assert!(up > uh * 1.3, "pilotage plasmatique : {up} contre {uh}");
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
    // Correction 6 : effet dromotrope du sympathique sur le nœud AV. Cai et al. 2021
    // (PMC7872775, lu) : section du tronc sympathique thoracique gauche chez le beagle en FA
    // persistante induite, 121,5 contre 144,5/min chez les témoins à 30 min (deux groupes ; ce
    // n'est pas un bêtabloquant). Bêtabloquant en FA chez le chien : effet modeste, carvédilol
    // 12,5 mg × 2/j 123,3 → 120,3/min, −2,4 % (Choi et al. 2014, PMC4081622, lu) ; aténolol seul
    // sans amélioration de la FC (Jung et al. 2016, PMC4913664, lu, n = 3). Attendu : baisse
    // franche mais modeste, entre 0 et 15 % (révisé le 24/11/2026 ; −8 % au moins auparavant).
    // Contrôle d'échec : sans av_symp, aucun ralentissement.
    let c = catalog();
    let af = patient(&c, Some(("atrial_fibrillation", 0.8)));
    let hr = |p: &Params| evaluate(p).unwrap().params.hr;
    let (h0, h1) = (hr(&af), hr(&drugged(&c, &af, "betabloquant", 10.0)));
    assert!(
        h1 < 0.98 * h0 && h1 > 0.85 * h0,
        "FC ventriculaire {h0} → {h1}"
    );
    let mut off = af.clone();
    off.av_symp = 0.0;
    let (g0, g1) = (hr(&off), hr(&drugged(&c, &off, "betabloquant", 10.0)));
    assert!(g1 > 0.97 * g0, "sans av_symp : {g0} → {g1}");
}

/// POG d'une MMVD sous furosémide à l'instant `t_h` d'un schéma (états évalués comme l'API).
fn la_under(c: &Catalog, p: &Params, doses: &[Dose], t_h: f64) -> (f64, f64) {
    let tc = timecourse(p, &c.drugs, doses, t_h, &[t_h]).unwrap();
    let conc: Vec<(String, f64)> = tc.conc.iter().map(|(d, v)| (d.clone(), v[0])).collect();
    let q = apply_drug_effects(p, &c.drugs, &conc, tc.d[0], tc.u[0]).unwrap();
    let e = evaluate(&q).unwrap();
    (e.hemo.summary.la_mean, e.hemo.summary.map)
}

#[test]
fn furosemide_lowers_lap_in_mitral_regurgitation() {
    // Suzuki 2013 (résumé) : furosémide 0,17 mg/kg/h pendant 6 h, IM expérimentale :
    // POG 14,75 → 10,77 mmHg (−27 %). Mécanismes : veinodilatation dépendante du rein (Bayne
    // 1979) et orifice mitral dynamique (Yoran 1979). PAM « peu changée » (Suzuki 2011).
    let c = catalog();
    let p = patient(&c, Some(("mmvd", 0.78)));
    let doses: Vec<Dose> = (0..12)
        .map(|i| Dose {
            t_h: f64::from(i) / 2.0,
            drug: "furosemide".into(),
            mg_per_kg: 0.085,
            route: Route::Iv,
        })
        .collect();
    let base = evaluate(&p).unwrap();
    let (la0, map0) = (base.hemo.summary.la_mean, base.hemo.summary.map);
    let (la, map) = la_under(&c, &p, &doses, 5.9);
    assert!(la < 0.82 * la0, "POG {la0} → {la}");
    assert!(map > 0.9 * map0, "PAM {map0} → {map}");
    // Contrôle d'échec : orifice fixe et sans veinodilatation, la POG ne baisse presque pas.
    let mut fixed = p.clone();
    fixed.mr_k = 0.0;
    let mut drugs = c.drugs.clone();
    for m in &mut drugs {
        m.pd.effects.retain(|e| e.param != "ven_cap");
    }
    let c2 = Catalog::new(c.patient.clone(), c.diseases.clone(), drugs).unwrap();
    let la_f0 = evaluate(&fixed).unwrap().hemo.summary.la_mean;
    let (la_f, _) = la_under(&c2, &fixed, &doses, 5.9);
    assert!(la_f > 0.9 * la_f0, "sans les mécanismes : {la_f0} → {la_f}");
}

#[test]
fn chronic_furosemide_kalemia_matches_cardiac_cohorts() {
    // Adin 2020 (lu) : stade C, 3,5 mg/kg/j : K 4,50 contre 4,70 (−4 %) ; TEST (Chetboul 2017,
    // lu) : K 4,50 à J84. Adaptation (W, τ 12 h, φ 0,98). Contrôle d'échec : sans adaptation,
    // la même posologie fait chuter K⁺ sous 3 mmol/L.
    let c = catalog();
    let p = patient(&c, None);
    let doses: Vec<Dose> = (0..56)
        .map(|i| Dose {
            t_h: 12.0 * f64::from(i),
            drug: "furosemide".into(),
            mg_per_kg: 1.75,
            route: Route::Po,
        })
        .collect();
    let t: Vec<f64> = (0..12).map(|i| 27.0 * 24.0 + 2.0 * f64::from(i)).collect();
    let k_mean = |drugs: &[DrugModule]| {
        let tc = timecourse(&p, drugs, &doses, 28.0 * 24.0, &t).unwrap();
        let ks: Vec<f64> =
            tc.u.iter()
                .zip(&tc.d)
                .map(|(u, d)| apply_drug_effects(&p, drugs, &[], *d, *u).unwrap().k)
                .collect();
        ks.iter().sum::<f64>() / ks.len() as f64
    };
    let k = k_mean(&c.drugs);
    assert!(
        (0.92..=0.995).contains(&(k / p.k)),
        "K⁺ chronique {k} (base {})",
        p.k
    );
    let mut no_adapt = c.drugs.clone();
    for m in &mut no_adapt {
        if let vetsim_core::modules::PdKind::Diuretic { adapt, .. } = &mut m.pd.kind {
            *adapt = None;
        }
    }
    let k2 = k_mean(&no_adapt);
    assert!(k2 < 3.0, "sans adaptation : K⁺ {k2}");
}

/// Solveur du baroréflexe : l'état rendu est un vrai point fixe (`|d_t(d) − d| < BARO_TOL`), y
/// compris dans les cas où l'ancienne itération amortie (5 passages) s'arrêtait sur un état
/// intermédiaire incohérent (opioïde : PAM 88 mmHg avec un sympathique en baisse).
#[test]
fn baroreflex_state_is_a_converged_fixed_point() {
    use vetsim_core::engine::BARO_TOL;
    let c = catalog();
    let cases: Vec<(&str, Params)> = vec![
        ("sain", patient(&c, None)),
        ("opioïde", drugged(&c, &patient(&c, None), "opioide", 0.1)),
        (
            "atropine",
            drugged(&c, &patient(&c, None), "atropine", 0.02),
        ),
        (
            "bêtabloquant",
            drugged(&c, &patient(&c, None), "betabloquant", 0.5),
        ),
        ("MMVD C", patient(&c, Some(("mmvd", 0.725)))),
        ("MMVD D", patient(&c, Some(("mmvd", 0.925)))),
        ("Addison", patient(&c, Some(("addison", 0.85)))),
        ("FA", patient(&c, Some(("atrial_fibrillation", 0.8)))),
    ];
    for (name, p) in cases {
        let e = evaluate(&p).unwrap();
        let q = &e.params;
        let s = q.sympathetic;
        if s <= 0.0 || s >= 1.0 {
            continue; // commande saturée : tonus écrêté
        }
        let d = (s - q.s0) / 0.8;
        let d_t = (q.baro_gain * (q.map_set - e.hemo.summary.map) / 45.0).clamp(-1.0, 1.0);
        assert!(
            (d_t - d).abs() < BARO_TOL + 1e-9,
            "{name} : d {d}, d_t {d_t} (PAM {})",
            e.hemo.summary.map
        );
    }
}
