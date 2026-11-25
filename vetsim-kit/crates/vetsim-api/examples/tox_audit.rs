//! Audit de la toxicité (surdosage) des agents d'anesthésie et des halogénés (docs/VERIFICATION_TOXICITE.tex).
//!
//! Chien sain de 20 kg, vigile, air ambiant, non intubé (agents IV/IM ; `pAo2_min` : PAO2 dynamique) ; halogénés : intubé,
//! O2 pur, ventilation spontanée, vaporisateur à k × MAC pendant 30 min. Dose de référence × 1,
//! 3, 10 ; extrema sur 20 min (pas de 5 s). Sortie TSV.
//!
//! ```text
//! cargo run --release -p vetsim-api --example tox_audit > tox_audit.tsv
//! ```
//! Usage pédagogique uniquement : les doses « × k » ne sont pas des recommandations.
use vetsim_core::anesthesia::{AdminRoute, AnesthesiaModel, Event, InhalantModule, Session};
use vetsim_core::{DrugModule, Patient, PatientModule};

const AGENTS: [(&str, f64, AdminRoute); 12] = [
    ("acepromazine", 0.03, AdminRoute::Im),
    ("alfaxalone", 2.0, AdminRoute::Iv),
    ("atipamezole", 0.1, AdminRoute::Im),
    ("bupivacaine", 1.0, AdminRoute::Iv),
    ("dexmedetomidine", 0.01, AdminRoute::Im),
    ("fentanyl", 0.005, AdminRoute::Iv),
    ("ketamine", 5.0, AdminRoute::Iv),
    ("lidocaine", 2.0, AdminRoute::Iv),
    ("methadone", 0.3, AdminRoute::Iv),
    ("midazolam", 0.2, AdminRoute::Iv),
    ("naloxone", 0.04, AdminRoute::Iv),
    ("propofol", 4.0, AdminRoute::Iv),
];

fn r(p: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[derive(Default)]
struct Ext {
    hr: (f64, f64),
    map: (f64, f64),
    paco2: f64,
    pao2: f64,
    apnea_s: f64,
    cmax: f64,
    dead: Option<(f64, &'static str)>,
    mech: Vec<String>,
    conduction: bool,
}

fn run(s: &mut Session, dur_s: f64, agent: Option<&str>) -> Ext {
    let mut x = Ext {
        hr: (f64::INFINITY, 0.0),
        map: (f64::INFINITY, 0.0),
        pao2: f64::INFINITY,
        ..Default::default()
    };
    let dt = 5.0;
    let mut t = 0.0;
    while t < dur_s {
        let sn = match s.step(dt) {
            Ok(sn) => sn,
            Err(e) => {
                x.mech.push(format!("erreur:{e}"));
                break;
            }
        };
        t += dt;
        if let Some(d) = sn.dead {
            x.dead = Some((t, d));
            break;
        }
        if let Some(a) = agent {
            if let Some(l) = sn.agents.iter().find(|l| l.id == a) {
                x.cmax = x.cmax.max(l.cp);
            }
        }
        let Some(e) = sn.eval.as_ref() else { break };
        x.hr = (x.hr.0.min(e.params.hr), x.hr.1.max(e.params.hr));
        let m = e.hemo.summary.map;
        x.map = (x.map.0.min(m), x.map.1.max(m));
        x.paco2 = x.paco2.max(sn.paco2);
        // PAO2 dynamique de la session (l'évaluation statique ne suit pas l'apnée)
        x.pao2 = x.pao2.min(sn.pao2_alv);
        if sn.apnea {
            x.apnea_s += dt;
        }
        x.conduction |= e.conduction.is_some();
        for mm in &e.outcome.mechanisms {
            if !x.mech.contains(&mm.id.to_string()) {
                x.mech.push(mm.id.to_string());
            }
        }
    }
    x
}

fn main() {
    let pm = PatientModule::from_json(&r("modules/healthy_dog.json"), "p").unwrap();
    let base = Patient::healthy(&pm, 20.0).unwrap().params;
    let model =
        AnesthesiaModel::from_json(&r("modules/anesthesia/anesthesia_model.json"), "m").unwrap();
    let agents: Vec<DrugModule> = AGENTS
        .iter()
        .map(|(d, _, _)| {
            DrugModule::from_json(&r(&format!("modules/anesthesia/agents/{d}.json")), d).unwrap()
        })
        .collect();
    let inh: Vec<InhalantModule> = ["isoflurane", "sevoflurane"]
        .iter()
        .map(|g| {
            InhalantModule::from_json(&r(&format!("modules/anesthesia/inhalants/{g}.json")), g)
                .unwrap()
        })
        .collect();
    // filtre facultatif : `-- <molécule>` (exécution en parallèle, une molécule par processus)
    let only: Option<String> = std::env::args().nth(1);
    let keep = |id: &str| only.as_deref().map_or(true, |o| o == id);
    let new = || {
        Session::new(
            base.clone(),
            agents.clone(),
            inh.clone(),
            model.clone(),
            None,
            1,
        )
        .unwrap()
    };
    if only.is_none() {
        println!("molecule\tdose\tvoie\tcmax_mg_L\thr_min\thr_max\tmap_min\tmap_max\tpaco2_max\tpAo2_min\tapnee_s\tconduction\tdeces\tmecanismes");
        let e0 = run(&mut new(), 60.0, None);
        println!(
            "base\t0\t-\t0\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.1}\t{:.0}\t{}\t{}\t-\t{}",
            e0.hr.0,
            e0.hr.1,
            e0.map.0,
            e0.map.1,
            e0.paco2,
            e0.pao2,
            e0.apnea_s,
            e0.conduction,
            e0.mech.join(",")
        );
    }
    for (a, d, route) in AGENTS.into_iter().filter(|x| keep(x.0)) {
        // doses absolues facultatives (mg/kg) : `-- <molécule> 0.125,1,3`
        let doses: Vec<f64> = match std::env::args().nth(2) {
            Some(l) => l
                .split(',')
                .map(|x| x.parse::<f64>().unwrap() / d)
                .collect(),
            None => vec![1.0, 3.0, 10.0],
        };
        for k in doses {
            let mut s = new();
            let ev = Event::Bolus {
                agent: a.into(),
                mg_per_kg: d * k,
                route,
                duration_s: None,
            };
            if let Err(e) = s.apply(ev) {
                println!("{a}\t{}\t{route:?}\trefusé : {e}", d * k);
                continue;
            }
            let x = run(&mut s, 1200.0, Some(a));
            println!(
                "{a}\t{:.4}\t{route:?}\t{:.4}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.1}\t{:.0}\t{}\t{}\t{}\t{}",
                d * k,
                x.cmax,
                x.hr.0,
                x.hr.1,
                x.map.0,
                x.map.1,
                x.paco2,
                x.pao2,
                x.apnea_s,
                x.conduction,
                x.dead.map_or("-".into(), |(t, c)| format!("{c}@{t:.0}s")),
                x.mech.join(",")
            );
        }
    }
    // halogénés : induction propofol 5,2 mg/kg sous O2, intubation, vaporisateur à k × MAC
    // (plafonné au maximum du vaporisateur) pendant 40 min ; relevé de la MAC télé-expiratoire
    // au début de l'apnée et des extrema sur les 10 dernières minutes (régime quasi établi).
    for g in inh.iter().filter(|g| keep(&g.id)) {
        for k in [1.0, 1.5, 2.0, 2.5, 3.0, 4.0] {
            let mut s = new();
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
            let pct = (k * g.mac_pct).min(g.vaporizer_max_pct);
            s.apply(Event::Vaporizer {
                agent: g.id.clone(),
                pct,
            })
            .unwrap();
            let mut t = 0.0;
            let mut apnea_mac: Option<f64> = None;
            let mut last: (f64, f64, f64, f64, f64, f64) =
                (f64::INFINITY, 0.0, f64::INFINITY, 0.0, 0.0, 0.0);
            let mut dead = None;
            let mut mech: Vec<String> = Vec::new();
            while t < 2400.0 {
                let sn = s.step(10.0).unwrap();
                t += 10.0;
                if let Some(d) = sn.dead {
                    dead = Some((t, d));
                    break;
                }
                let fe_mac = sn
                    .inhalants
                    .iter()
                    .find(|l| l.id == g.id)
                    .map_or(0.0, |l| l.fe / g.mac_pct);
                // apnée après dissipation du propofol (> 10 min)
                if sn.apnea && t > 600.0 && apnea_mac.is_none() {
                    apnea_mac = Some(fe_mac);
                }
                let Some(e) = sn.eval.as_ref() else { break };
                if t >= 1800.0 {
                    let m = e.hemo.summary.map;
                    last = (
                        last.0.min(m),
                        last.1.max(m),
                        last.2.min(e.params.hr),
                        last.3.max(e.params.hr),
                        last.4.max(sn.paco2),
                        fe_mac,
                    );
                }
                for mm in &e.outcome.mechanisms {
                    if !mech.contains(&mm.id.to_string()) {
                        mech.push(mm.id.to_string());
                    }
                }
            }
            println!(
                "{}\t{k}MAC({pct:.2}%)\tinhal\tfe/MAC={:.2}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.1}\t-\tapnee@{}\t-\t{}\t{}",
                g.id, last.5, last.2, last.3, last.0, last.1, last.4,
                apnea_mac.map_or("-".into(), |m| format!("{m:.2}MAC")),
                dead.map_or("-".into(), |(t, c)| format!("{c}@{t:.0}s")),
                mech.join(",")
            );
        }
    }
}
