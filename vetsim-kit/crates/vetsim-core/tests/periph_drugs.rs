//! Périphérie et relations dose-effet des médicaments (19/11/2026) : cibles publiées chez le
//! chien, simulées de bout en bout (PK, site d'effet, effets, baroréflexe, thermorégulation).
//! Sources et accès : modules/drugs/*.json (`refs`, `notes`, `toxicity[].source`).

use std::path::PathBuf;
use vetsim_core::periph::{self, thermal_course};
use vetsim_core::{
    apply_drug_effects_sites, evaluate, timecourse, Dose, DrugModule, Evaluation, Params,
    PatientModule, Route,
};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn drugs() -> Vec<DrugModule> {
    [
        "atropine",
        "betabloquant",
        "furosemide",
        "opioide",
        "pimobendan",
    ]
    .iter()
    .map(|d| DrugModule::from_json(&read(&format!("modules/drugs/{d}.json")), d).unwrap())
    .collect()
}

fn healthy() -> Params {
    let m = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    vetsim_core::healthy_params(&m, 20.0).unwrap()
}

fn dose(drug: &str, mg: f64, route: Route, t_h: f64) -> Dose {
    Dose {
        t_h,
        drug: drug.into(),
        mg_per_kg: mg,
        route,
    }
}

/// Paramètres effectifs à l'instant `t` (h) d'un schéma, température comprise.
fn params_at(p: &Params, ds: &[Dose], t: f64, thermal: bool) -> Params {
    let dr = drugs();
    let tc = timecourse(p, &dr, ds, t, &[t]).unwrap();
    let at = |v: &[(String, Vec<f64>)]| -> Vec<(String, f64)> {
        v.iter().map(|(d, x)| (d.clone(), x[0])).collect()
    };
    let mut q =
        apply_drug_effects_sites(p, &dr, &at(&tc.conc), &at(&tc.conc_e), tc.d[0], tc.u[0]).unwrap();
    if thermal {
        q.temp_c = thermal_course(p, &dr, ds, &[t]).unwrap()[0].tc;
    }
    q
}

fn eval_at(p: &Params, ds: &[Dose], t: f64) -> Evaluation {
    evaluate(&params_at(p, ds, t, false)).unwrap()
}

fn hr_at(ds: &[Dose], t: f64) -> f64 {
    eval_at(&healthy(), ds, t).params.hr
}

#[test]
fn atropine_dose_heart_rate_relation() {
    let hr0 = evaluate(&healthy()).unwrap().params.hr;
    // +50 % de FC à 39,5 ± 6,2 µg/kg IV (Hendrix 1997) : au pic d'effet (≈ 5 min)
    let peak = (1..=15)
        .map(|m| hr_at(&[dose("atropine", 0.0395, Route::Iv, 0.0)], m as f64 / 60.0))
        .fold(0.0, f64::max);
    assert!(
        (peak / hr0 - 1.5).abs() < 0.08,
        "39,5 µg/kg IV : FC {peak:.1} / {hr0:.1}"
    );
    // 0,06 mg/kg cumulés (6 × 0,01 IV) : blocage quasi complet, FC × 1,9 (Little 1999) ;
    // le modèle sature à × 1,74 au blocage complet
    let ds: Vec<Dose> = (0..6)
        .map(|i| dose("atropine", 0.01, Route::Iv, i as f64 * 2.0 / 60.0))
        .collect();
    let hr6 = hr_at(&ds, 12.0 / 60.0);
    assert!(hr6 / hr0 > 1.6, "0,06 mg/kg cumulés : {hr6:.1}");
    // +26 ± 6 bpm au-delà de la FC de vagotomie à 0,1 mg/kg IV (Rigel 1984)
    let hr01 = hr_at(&[dose("atropine", 0.1, Route::Iv, 0.0)], 3.0 / 60.0);
    let mut vag = healthy();
    vag.vagal_block = 1.0;
    let hr_vag = evaluate(&vag).unwrap().params.hr;
    assert!(
        (5.0..45.0).contains(&(hr01 - hr_vag)),
        "tachycardie excessive {hr01:.1} vs vagotomie {hr_vag:.1}"
    );
}

#[test]
fn atropine_paradoxical_bradycardia_at_low_dose() {
    let hr0 = evaluate(&healthy()).unwrap().params.hr;
    // 6,25 µg/kg IV : ralentissement du nœud sinusal (Kantelip 1985)
    let low = (1..=10)
        .map(|m| {
            hr_at(
                &[dose("atropine", 0.00625, Route::Iv, 0.0)],
                m as f64 / 60.0,
            )
        })
        .fold(f64::INFINITY, f64::min);
    assert!(low < hr0 - 3.0, "6,25 µg/kg : FC min {low:.1} / {hr0:.1}");
    // 0,02 mg/kg IM : bradycardie transitoire (≈ 4,6 min, Rishniw 1996) puis tachycardie
    let ds = [dose("atropine", 0.02, Route::Im, 0.0)];
    let early = hr_at(&ds, 1.5 / 60.0);
    let late = hr_at(&ds, 20.0 / 60.0);
    assert!(early < hr0, "IM, 1,5 min : {early:.1} / {hr0:.1}");
    assert!(late > hr0 + 5.0, "IM, 20 min : {late:.1} / {hr0:.1}");
}

#[test]
fn atropine_glands_gut_and_pupil() {
    let p = healthy();
    // motilité du grêle −93 ± 13,9 % à 39,5 µg/kg IV (Hendrix 1997), à 10 min
    let e = eval_at(&p, &[dose("atropine", 0.0395, Route::Iv, 0.0)], 10.0 / 60.0);
    let per = e.peripheral();
    assert!(per.gi_motility < 0.2, "{per:?}");
    assert!(per.salivation < 0.2);
    // SOI 47,9 → 13,2 mmHg sous atropine (Strombeck 1985)
    assert!(per.les_mmhg < 20.0);
    // 0,02 mg/kg IM : arrêt antral ≥ 30 min (Burger 2006)
    let e = eval_at(&p, &[dose("atropine", 0.02, Route::Im, 0.0)], 30.0 / 60.0);
    assert!(e.peripheral().gi_motility < 0.3, "{:?}", e.peripheral());
    // 0,06 mg/kg IM : mydriase bilatérale au pic (30 ± 11,6 min, Kovalcuka 2015 ; amplitude en
    // mm non publiée dans le résumé : critère qualitatif) et réflexe photomoteur déprimé
    let base = evaluate(&p).unwrap().peripheral().pupil.diameter;
    let pup = |m: f64| {
        eval_at(&p, &[dose("atropine", 0.06, Route::Im, 0.0)], m / 60.0)
            .peripheral()
            .pupil
    };
    let at30 = pup(30.0);
    let base_plr = evaluate(&p).unwrap().peripheral().pupil.plr_pct;
    assert!(at30.diameter > base + 0.5, "{at30:?} vs {base}");
    assert!(
        at30.plr_pct < 0.8 * base_plr,
        "réflexe photomoteur : {at30:?}"
    );
    // pic de mydriase entre 10 et 45 min
    let tmax = (1..=90).max_by(|a, b| {
        pup(*a as f64)
            .diameter
            .partial_cmp(&pup(*b as f64).diameter)
            .unwrap()
    });
    assert!((10..=45).contains(&tmax.unwrap()), "pic à {tmax:?} min");
    // retour vers la base quand le médicament s'élimine
    assert!(pup(360.0).diameter < at30.diameter);
}

#[test]
fn opioid_hypothermia_myosis_and_gut() {
    let p = healthy();
    // perfusion de méthadone 0,1 mg/kg/h après 0,2 mg/kg (Amon 2021) : T 38,4 → 37,1 °C
    let mut ds = vec![dose("opioide", 0.2, Route::Iv, 0.0)];
    ds.extend((1..=48).map(|i| dose("opioide", 0.05, Route::Iv, i as f64 * 0.5)));
    let th = thermal_course(&p, &drugs(), &ds, &[0.0, 12.0, 24.0]).unwrap();
    let drop = th[2].tc - th[0].tc;
    assert!((-2.2..=-0.8).contains(&drop), "méthadone : ΔT {drop:.2} °C");
    let q = params_at(&p, &ds, 24.0, true);
    let e = evaluate(&q).unwrap();
    let per = e.peripheral();
    assert!(per.thermo.panting >= 0.0);
    assert!(per.gi_motility < 0.5, "{per:?}");
    // sédation légère seulement (Amon 2021, KuKanich 2008) : score < 7/21
    assert!(
        per.sedation_score > 2.0 && per.sedation_score < 7.0,
        "{per:?}"
    );
    assert!(per.pupil.diameter < 8.53, "myosis : {per:?}");
}

#[test]
fn pimobendan_shortens_pr() {
    let p = healthy();
    let q = params_at(&p, &[dose("pimobendan", 0.15, Route::Iv, 0.0)], 1.0, false);
    // PQ −11,8 % à 60 min (Pichayapaiboon 2021)
    let r = q.pr / p.pr;
    assert!((0.85..0.93).contains(&r), "PR × {r:.3}");
}

#[test]
fn furosemide_haemoconcentration() {
    let p = healthy();
    let base = evaluate(&p).unwrap();
    let e = eval_at(&p, &[dose("furosemide", 2.0, Route::Iv, 0.0)], 2.0);
    let alb = e.metabolic.chem.alb / base.metabolic.chem.alb;
    // protéines 5,90 → 6,32 g/dL (+7 %) à 2 h de 2 mg/kg IV ; Ht inchangé (Lee 2021)
    assert!((1.03..1.12).contains(&alb), "albumine × {alb:.3}");
    let hct = e.metabolic.chem.hct / base.metabolic.chem.hct;
    assert!((1.0..1.07).contains(&hct), "Ht × {hct:.3}");
}

#[test]
fn temperature_feeds_back_on_heart_rate_and_oxygen() {
    let mut p = healthy();
    let e0 = evaluate(&p).unwrap();
    p.temp_c = 34.0;
    let e = evaluate(&p).unwrap();
    // FC −17 % de 38,5 à ≈ 34 °C sous isoflurane (Yoshida 2001) : intrinsèque, avant baroréflexe
    assert!(
        e.params.hr < e0.params.hr,
        "{} vs {}",
        e.params.hr,
        e0.params.hr
    );
    // P50 abaissée (Bohr thermique) : ΔlogP50 = 0,021 × ΔT
    assert!(e.metabolic.art.p50 < e0.metabolic.art.p50);
    // frisson : VO2 augmentée chez le vigile hypotherme
    assert!(e.peripheral().thermo.shiver > 0.0);
    // CAM effective accrue en hypothermie (Wilson 2006)
    p.anesth_mac = 1.0;
    let per = evaluate(&p).unwrap().peripheral();
    assert!(per.mac_eff > 1.3 && per.thermo.shiver == 0.0, "{per:?}");
    let _ = periph::T_REF;
}
