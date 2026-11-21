//! Sonde de calage du rein dynamique (docs/VALIDATION_HORS_CALIBRAGE.tex, « Rein dynamique ») :
//!
//! `cargo run --release -p vetsim-core --example renal_probe -- [clé=valeur ...]`
//!
//! Les clés surchargent les constantes de `modules/renal/renal_model.json` ; `p.<nom>=valeur`
//! surcharge un paramètre du patient (ex. `p.renal_co_auto=1.0`). Variables d'environnement :
//! `ONLY=koh|adin|chronic` (sous-ensemble des scénarios), `DT` (pas en h des scénarios de
//! plus de 30 h ; 0,25 par défaut), `ACE`, `NA_DIET`, `K_DIET` (leviers de `RenalOptions`),
//! `NODOSE`, `TEND` (scénario MRC), `VETSIM_ROOT` (racine du kit).
use std::path::PathBuf;
use vetsim_core::renal::{renal_course, RenalModel, RenalOptions};
use vetsim_core::{DiseaseModule, Dose, DrugModule, NoNoise, Patient, PatientModule, Route};

fn read(rel: &str) -> String {
    let root = std::env::var("VETSIM_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    std::fs::read_to_string(root.join(rel)).unwrap()
}

fn dose(t_h: f64, mg_per_kg: f64, route: Route) -> Dose {
    Dose {
        t_h,
        drug: "furosemide".into(),
        mg_per_kg,
        route,
    }
}

fn env(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let mut j: serde_json::Value =
        serde_json::from_str(&read("modules/renal/renal_model.json")).unwrap();
    let mut pover: Vec<(String, f64)> = Vec::new();
    for a in std::env::args().skip(1) {
        let (k, v) = a.split_once('=').expect("clé=valeur");
        let v: f64 = v.parse().expect("nombre");
        match k.strip_prefix("p.") {
            Some(pk) => pover.push((pk.to_string(), v)),
            None => j["values"][k]["value"] = serde_json::json!(v),
        }
    }
    let model = RenalModel::from_json(&j.to_string(), "renal").unwrap();
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let furo = DrugModule::from_json(&read("modules/drugs/furosemide.json"), "f").unwrap();
    let mmvd = DiseaseModule::from_json(&read("modules/diseases/mmvd.json"), "m").unwrap();
    let ckd = DiseaseModule::from_json(&read("modules/diseases/ckd.json"), "c").unwrap();
    let drugs = vec![furo];
    let dt_long: f64 = std::env::var("DT")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(0.25);
    let run = |label: &str,
               bw: f64,
               dis: Option<(&DiseaseModule, f64)>,
               doses: Vec<Dose>,
               t_end: f64,
               show: &[f64]| {
        let mut pt = Patient::healthy(&pm, bw).unwrap();
        for (k, v) in &pover {
            *pt.params.get_mut(k).expect("paramètre inconnu") = *v;
        }
        if let Some((d, s)) = dis {
            pt.add_disease(d, s, &mut NoNoise).unwrap();
        }
        let opt = RenalOptions {
            t_end_h: t_end,
            dt_h: if t_end > 30.0 { dt_long } else { 0.25 },
            ace_inhibition: env("ACE", 0.0),
            na_intake_factor: env("NA_DIET", 1.0),
            k_intake_factor: env("K_DIET", 1.0),
            k_loads: if env("KLOAD", 0.0) > 0.0 { vec![(0.0, env("KLOAD", 0.0))] } else { vec![] },
        };
        let tt = std::time::Instant::now();
        let c = renal_course(&model, &pt.params, &drugs, &doses, &opt).unwrap();
        println!(
            "== {label} (mort : {:?}) {:.1} s, {} évaluations",
            c.death,
            tt.elapsed().as_secs_f64(),
            c.n_evals
        );
        let p0 = &c.points[0];
        for &t in show {
            let p = c
                .points
                .iter()
                .min_by(|a, b| (a.t_h - t).abs().total_cmp(&(b.t_h - t).abs()))
                .unwrap();
            let ev = p.eval.as_ref().unwrap();
            println!(
                "t {:6.2} h  UK cum {:5.2} mmol/kg  urine cum {:6.2} mL/kg  UNa {:4.0} mmol/L  K {:.2} ΔK {:+.2}  ΔECF {:+6.1} mL/kg  A {:.2}  creat {:5.1} ({:+.0} %)  FE_Na {:.4}  FE_K {:.3}  E {:.2}  PAM {:5.1}  FC {:5.1}  DFG {:5.2}",
                p.t_h, p.uk_cum_mmol_kg, p.urine_cum_ml_kg, p.una_mmol_l, p.k, p.k - p0.k, p.d_ecf_ml_kg, p.aldo, p.creat,
                100.0 * (p.creat / p0.creat - 1.0), p.fe_na, p.fe_k, p.e_loop,
                ev.hemo.summary.map, ev.params.hr, ev.metabolic.chem.gfr / bw
            );
        }
    };
    let only = std::env::var("ONLY").unwrap_or_default();
    let want = |s: &str| only.is_empty() || only == s;
    if want("koh") {
        run(
            "Koh IV 2 mg/kg (11,5 kg)",
            11.5,
            None,
            vec![dose(0.0, 2.0, Route::Iv)],
            8.0,
            &[0.0, 1.0, 2.0, 4.0, 8.0],
        );
        run(
            "Koh PO 2 mg/kg (11,5 kg)",
            11.5,
            None,
            vec![dose(0.0, 2.0, Route::Po)],
            8.0,
            &[0.0, 2.0, 8.0],
        );
    }
    if want("adin") {
        let mut d = vec![dose(0.0, 0.66, Route::Iv)];
        d.extend((0..50).map(|i| dose(0.05 + 0.1 * i as f64, 0.066, Route::Iv)));
        run(
            "Adin 2021 perfusion 0,66 mg/kg/h 5 h (23,4 kg)",
            23.4,
            None,
            d,
            24.0,
            &[0.0, 5.0, 24.0],
        );
    }
    if want("kload") {
        // Bourgoignie 1981 : 50 mEq de KCl par sonde, DFG 57 (normal) contre 16 mL/min (rein
        // restant) ; poids non rapporté (≈ 17 kg pour 57 mL/min à 3,4 mL/min/kg)
        let grid: Vec<f64> = (0..=20).map(|i| 0.25 * i as f64).collect();
        for s in [0.0, env("KLOAD_S", 0.55)] {
            let d = if s > 0.0 { Some((&ckd, s)) } else { None };
            run(&format!("Charge de K, MRC {s}"), 17.0, d, vec![], 5.0, &grid);
        }
    }
    if want("ckd") {
        for s in [0.0, 0.375, 0.625, 0.85] {
            let lab = format!("MRC {s} : furosémide 2 mg/kg IV (20 kg)");
            let d = if s > 0.0 { Some((&ckd, s)) } else { None };
            let doses = if env("NODOSE", 0.0) > 0.0 {
                vec![]
            } else {
                vec![dose(0.0, 2.0, Route::Iv)]
            };
            run(
                &lab,
                20.0,
                d,
                doses,
                env("TEND", 24.0),
                &[0.0, 2.0, 8.0, 24.0, 72.0, 168.0],
            );
        }
    }
    if want("lantis") {
        let q12: Vec<Dose> = (0..20).map(|i| dose(12.0 * i as f64, 2.0, Route::Po)).collect();
        run("Lantis 2 mg/kg PO q12h 10 j (25 kg)", 25.0, None, q12, 240.0, &[0.0, 120.0, 240.0]);
    }
    if want("chronic") {
        let q12: Vec<Dose> = (0..20)
            .map(|i| dose(12.0 * i as f64, 2.0, Route::Po))
            .collect();
        run(
            "Lantis 2 mg/kg PO q12h 10 j (25 kg)",
            25.0,
            None,
            q12,
            240.0,
            &[0.0, 24.0, 120.0, 240.0],
        );
        let q24: Vec<Dose> = (0..7)
            .map(|i| dose(24.0 * i as f64, 1.0, Route::Iv))
            .collect();
        run(
            "Wong 1 mg/kg IV q24h 7 j (20 kg)",
            20.0,
            None,
            q24,
            168.0,
            &[0.0, 24.0, 168.0],
        );
        let giorgi: Vec<Dose> = (0..36)
            .map(|i| dose(8.0 * i as f64, 1.37, Route::Po))
            .collect();
        run(
            "Giorgi ≈ 4,1 mg/kg/j PO, MMVD 0,8 (20 kg)",
            20.0,
            Some((&mmvd, 0.8)),
            giorgi,
            288.0,
            &[0.0, 24.0, 288.0],
        );
    }
}
