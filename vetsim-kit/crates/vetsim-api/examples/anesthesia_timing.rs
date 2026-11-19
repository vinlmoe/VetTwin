//! Temps de calcul d'un pas de session sous dexmédétomidine 10 µg/kg IM (diagnostic) et petite
//! grille de calage de ses effets : `-- [--all] [--rsys x] [--s0 x] [--vagal x]` (docs/ANESTHESIE.tex).
use std::time::Instant;
use vetsim_core::anesthesia::{AdminRoute, AnesthesiaModel, Event, Session};
use vetsim_core::{DrugModule, Patient, PatientModule};
fn main() {
    let r = |p: &str| {
        std::fs::read_to_string(format!("{}/../../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    };
    let pm = PatientModule::from_json(&r("modules/healthy_dog.json"), "p").unwrap();
    let base = Patient::healthy(&pm, 20.0).unwrap().params;
    let model =
        AnesthesiaModel::from_json(&r("modules/anesthesia/anesthesia_model.json"), "m").unwrap();
    let mut dex =
        DrugModule::from_json(&r("modules/anesthesia/agents/dexmedetomidine.json"), "d").unwrap();
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--vagal") {
        let v: f64 = args[i + 1].parse().unwrap();
        let j = serde_json::json!({"mul": [v, "E"]});
        let mut issues = Vec::new();
        dex.pd.effects[2].expr =
            vetsim_core::dsl::Expr::parse(&j, "", &["E", "EP"], &mut issues).unwrap();
    }
    if let Some(i) = args.iter().position(|a| a == "--rsys") {
        let v: f64 = args[i + 1].parse().unwrap();
        let j = serde_json::json!({"add": [1, {"mul": [v, "EP"]}]});
        let mut issues = Vec::new();
        dex.pd.effects[0].expr =
            vetsim_core::dsl::Expr::parse(&j, "", &["E", "EP"], &mut issues).unwrap();
    }
    if let Some(i) = args.iter().position(|a| a == "--s0") {
        let v: f64 = args[i + 1].parse().unwrap();
        let j = serde_json::json!({"mul": [-v, "E"]});
        let mut issues = Vec::new();
        dex.pd.effects[1].expr =
            vetsim_core::dsl::Expr::parse(&j, "", &["E", "EP"], &mut issues).unwrap();
    }
    let all = std::env::args().any(|a| a == "--all") || args.iter().any(|a| a == "--bolus");
    let mut agents = vec![dex];
    let mut inh = vec![];
    if all {
        for d in [
            "acepromazine",
            "alfaxalone",
            "atipamezole",
            "bupivacaine",
            "fentanyl",
            "ketamine",
            "lidocaine",
            "methadone",
            "midazolam",
            "naloxone",
            "propofol",
        ] {
            agents.push(
                DrugModule::from_json(&r(&format!("modules/anesthesia/agents/{d}.json")), d)
                    .unwrap(),
            );
        }
        for g in ["isoflurane", "sevoflurane"] {
            inh.push(
                vetsim_core::anesthesia::InhalantModule::from_json(
                    &r(&format!("modules/anesthesia/inhalants/{g}.json")),
                    g,
                )
                .unwrap(),
            );
        }
    }
    let mut s = Session::new(base, agents, inh, model, None, 1).unwrap();
    let (agent, dose, dt) = match args.iter().position(|a| a == "--bolus") {
        Some(i) => (
            args[i + 1].clone(),
            args[i + 2].parse::<f64>().unwrap(),
            5.0,
        ),
        None => ("dexmedetomidine".to_string(), 0.01, 30.0),
    };
    let route = if agent == "dexmedetomidine" {
        AdminRoute::Im
    } else {
        AdminRoute::Iv
    };
    s.apply(Event::Bolus {
        agent,
        mg_per_kg: dose,
        route,
        duration_s: None,
    })
    .unwrap();
    for i in 0..40 {
        let t = Instant::now();
        let sn = s.step(dt).unwrap();
        let Some(e) = sn.eval.as_ref() else {
            println!("{i} décès {:?}", sn.dead);
            break;
        };
        println!(
            "{i} {:.0} ms hr {:.0} map {:.0} co {:.2} block {} iter {} loc {:.2}",
            t.elapsed().as_secs_f64() * 1e3,
            e.params.hr,
            e.hemo.summary.map,
            e.hemo.summary.co,
            e.conduction.is_some(),
            e.decomp.iterations,
            sn.depth.loc_index
        );
    }
}
