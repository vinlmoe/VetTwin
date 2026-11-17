//! vetsim-cli : binaire de conformité (contrat : docs/CLI_CONTRACT.md).
//!
//! Sortie : une ligne de JSON sur stdout, code 0. Erreurs : message sur stderr, code ≠ 0
//! (2 = usage, 3 = modules invalides, 4 = erreur du moteur, 5 = fonction pas encore implémentée).
//! Mode déterministe : aucun bruit, aucune variabilité interindividuelle.
//!
//! Commandes : `eval` et `pk` (contrat), `params` (diagnostic : paramètres du patient après
//! application des maladies, pour la comparaison avec la référence Python).

use serde_json::{Map, Value as Json};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use vetsim_core::{Catalog, DiseaseModule, DrugModule, NoNoise, Patient, PatientModule};

const USAGE: &str = "\
usage :
  vetsim-cli [--modules DIR] eval   --bw KG [--disease id:sévérité ...] [--conc drug=mg_per_L ...] [--D mL] [--U mL_per_kg]
  vetsim-cli [--modules DIR] pk     --bw KG --drug ID --dose MG_PER_KG --route iv|po --times T1,T2,... [--disease id:sévérité ...]
  vetsim-cli [--modules DIR] params --bw KG [--disease id:sévérité ...]";

enum Failure {
    Usage(String),
    Modules(String),
    Engine(String),
    NotYet(&'static str),
}

impl Failure {
    fn report(self) -> ExitCode {
        let (code, msg) = match self {
            Failure::Usage(m) => (2, format!("{m}\n{USAGE}")),
            Failure::Modules(m) => (3, m),
            Failure::Engine(m) => (4, m),
            Failure::NotYet(m) => (5, m.to_string()),
        };
        eprintln!("vetsim-cli : {msg}");
        ExitCode::from(code)
    }
}

#[derive(Debug, Default)]
struct Args {
    command: String,
    modules: Option<PathBuf>,
    bw: Option<f64>,
    diseases: Vec<(String, f64)>,
    conc: Vec<(String, f64)>,
    d: Option<f64>,
    u: Option<f64>,
    drug: Option<String>,
    dose: Option<f64>,
    route: Option<String>,
    times: Option<Vec<f64>>,
}

fn num(flag: &str, v: &str) -> Result<f64, Failure> {
    v.trim()
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
        .ok_or_else(|| Failure::Usage(format!("{flag} : nombre attendu, « {v} » reçu")))
}

fn parse_args(argv: &[String]) -> Result<Args, Failure> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(tok) = it.next() {
        // Accepte `--flag valeur` et `--flag=valeur`.
        let (flag, inline) = match tok.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (tok.as_str(), None),
        };
        if !flag.starts_with("--") {
            if a.command.is_empty() {
                a.command = flag.to_string();
                continue;
            }
            return Err(Failure::Usage(format!("argument inattendu « {tok} »")));
        }
        let mut value = || -> Result<String, Failure> {
            match &inline {
                Some(v) => Ok(v.clone()),
                None => it
                    .next()
                    .cloned()
                    .ok_or_else(|| Failure::Usage(format!("{flag} : valeur manquante"))),
            }
        };
        match flag {
            "--modules" => a.modules = Some(PathBuf::from(value()?)),
            "--bw" => a.bw = Some(num(flag, &value()?)?),
            "--disease" => {
                let v = value()?;
                let (id, s) = v.rsplit_once(':').ok_or_else(|| {
                    Failure::Usage(format!(
                        "--disease : forme id:sévérité attendue, « {v} » reçu"
                    ))
                })?;
                a.diseases.push((id.to_string(), num(flag, s)?));
            }
            "--conc" => {
                let v = value()?;
                let (id, c) = v.split_once('=').ok_or_else(|| {
                    Failure::Usage(format!(
                        "--conc : forme drug=mg_per_L attendue, « {v} » reçu"
                    ))
                })?;
                a.conc.push((id.to_string(), num(flag, c)?));
            }
            "--D" => a.d = Some(num(flag, &value()?)?),
            "--U" => a.u = Some(num(flag, &value()?)?),
            "--drug" => a.drug = Some(value()?),
            "--dose" => a.dose = Some(num(flag, &value()?)?),
            "--route" => {
                let r = value()?;
                if r != "iv" && r != "po" {
                    return Err(Failure::Usage(format!(
                        "--route : iv ou po attendu, « {r} » reçu"
                    )));
                }
                a.route = Some(r);
            }
            "--times" => {
                let v = value()?;
                let t = v
                    .split(',')
                    .map(|x| num(flag, x))
                    .collect::<Result<Vec<_>, _>>()?;
                a.times = Some(t);
            }
            "--help" | "-h" => return Err(Failure::Usage(String::new())),
            other => return Err(Failure::Usage(format!("option inconnue « {other} »"))),
        }
    }
    let allowed: &[&str] = match a.command.as_str() {
        "eval" => &["bw", "disease", "conc", "D", "U"],
        "pk" => &["bw", "disease", "drug", "dose", "route", "times"],
        "params" => &["bw", "disease"],
        "" => return Err(Failure::Usage("commande manquante".into())),
        other => return Err(Failure::Usage(format!("commande inconnue « {other} »"))),
    };
    let used = [
        ("conc", !a.conc.is_empty()),
        ("D", a.d.is_some()),
        ("U", a.u.is_some()),
        ("drug", a.drug.is_some()),
        ("dose", a.dose.is_some()),
        ("route", a.route.is_some()),
        ("times", a.times.is_some()),
    ];
    for (name, set) in used {
        if set && !allowed.contains(&name) {
            return Err(Failure::Usage(format!(
                "--{name} n'est pas une option de « {} »",
                a.command
            )));
        }
    }
    if a.command == "pk" {
        for (name, set) in [
            ("drug", a.drug.is_some()),
            ("dose", a.dose.is_some()),
            ("route", a.route.is_some()),
            ("times", a.times.is_some()),
        ] {
            if !set {
                return Err(Failure::Usage(format!("pk : --{name} est requis")));
            }
        }
    }
    Ok(a)
}

/// Lit `DIR/healthy_dog.json`, `DIR/diseases/*.json`, `DIR/drugs/*.json` (ordre alphabétique)
/// et rapporte toutes les erreurs de chargement d'un coup.
fn load_catalog(dir: &Path) -> Result<Catalog, Failure> {
    let mut errors = Vec::new();
    let read = |p: &Path| {
        std::fs::read_to_string(p)
            .map_err(|e| format!("{} : lecture impossible ({e})", p.display()))
    };
    let list = |sub: &str| -> Result<Vec<PathBuf>, String> {
        let d = dir.join(sub);
        let mut v: Vec<PathBuf> = std::fs::read_dir(&d)
            .map_err(|e| format!("{} : lecture impossible ({e})", d.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        v.sort();
        Ok(v)
    };
    let patient_path = dir.join("healthy_dog.json");
    let patient = read(&patient_path).and_then(|s| {
        PatientModule::from_json(&s, &patient_path.display().to_string()).map_err(|e| e.to_string())
    });
    let mut diseases = Vec::new();
    let mut drugs = Vec::new();
    match list("diseases") {
        Ok(files) => {
            for f in files {
                match read(&f).and_then(|s| {
                    DiseaseModule::from_json(&s, &f.display().to_string())
                        .map_err(|e| e.to_string())
                }) {
                    Ok(m) => diseases.push(m),
                    Err(e) => errors.push(e),
                }
            }
        }
        Err(e) => errors.push(e),
    }
    match list("drugs") {
        Ok(files) => {
            for f in files {
                match read(&f).and_then(|s| {
                    DrugModule::from_json(&s, &f.display().to_string()).map_err(|e| e.to_string())
                }) {
                    Ok(m) => drugs.push(m),
                    Err(e) => errors.push(e),
                }
            }
        }
        Err(e) => errors.push(e),
    }
    let patient = match patient {
        Ok(p) => Some(p),
        Err(e) => {
            errors.insert(0, e);
            None
        }
    };
    match (patient, errors.is_empty()) {
        (Some(p), true) => {
            Catalog::new(p, diseases, drugs).map_err(|e| Failure::Modules(e.to_string()))
        }
        _ => Err(Failure::Modules(errors.join("\n"))),
    }
}

fn build_patient(cat: &Catalog, a: &Args) -> Result<Patient, Failure> {
    let bw = a.bw.unwrap_or(20.0);
    let mut pt = Patient::healthy(&cat.patient, bw).map_err(|e| Failure::Engine(e.to_string()))?;
    for (id, s) in &a.diseases {
        let module = cat.disease(id).ok_or_else(|| {
            let known: Vec<&str> = cat.diseases.iter().map(|d| d.id.as_str()).collect();
            Failure::Engine(format!(
                "maladie inconnue « {id} » (disponibles : {})",
                known.join(", ")
            ))
        })?;
        pt.add_disease(module, *s, &mut NoNoise)
            .map_err(|e| Failure::Engine(e.to_string()))?;
    }
    Ok(pt)
}

fn check_drug(cat: &Catalog, id: &str) -> Result<(), Failure> {
    if cat.drug(id).is_some() {
        Ok(())
    } else {
        let known: Vec<&str> = cat.drugs.iter().map(|d| d.id.as_str()).collect();
        Err(Failure::Engine(format!(
            "médicament inconnu « {id} » (disponibles : {})",
            known.join(", ")
        )))
    }
}

fn run(argv: &[String]) -> Result<Json, Failure> {
    let a = parse_args(argv)?;
    let dir = a
        .modules
        .clone()
        .unwrap_or_else(|| PathBuf::from("modules"));
    let cat = load_catalog(&dir)?;
    let pt = build_patient(&cat, &a)?;
    match a.command.as_str() {
        "params" => {
            let mut m = Map::new();
            for (k, v) in pt.params.to_vec() {
                m.insert(k.to_string(), Json::from(v));
            }
            Ok(Json::Object(m))
        }
        "eval" => {
            for (id, _) in &a.conc {
                check_drug(&cat, id)?;
            }
            Err(Failure::NotYet(
                "eval : l'hémodynamique et les gaz du sang ne sont pas encore implémentés (jalons M2-M3)",
            ))
        }
        "pk" => {
            check_drug(&cat, a.drug.as_deref().unwrap_or_default())?;
            Err(Failure::NotYet(
                "pk : la PK/PD n'est pas encore implémentée (jalon M4)",
            ))
        }
        _ => unreachable!("commande validée par parse_args"),
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(&argv) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(f) => f.report(),
    }
}
