//! Banc « maladie × médicament × dose » (docs/VERIFICATION_MALADIES_MEDICAMENTS.tex).
//!
//! Patient de 20 kg sans variabilité (tirages sans bruit), dose unique à t = 0, états aux
//! instants `T`. Pour chaque scénario : instant retenu (extremum de la variable clé du
//! médicament, ou 12 h pour le furosémide), concentration, état, mécanismes de
//! décompensation et instant du décès éventuel. Sortie TSV sur stdout.
//!
//! ```text
//! cargo run --release -p vetsim-api --example drug_matrix [-- --modules modules] > matrice.tsv
//! ```
//!
//! Usage pédagogique uniquement : les doses « × k » ne sont pas des recommandations.

use serde_json::{json, Value as Json};
use std::fs;
use std::path::Path;

const T: [f64; 9] = [0.0, 0.0833, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 12.0];
const MULT: [f64; 5] = [1.0, 3.0, 10.0, 30.0, 100.0];
const KEYS: [&str; 14] = [
    "hr", "map", "co", "la_mean", "rr", "pao2", "pco2", "ph", "k", "na", "cl", "hco3", "creat",
    "lactate",
];

/// Médicament, dose de référence (mg/kg), voie, variable clé, extremum (true = max).
const DRUGS: [(&str, f64, &str, &str, bool); 5] = [
    ("furosemide", 2.0, "iv", "k", false),
    ("pimobendan", 0.25, "po", "map", false),
    ("atropine", 0.04, "iv", "hr", true),
    ("betabloquant", 1.0, "po", "hr", false),
    ("opioide", 0.3, "iv", "pco2", true),
];

const DISEASES: [(&str, &str, f64); 12] = [
    ("sain", "", 0.0),
    ("mmvd_B2", "mmvd", 0.45),
    ("mmvd_C", "mmvd", 0.7),
    ("mmvd_D", "mmvd", 0.9),
    ("ckd_IRIS3", "ckd", 0.625),
    ("addison_crise", "addison", 0.85),
    ("pneumonie_severe", "pneumonia", 0.8),
    ("bav_2", "av_block", 0.5),
    ("bav_3", "av_block", 0.85),
    ("maladie_sinus", "sick_sinus", 0.6),
    ("fa_rapide", "atrial_fibrillation", 0.8),
    ("esv", "ventricular_premature", 0.5),
];

fn read(p: &Path) -> Json {
    let s = fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn dir_modules(d: &Path) -> Vec<Json> {
    let mut paths: Vec<_> = fs::read_dir(d)
        .unwrap_or_else(|e| panic!("{}: {e}", d.display()))
        .map(|e| e.expect("entrée de dossier").path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths.iter().map(|p| read(p)).collect()
}

fn ok(s: &str) -> Json {
    let v: Json = serde_json::from_str(s).expect("réponse JSON de l'API");
    assert_eq!(v["ok"], true, "{s}");
    v
}

fn fmt(v: &Json) -> String {
    v.as_f64()
        .map_or_else(|| "NA".into(), |x| format!("{x:.4}"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args
        .iter()
        .position(|a| a == "--modules")
        .and_then(|i| args.get(i + 1))
        .map_or("modules", String::as_str);
    let m = Path::new(dir);
    let mods = json!({
        "patient": read(&m.join("healthy_dog.json")),
        "diseases": dir_modules(&m.join("diseases")),
        "drugs": dir_modules(&m.join("drugs")),
        "auscultation": read(&m.join("auscultation.json")),
    });
    let mut e = vetsim_api::Engine::new();
    ok(&e.load_modules(&mods.to_string()));

    println!(
        "maladie\tmedicament\tmultiple\tdose_mg_kg\tt_h\tconc_mg_L\t{}\tetat\tmecanismes\tdeces_t_h\tcause",
        KEYS.join("\t")
    );
    for (name, id, sev) in DISEASES {
        let new_patient = |e: &mut vetsim_api::Engine| {
            let p = ok(&e.new_patient(&json!({"bw": 20, "variability": false}).to_string()));
            let pid = p["patient_id"].as_str().expect("patient_id").to_string();
            if !id.is_empty() {
                ok(&e.add_disease(&pid, &json!({"id": id, "severity": sev}).to_string()));
            }
            pid
        };
        let pid = new_patient(&mut e);
        let st = ok(&e.state(&pid, "{}"));
        let mech: Vec<String> = st["outcome"]["mechanisms"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| x["id"].as_str().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default();
        println!(
            "{name}\taucun\t0\t0\t0\t0\t{}\t{}\t{}\t\t{}",
            KEYS.iter()
                .map(|k| fmt(&st[*k]))
                .collect::<Vec<_>>()
                .join("\t"),
            st["outcome"]["state"].as_str().unwrap_or(""),
            mech.join(","),
            st["outcome"]["cause"].as_str().unwrap_or("")
        );
        for (drug, dose, route, key, maximize) in DRUGS {
            for k in MULT {
                let pid = new_patient(&mut e);
                let req = json!({
                    "doses": [{"t_h": 0, "drug": drug, "mg_per_kg": dose * k, "route": route}],
                    "t_end_h": 12, "t_eval_h": T, "state_times_h": T,
                });
                let tc = ok(&e.timecourse(&pid, &req.to_string()));
                let states = tc["states_at"].as_array().expect("states_at");
                let conc = tc["conc_mg_L"][drug].as_array().expect("conc");
                let alive: Vec<usize> = (0..states.len())
                    .filter(|&i| states[i]["outcome"] != "dead")
                    .collect();
                let death = &tc["death_t_h"];
                let cause = states
                    .iter()
                    .find(|s| s["outcome"] == "dead")
                    .map_or("", |s| s["cause"].as_str().unwrap_or(""));
                let mut mechs: Vec<String> = alive
                    .iter()
                    .flat_map(|&i| {
                        states[i]["mechanisms"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                    })
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect();
                mechs.sort();
                mechs.dedup();
                // Instant retenu : extremum de la variable clé après t = 0 (12 h pour le
                // furosémide, dont les effets électrolytiques sont cumulatifs).
                let pick = if drug == "furosemide" {
                    alive.last().copied()
                } else {
                    let cand: Vec<usize> = alive.iter().copied().filter(|&i| i > 0).collect();
                    let cand = if cand.is_empty() { alive.clone() } else { cand };
                    let val = |i: &usize| states[*i][key].as_f64().unwrap_or(f64::NAN);
                    if maximize {
                        cand.into_iter().max_by(|a, b| val(a).total_cmp(&val(b)))
                    } else {
                        cand.into_iter().min_by(|a, b| val(a).total_cmp(&val(b)))
                    }
                };
                let (t, c, vals, state) = match pick {
                    Some(i) => (
                        format!("{}", T[i]),
                        fmt(&conc[i]),
                        KEYS.iter()
                            .map(|k| fmt(&states[i][*k]))
                            .collect::<Vec<_>>()
                            .join("\t"),
                        states[i]["outcome"].as_str().unwrap_or("").to_string(),
                    ),
                    None => (
                        "0".into(),
                        fmt(&conc[0]),
                        vec!["NA"; KEYS.len()].join("\t"),
                        "dead".into(),
                    ),
                };
                println!(
                    "{name}\t{drug}\t{k}\t{}\t{t}\t{c}\t{vals}\t{state}\t{}\t{}\t{cause}",
                    (dose * k * 1e6).round() / 1e6,
                    mechs.join(","),
                    death.as_f64().map_or(String::new(), |x| format!("{x}"))
                );
            }
        }
    }
}
