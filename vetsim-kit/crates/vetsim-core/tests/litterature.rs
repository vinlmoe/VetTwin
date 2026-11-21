//! Banc de validation contre la littérature canine (docs/VALIDATION_HORS_CALIBRAGE.tex).
//!
//! Chaque vérification compare une sortie du moteur à une fourchette publiée, avec sa source et
//! son niveau d'accès. Une régression du modèle fait échouer la CI.
//!
//! Contrôles d'échec (`each_mechanism_is_needed`) : on retire le mécanisme calé sur une source et
//! la vérification correspondante doit échouer. Une vérification qui passe sans son mécanisme ne
//! prouve rien.
use std::path::PathBuf;
use vetsim_core::anesthesia::{
    AdminRoute, AnesthesiaModel, Event, InhalantModule, Session, Ventilation,
};
use vetsim_core::{
    evaluate, Catalog, DiseaseModule, DrugModule, NoNoise, Params, Patient, PatientModule,
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
    ]
    .iter()
    .map(|d| DiseaseModule::from_json(&read(&format!("modules/diseases/{d}.json")), d).unwrap())
    .collect();
    let drugs = ["furosemide", "pimobendan"]
        .iter()
        .map(|d| DrugModule::from_json(&read(&format!("modules/drugs/{d}.json")), d).unwrap())
        .collect();
    Catalog::new(patient, diseases, drugs).unwrap()
}

fn params(c: &Catalog, disease: Option<(&str, f64)>) -> Params {
    let mut pt = Patient::healthy(&c.patient, 20.0).unwrap();
    if let Some((id, s)) = disease {
        pt.add_disease(c.disease(id).unwrap(), s, &mut NoNoise)
            .unwrap();
    }
    pt.params
}

/// Une vérification : valeur du modèle, fourchette publiée, source.
struct Check {
    id: &'static str,
    value: f64,
    lo: f64,
    hi: f64,
    source: &'static str,
}

impl Check {
    fn ok(&self) -> bool {
        self.value.is_finite() && self.value >= self.lo && self.value <= self.hi
    }
}

/// Réglages modifiables pour les contrôles d'échec (`None` : valeur du module).
#[derive(Default, Clone, Copy)]
struct Knobs {
    renal_k_symp: Option<f64>,
    anemia_visc: Option<f64>,
    hypox_symp: Option<f64>,
    hypox_inotropy: Option<f64>,
    hypox_vaso: Option<f64>,
}

impl Knobs {
    fn apply(&self, p: &mut Params) {
        if let Some(v) = self.renal_k_symp {
            p.renal_k_symp = v;
        }
        if let Some(v) = self.anemia_visc {
            p.anemia_visc = v;
        }
        if let Some(v) = self.hypox_symp {
            p.hypox_symp = v;
        }
        if let Some(v) = self.hypox_inotropy {
            p.hypox_inotropy = v;
        }
        if let Some(v) = self.hypox_vaso {
            p.hypox_vaso = v;
        }
    }
}

/// Vérifications stationnaires (moteur `evaluate`, chien de 20 kg, mode déterministe).
fn steady_checks(k: Knobs) -> Vec<Check> {
    let c = catalog();
    let ev = |d: Option<(&str, f64)>, f: &dyn Fn(&mut Params)| {
        let mut p = params(&c, d);
        k.apply(&mut p);
        f(&mut p);
        evaluate(&p).unwrap_or_else(|e| panic!("{d:?} : {e}"))
    };
    let none = |_: &mut Params| {};
    let h = ev(None, &none);
    let hm = &h.hemo.summary;
    let mut v = Vec::new();
    let mut push = |id, value, lo, hi, source| {
        v.push(Check {
            id,
            value,
            lo,
            hi,
            source,
        })
    };
    // chien sain
    push(
        "sain_fc",
        h.params.hr,
        80.0,
        95.0,
        "Haskins 2005 : 87 /min (résumé)",
    );
    push(
        "sain_pam",
        hm.map,
        97.0,
        109.0,
        "Haskins 2005 : 103 mmHg (résumé)",
    );
    push(
        "sain_paco2",
        h.metabolic.art.pco2,
        38.0,
        42.5,
        "Haskins 2005 : 40,2 mmHg (résumé)",
    );
    push(
        "sain_pao2",
        h.metabolic.art.pao2,
        90.0,
        105.0,
        "Haskins 2005 : 99,5 mmHg (résumé)",
    );
    // compensation respiratoire des acidoses métaboliques
    let ckd = ev(Some(("ckd", 0.8)), &none);
    push(
        "compensation_pente",
        (ckd.metabolic.art.pco2 - h.metabolic.art.pco2)
            / (ckd.metabolic.art.hco3 - h.metabolic.art.hco3),
        0.64,
        0.84,
        "Madias 1984 : 0,74 mmHg/(mmol/L) (résumé) ; DiBartola : 0,7 (lu)",
    );
    // MRC : créatinine du stade IRIS 2–3 et hypertension
    let ckd5 = ev(Some(("ckd", 0.5)), &none);
    push(
        "mrc05_creat",
        ckd5.metabolic.chem.creat,
        200.0,
        300.0,
        "IRIS : 250 µmol/L à la frontière 2/3 (lu)",
    );
    push(
        "mrc05_pam",
        ckd5.hemo.summary.map,
        105.0,
        125.0,
        "Pelander 2017 : PAS 157 contre 129 (lu) ; Sharkey 2009 (résumé)",
    );
    // pneumonie (A-a ≈ 47 mmHg)
    let pn = ev(Some(("pneumonia", 0.72)), &none);
    push(
        "pneumonie_paco2",
        pn.metabolic.art.pco2,
        27.0,
        33.0,
        "Viitanen 2017 : 28,8 mmHg (lu)",
    );
    push(
        "pneumonie_fc",
        pn.params.hr,
        100.0,
        140.0,
        "Goggs 2022 : 117 ± 27 ; Young 2022 : 126–130 (lus)",
    );
    push(
        "pneumonie_fr",
        pn.metabolic.art.rr,
        40.0,
        62.0,
        "Viitanen 2017 : 43–61 ; Young 2022 : 41–50 (lus)",
    );
    // Addison
    let ad1 = ev(Some(("addison", 1.0)), &none);
    push(
        "addison_hco3v",
        ad1.metabolic.ven.hco3,
        12.5,
        19.9,
        "Chalifoux 2023 : 14,7 (12,5–19,9) en crise (lu)",
    );
    let ad5 = ev(Some(("addison", 0.5)), &none);
    push(
        "addison_creat",
        ad5.metabolic.chem.creat,
        100.0,
        175.0,
        "Hall 2023 : médianes 1,4–1,7 mg/dL (lu)",
    );
    // troubles du rythme
    let av = ev(Some(("av_block", 1.0)), &none);
    push(
        "bav3_fc",
        av.params.hr,
        31.0,
        53.0,
        "42 ± 11 /min, n = 97 (10.1111/jvim.12512) ; van Bavel 2023 (lu)",
    );
    let fa = ev(Some(("atrial_fibrillation", 0.8)), &none);
    push(
        "fa_icc_fc",
        fa.params.hr,
        185.0,
        220.0,
        "Menaut 2005 : 203 ± 6 en ICC (résumé)",
    );
    // MMVD : axe cardio-rénal et œdème
    let m5 = ev(Some(("mmvd", 0.5)), &none);
    push(
        "mmvd_b2_dfg",
        m5.metabolic.chem.gfr / 20.0,
        2.0,
        3.4,
        "Kaplan 2022 : 2,8 (2,0–3,7) mL/min/kg, B2 non traités (lu)",
    );
    let m8 = ev(Some(("mmvd", 0.8)), &none);
    push(
        "mmvd_c_creat_cardiaque",
        m8.metabolic.chem.creat / h.metabolic.chem.creat,
        1.12,
        1.40,
        "Adin 2020 : +47 % en C dont ~21 % de furosémide (lu)",
    );
    let m10 = ev(Some(("mmvd", 1.0)), &none);
    push(
        "mmvd_d_pao2",
        m10.metabolic.art.pao2,
        35.0,
        75.0,
        "œdème cardiogénique : SpO2 < 90 % fréquente (Yin 2026, lu)",
    );
    // hypoxémie aiguë (chien conscient)
    let hx36 = ev(None, &|p: &mut Params| p.fio2 = 0.115);
    push(
        "hypoxie36_fc",
        hx36.params.hr / h.params.hr - 1.0,
        0.04,
        0.25,
        "Krebs 1999 : FC +11 /min à PaO2 36 (résumé)",
    );
    let hx28 = ev(None, &|p: &mut Params| p.fio2 = 0.10);
    push(
        "hypoxie28_dc",
        hx28.hemo.summary.co / hm.co - 1.0,
        0.15,
        0.60,
        "Krasney & Koehler 1977 : DC +48 % à PaO2 28 (résumé)",
    );
    // anémie normovolémique (Ht 20 %)
    let an = ev(None, &|p: &mut Params| p.hb = 6.67);
    push(
        "anemie_ht20_dc",
        an.hemo.summary.co / hm.co - 1.0,
        0.35,
        0.85,
        "Rooney 1993 : DC +63–65 % à Ht 20 % (résumé)",
    );
    // cohérence interne : régime périodique en hypovolémie
    let hv = ev(None, &|p: &mut Params| p.vol_shift -= 510.0);
    push(
        "hypovolemie_dc_coherent",
        hv.hemo.summary.co / (hv.params.hr * hv.hemo.summary.sv / 1000.0) - 1.0,
        -0.05,
        0.05,
        "cohérence DC = FC × VS (convergence du cycle)",
    );
    v
}

fn report(checks: &[Check]) -> Vec<String> {
    checks
        .iter()
        .filter(|c| !c.ok())
        .map(|c| {
            format!(
                "{} = {:.3} hors de [{}, {}] ({})",
                c.id, c.value, c.lo, c.hi, c.source
            )
        })
        .collect()
}

#[test]
fn steady_state_matches_literature() {
    let checks = steady_checks(Knobs::default());
    for c in &checks {
        eprintln!("{:<26} {:>9.3}  [{}, {}]", c.id, c.value, c.lo, c.hi);
    }
    let bad = report(&checks);
    assert!(
        bad.is_empty(),
        "{} écarts à la littérature :\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// Retirer un mécanisme calé doit faire échouer la vérification qui le fonde.
#[test]
fn each_mechanism_is_needed() {
    let cases: [(&str, Knobs); 3] = [
        (
            "mmvd_c_creat_cardiaque",
            Knobs {
                renal_k_symp: Some(0.0),
                ..Knobs::default()
            },
        ),
        (
            "anemie_ht20_dc",
            Knobs {
                anemia_visc: Some(0.0),
                ..Knobs::default()
            },
        ),
        (
            "hypoxie28_dc",
            Knobs {
                hypox_symp: Some(0.0),
                hypox_inotropy: Some(0.0),
                hypox_vaso: Some(0.0),
                ..Knobs::default()
            },
        ),
    ];
    for (id, k) in cases {
        let checks = steady_checks(k);
        let c = checks.iter().find(|c| c.id == id).unwrap();
        assert!(
            !c.ok(),
            "{id} passe encore sans son mécanisme ({:.3})",
            c.value
        );
    }
}

/// Anesthésie : oxygénation sous O2 (atélectasie) et asphyxie (DeBehnke 1995).
#[test]
fn anesthesia_matches_literature() {
    let c = catalog();
    let base = params(&c, None);
    let dir = |rel: &str| -> Vec<String> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel);
        let mut v: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        v.sort();
        v.into_iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect()
    };
    let model =
        AnesthesiaModel::from_json(&read("modules/anesthesia/anesthesia_model.json"), "m").unwrap();
    let agents = dir("modules/anesthesia/agents")
        .iter()
        .map(|s| DrugModule::from_json(s, "a").unwrap())
        .collect();
    let inh = dir("modules/anesthesia/inhalants")
        .iter()
        .map(|s| InhalantModule::from_json(s, "i").unwrap())
        .collect();
    let mut s = Session::new(base, agents, inh, model, None, 1).unwrap();
    s.apply(Event::Oxygen(true)).unwrap();
    for _ in 0..12 {
        s.step(10.0).unwrap();
    }
    s.apply(Event::Bolus {
        agent: "propofol".into(),
        mg_per_kg: 5.2,
        route: AdminRoute::Iv,
        duration_s: None,
    })
    .unwrap();
    for _ in 0..18 {
        s.step(5.0).unwrap();
    }
    s.apply(Event::Intubate).unwrap();
    s.apply(Event::FreshGas(2.0)).unwrap();
    s.apply(Event::Vaporizer {
        agent: "isoflurane".into(),
        pct: 2.0,
    })
    .unwrap();
    s.apply(Event::SetVentilation(Ventilation::Controlled {
        rr: 12.0,
        vt_ml_kg: 12.0,
    }))
    .unwrap();
    let mut last = None;
    for _ in 0..40 {
        last = Some(s.step(30.0).unwrap());
    }
    let pao2 = last.unwrap().eval.unwrap().metabolic.art.pao2;
    assert!(
        (495.0..=616.0).contains(&pao2),
        "PaO2 sous O2 {pao2} (Itami 2019 : 547 [495–616], lu)"
    );
    // asphyxie : ventilateur arrêté ; arrêt hypoxique dans 11,4 ± 2,4 min (DeBehnke 1995, résumé)
    s.apply(Event::SetVentilation(Ventilation::Controlled {
        rr: 0.0,
        vt_ml_kg: 0.0,
    }))
    .unwrap();
    let t0 = s.time_s();
    let mut t_arrest = None;
    for _ in 0..120 {
        let sn = s.step(10.0).unwrap();
        if sn.dead.is_some() {
            t_arrest = Some((sn.t_s - t0) / 60.0);
            break;
        }
    }
    let t = t_arrest.expect("arrêt hypoxique attendu");
    assert!(
        (7.0..=16.0).contains(&t),
        "arrêt à {t:.1} min (DeBehnke 1995 : 11,4 ± 2,4)"
    );
}

/// Hémorragie chez le chien conscient : retraits de 5 % du volume sanguin toutes les 5 min.
/// Renvoie la baisse de PAM (mmHg) à 20 % (17 mL/kg) et à 35 % (30 mL/kg).
fn hemorrhage_drops(compensated: bool) -> (f64, f64) {
    let c = catalog();
    let base = params(&c, None);
    let mut model =
        AnesthesiaModel::from_json(&read("modules/anesthesia/anesthesia_model.json"), "m").unwrap();
    if !compensated {
        model.pressor_gain = 0.0;
        model.spleen_ml_kg = 0.0;
        model.refill_frac = 0.0;
    }
    let mut s = Session::new(base, vec![], vec![], model, None, 1).unwrap();
    let map = |sn: &vetsim_core::anesthesia::Snapshot| sn.eval.as_ref().unwrap().hemo.summary.map;
    let m0 = map(&s.step(10.0).unwrap());
    let at = |k: usize, s: &mut Session| {
        let mut last = None;
        for _ in 0..k {
            s.apply(Event::Hemorrhage(0.05 * 85.0)).unwrap();
            for _ in 0..30 {
                last = Some(s.step(10.0).unwrap());
            }
        }
        m0 - map(&last.unwrap())
    };
    let d20 = at(4, &mut s);
    let d35 = at(3, &mut s);
    (d20, d35)
}

#[test]
fn hemorrhage_matches_literature() {
    let (d20, d35) = hemorrhage_drops(true);
    // Shen 1990 : −15 ± 3,3 mmHg à 20 mL/kg ; Thrasher 2001 : PAM tenue jusqu'à ~19 mL/kg (résumés)
    assert!(
        (3.0..=20.0).contains(&d20),
        "baisse de PAM à 17 mL/kg : {d20:.1} mmHg"
    );
    // Thrasher & Keil 1998 : −35 ± 10 mmHg à 30 mL/kg (résumé)
    assert!(
        (15.0..=45.0).contains(&d35),
        "baisse de PAM à 30 mL/kg : {d35:.1} mmHg"
    );
    // contrôle : sans remplissage, rate ni hormones presseuses, la PAM chute trop tôt
    let (d20_0, _) = hemorrhage_drops(false);
    assert!(
        d20_0 > 20.0,
        "sans compensation, baisse à 17 mL/kg {d20_0:.1} mmHg"
    );
}

// ------------------------------------------------------------------ rein dynamique (renal.rs)

/// Évolution rénale d'un chien sain de `bw` kg après les doses de furosémide `doses`
/// (`(t_h, mg/kg, voie)`) ; `tweak` modifie les constantes ou les paramètres du patient.
fn renal_run(
    bw: f64,
    doses: &[(f64, f64, vetsim_core::Route)],
    t_end: f64,
    tweak: impl Fn(&mut vetsim_core::renal::RenalModel, &mut Params),
) -> vetsim_core::renal::RenalCourse {
    renal_run_k(bw, doses, &[], t_end, tweak)
}

/// Comme [`renal_run`], avec des charges orales de K `(t_h, mmol/kg)`.
fn renal_run_k(
    bw: f64,
    doses: &[(f64, f64, vetsim_core::Route)],
    k_loads: &[(f64, f64)],
    t_end: f64,
    tweak: impl Fn(&mut vetsim_core::renal::RenalModel, &mut Params),
) -> vetsim_core::renal::RenalCourse {
    use vetsim_core::renal::{renal_course, RenalModel, RenalOptions};
    let c = catalog();
    let mut model = RenalModel::from_json(&read("modules/renal/renal_model.json"), "r").unwrap();
    let mut p = Patient::healthy(&c.patient, bw).unwrap().params;
    tweak(&mut model, &mut p);
    let ds: Vec<vetsim_core::Dose> = doses
        .iter()
        .map(|&(t_h, mg_per_kg, route)| vetsim_core::Dose {
            t_h,
            drug: "furosemide".into(),
            mg_per_kg,
            route,
        })
        .collect();
    let opt = RenalOptions {
        t_end_h: t_end,
        k_loads: k_loads.to_vec(),
        ..RenalOptions::default()
    };
    renal_course(&model, &p, &c.drugs, &ds, &opt).unwrap()
}

fn at(c: &vetsim_core::renal::RenalCourse, t: f64) -> &vetsim_core::renal::RenalPoint {
    c.points
        .iter()
        .min_by(|a, b| (a.t_h - t).abs().total_cmp(&(b.t_h - t).abs()))
        .unwrap()
}

/// Adin 2021 : 0,66 mg/kg IV puis 0,66 mg/kg/h pendant 5 h (perfusion en bolus de 6 min).
fn adin_doses() -> Vec<(f64, f64, vetsim_core::Route)> {
    let mut d = vec![(0.0, 0.66, vetsim_core::Route::Iv)];
    d.extend((0..50).map(|i| (0.05 + 0.1 * i as f64, 0.066, vetsim_core::Route::Iv)));
    d
}

/// Chien de 20 kg en MRC de sévérité `s` (0 : sain).
fn ckd_params(s: f64) -> impl Fn(&mut vetsim_core::renal::RenalModel, &mut Params) {
    move |_, p| {
        if s > 0.0 {
            let c = catalog();
            let mut pt = Patient::healthy(&c.patient, p.bw).unwrap();
            pt.add_disease(c.disease("ckd").unwrap(), s, &mut NoNoise)
                .unwrap();
            *p = pt.params;
        }
    }
}

/// Sans médicament, le rein dynamique reste sur l'état stationnaire (point fixe), y compris
/// en MRC.
#[test]
fn renal_fixed_point_is_the_steady_state() {
    for s in [0.0, 0.85] {
        let c = renal_run(20.0, &[], 24.0, ckd_params(s));
        let p0 = &c.points[0];
        for p in &c.points {
            assert!(p.d_ecf_ml_kg.abs() < 1e-9 && (p.k - p0.k).abs() < 1e-9);
            assert!((p.creat - p0.creat).abs() < 1e-6 && (p.aldo - 1.0).abs() < 1e-9);
            assert!((p.urine_ml_kg_h - 1.0).abs() < 1e-9);
        }
        assert!(c.n_evals <= 2, "{} évaluations", c.n_evals);
    }
}

/// MRC : l'apport alimentaire ne dépend pas du DFG, le néphron restant s'adapte.
/// - FE de base croissantes avec le stade (×10 environ au stade IV, étude de 20 chiens ;
///   « dépendent de la créatinine », Selin 2025 : résumés de recherche) ;
/// - urine de base bien moins concentrée en Na que le plasma ;
/// - fuite sodée sous furosémide plus faible que chez le chien sain.
#[test]
fn renal_ckd_adapts_fractional_excretion() {
    let run = |s: f64, doses: &[(f64, f64, vetsim_core::Route)]| {
        renal_run(20.0, doses, 8.0, ckd_params(s))
    };
    let (h, k4) = (run(0.0, &[]), run(0.85, &[]));
    let (fh, f4) = (&h.points[0], &k4.points[0]);
    let gfr =
        |c: &vetsim_core::renal::RenalCourse| c.points[0].eval.as_ref().unwrap().metabolic.chem.gfr;
    let ratio_fe = f4.fe_na / fh.fe_na;
    eprintln!(
        "FE_Na {:.4} → {:.4} (×{ratio_fe:.1}), FE_K {:.3} → {:.3}, DFG {:.0} → {:.0} mL/min, UNa {:.0} mmol/L",
        fh.fe_na, f4.fe_na, fh.fe_k, f4.fe_k, gfr(&h), gfr(&k4), fh.una_mmol_l
    );
    assert!((5.0..=15.0).contains(&ratio_fe), "FE_Na ×{ratio_fe:.1}");
    assert!((f4.fe_k / fh.fe_k - ratio_fe).abs() < 0.5);
    assert!(fh.una_mmol_l < 100.0 && (f4.una_mmol_l - fh.una_mmol_l).abs() < 1.0);
    let iv = [(0.0, 2.0, vetsim_core::Route::Iv)];
    let (dh, d4) = (run(0.0, &iv), run(0.85, &iv));
    let (vh, v4) = (
        dh.points.last().unwrap().d_ecf_ml_kg,
        d4.points.last().unwrap().d_ecf_ml_kg,
    );
    assert!(v4 > 0.3 * vh, "VEC à 8 h : sain {vh:.1}, MRC {v4:.1} mL/kg");
}

/// Furosémide en aigu : calage (Koh 2021, IV) et validation (Koh 2021 per os, Adin 2021).
#[test]
fn renal_matches_literature() {
    use vetsim_core::Route::{Iv, Po};
    // calage (avec Koh per os, Bourgoignie 1981 chien normal et Lantis 2011) : Koh 2021,
    // 2 mg/kg IV, beagles 10–13 kg (lu)
    let iv = renal_run(11.5, &[(0.0, 2.0, Iv)], 8.0, |_, _| {});
    let (k0, u8) = (iv.points[0].k, at(&iv, 8.0).urine_cum_ml_kg);
    let (dk2, dk8) = (at(&iv, 2.0).k - k0, at(&iv, 8.0).k - k0);
    eprintln!("Koh IV : diurèse 8 h {u8:.1} mL/kg, ΔK 2 h {dk2:+.2}, 8 h {dk8:+.2}");
    assert!((22.0..=27.5).contains(&u8), "diurèse 8 h {u8:.1} (24,65)");
    assert!((-0.6..=-0.42).contains(&dk2), "ΔK 2 h {dk2:.2} (−0,51)");
    assert!((-0.4..=-0.26).contains(&dk8), "ΔK 8 h {dk8:.2} (−0,33)");
    // calage : Koh 2021 per os (mêmes chiens), diurèse et ΔK à 8 h ; ordre IV < PO
    let po = renal_run(11.5, &[(0.0, 2.0, Po)], 8.0, |_, _| {});
    let (pu8, pdk8) = (
        at(&po, 8.0).urine_cum_ml_kg,
        at(&po, 8.0).k - po.points[0].k,
    );
    eprintln!("Koh PO : diurèse 8 h {pu8:.1} mL/kg, ΔK 8 h {pdk8:+.2}");
    assert!(pu8 > u8, "ordre IV < PO de Koh 2021 non reproduit");
    assert!((23.0..=40.0).contains(&pu8), "diurèse PO {pu8:.1} (30,9)");
    assert!(
        (-0.6..=-0.35).contains(&pdk8),
        "ΔK PO 8 h {pdk8:.2} (−0,46)"
    );
    // validation : Adin 2021, créatinine +23,9 % (ET 12,4) à 24 h (lu)
    let ad = renal_run(23.4, &adin_doses(), 24.0, |_, _| {});
    let cr = 100.0 * (at(&ad, 24.0).creat / ad.points[0].creat - 1.0);
    eprintln!("Adin : créatinine à 24 h {cr:+.0} %");
    assert!((11.5..=36.3).contains(&cr), "créatinine 24 h {cr:+.0} %");
    // contrôle d'échec : sans autorégulation du DFG vis-à-vis du débit, l'azotémie déborde
    let ad0 = renal_run(23.4, &adin_doses(), 24.0, |_, p| p.renal_co_auto = 1.0);
    let cr0 = 100.0 * (at(&ad0, 24.0).creat / ad0.points[0].creat - 1.0);
    assert!(cr0 > 36.3, "sans autorégulation : {cr0:+.0} %");
    // contrôle d'échec : sans tampon intracellulaire, le nadir de K est bien trop profond
    let nb = renal_run(11.5, &[(0.0, 2.0, Iv)], 8.0, |m, _| m.k_buffer_l_kg = 0.0);
    let nb2 = at(&nb, 2.0).k - nb.points[0].k;
    assert!(
        !(-0.6..=-0.42).contains(&nb2),
        "sans tampon : ΔK 2 h {nb2:.2}"
    );
    // contrôle d'échec : sans composante kaliémique de l'aldostérone (Young 1984), la kaliémie
    // ne remonte pas à 8 h
    let na = renal_run(11.5, &[(0.0, 2.0, Iv)], 8.0, |m, _| m.aldo_k0 = -1000.0);
    let na8 = at(&na, 8.0).k - na.points[0].k;
    assert!(
        !(-0.4..=-0.26).contains(&na8),
        "sans rétrocontrôle K → aldostérone : ΔK 8 h {na8:.2}"
    );
}

/// Charge orale de K (Bourgoignie 1981, 50 mEq de KCl par sonde ; résumé). Chien normal
/// (DFG 57 mL/min) : pic de K +0,9 à 1,2 mEq/L, 61–67 % de la charge excrétée en 5 h (calage).
/// Rein restant (DFG 16 mL/min, rapport 0,28) : pic +2,2 à 2,5, 30–37 % (validation de la
/// réserve d'excrétion proportionnelle au DFG).
#[test]
fn renal_potassium_load_scales_with_gfr() {
    // poids non rapporté : 17 kg (≈ 57 mL/min à 3,4 mL/min/kg) ; 50 mEq = 2,94 mmol/kg
    let load = [(0.0, 2.94)];
    let summary = |s: f64, tweak: &dyn Fn(&mut vetsim_core::renal::RenalModel)| {
        let ckd = ckd_params(s);
        let c = renal_run_k(17.0, &[], &load, 5.0, |m, p| {
            ckd(m, p);
            tweak(m);
        });
        let k0 = c.points[0].k;
        let peak = c.points.iter().map(|p| p.k - k0).fold(f64::MIN, f64::max);
        // fraction de la charge excrétée, nette de la kaliurèse de base
        let base = c.points[0].uk_mmol_kg_h * 5.0;
        let frac = (at(&c, 5.0).uk_cum_mmol_kg - base) / 2.94;
        let gfr = c.points[0].eval.as_ref().unwrap().metabolic.chem.gfr;
        (peak, frac, gfr)
    };
    let (pn, fn_, gn) = summary(0.0, &|_| {});
    let (pr, fr, gr) = summary(0.55, &|_| {});
    eprintln!(
        "normal : pic {pn:+.2}, {:.0} % ; rein restant (DFG ×{:.2}) : pic {pr:+.2}, {:.0} %",
        100.0 * fn_,
        gr / gn,
        100.0 * fr
    );
    assert!(
        (0.24..=0.32).contains(&(gr / gn)),
        "rapport de DFG {}",
        gr / gn
    );
    assert!((0.85..=1.25).contains(&pn), "pic normal {pn:.2}");
    assert!((0.55..=0.72).contains(&fn_), "fraction normale {fn_:.2}");
    // validation : la fraction excrétée suit le DFG
    assert!(
        (0.25..=0.40).contains(&fr),
        "fraction MRC {fr:.2} (30–37 %)"
    );
    assert!(pr > 1.2 * pn, "pic MRC {pr:.2} ≤ 1,2 × pic normal {pn:.2}");
    // contrôle d'échec : sans réserve d'excrétion au-dessus de la base, la charge n'est
    // presque pas excrétée en 5 h
    let (_, f0, _) = summary(0.0, &|m| m.k_sigma_up = 0.0);
    assert!(f0 < 0.55, "sans réserve : {f0:.2}");
}
