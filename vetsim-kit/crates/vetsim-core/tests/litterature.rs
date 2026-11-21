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
