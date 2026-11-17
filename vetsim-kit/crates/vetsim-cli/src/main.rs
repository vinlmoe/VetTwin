//! vetsim-cli : binaire de conformité (contrat : docs/CLI_CONTRACT.md).
//!
//! Sortie : une ligne de JSON sur stdout, code 0. Erreurs : message sur stderr, code ≠ 0
//! (2 = usage, 3 = modules invalides, 4 = erreur du moteur).
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
  vetsim-cli [--modules DIR] params --bw KG [--disease id:sévérité ...]
  vetsim-cli [--modules DIR] calibrate [--targets FICHIER] [--n-healthy 50] [--n-disease 24] [--seed0 100] [--n-hrv 20]
  vetsim-cli [--modules DIR] signals --bw KG [--disease id:sévérité ...] [--conc ...] [--seed 0] [--duration 8]";

enum Failure {
    Usage(String),
    Modules(String),
    Engine(String),
}

impl Failure {
    fn report(self) -> ExitCode {
        let (code, msg) = match self {
            Failure::Usage(m) => (2, format!("{m}\n{USAGE}")),
            Failure::Modules(m) => (3, m),
            Failure::Engine(m) => (4, m),
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
    targets: Option<PathBuf>,
    n_healthy: Option<usize>,
    n_disease: Option<usize>,
    seed0: Option<u64>,
    n_hrv: Option<usize>,
    seed: Option<u64>,
    duration: Option<f64>,
}

fn count(flag: &str, v: &str) -> Result<usize, Failure> {
    v.trim()
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| Failure::Usage(format!("{flag} : entier > 0 attendu, « {v} » reçu")))
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
            "--targets" => a.targets = Some(PathBuf::from(value()?)),
            "--n-healthy" => a.n_healthy = Some(count(flag, &value()?)?),
            "--n-disease" => a.n_disease = Some(count(flag, &value()?)?),
            "--n-hrv" => a.n_hrv = Some(count(flag, &value()?)?),
            "--duration" => a.duration = Some(num(flag, &value()?)?),
            "--seed" => {
                let v = value()?;
                a.seed = Some(v.trim().parse().map_err(|_| {
                    Failure::Usage(format!("--seed : entier ≥ 0 attendu, « {v} » reçu"))
                })?);
            }
            "--seed0" => {
                let v = value()?;
                a.seed0 = Some(v.trim().parse().map_err(|_| {
                    Failure::Usage(format!("--seed0 : entier ≥ 0 attendu, « {v} » reçu"))
                })?);
            }
            "--help" | "-h" => return Err(Failure::Usage(String::new())),
            other => return Err(Failure::Usage(format!("option inconnue « {other} »"))),
        }
    }
    let allowed: &[&str] = match a.command.as_str() {
        "eval" => &["bw", "disease", "conc", "D", "U"],
        "pk" => &["bw", "disease", "drug", "dose", "route", "times"],
        "params" => &["bw", "disease"],
        "calibrate" => &["targets", "n_healthy", "n_disease", "seed0", "n_hrv"],
        "signals" => &["bw", "disease", "conc", "D", "U", "seed", "duration"],
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
        ("targets", a.targets.is_some()),
        ("n_healthy", a.n_healthy.is_some()),
        ("n_disease", a.n_disease.is_some()),
        ("seed0", a.seed0.is_some()),
        ("n_hrv", a.n_hrv.is_some()),
        ("seed", a.seed.is_some()),
        ("duration", a.duration.is_some()),
        ("bw", a.bw.is_some()),
        ("disease", !a.diseases.is_empty()),
    ];
    for (name, set) in used {
        if set && !allowed.contains(&name) {
            return Err(Failure::Usage(format!(
                "--{} n'est pas une option de « {} »",
                name.replace('_', "-"),
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
        "eval" | "signals" => {
            let d = a.d.unwrap_or(0.0);
            let u = a.u.unwrap_or(0.0);
            let params = if a.conc.is_empty() && d == 0.0 && u == 0.0 {
                pt.params.clone()
            } else {
                for (id, _) in &a.conc {
                    check_drug(&cat, id)?;
                }
                vetsim_core::apply_drug_effects(&pt.params, &cat.drugs, &a.conc, d, u)
                    .map_err(|e| Failure::Engine(e.to_string()))?
            };
            let ev = vetsim_core::evaluate(&params).map_err(|e| Failure::Engine(e.to_string()))?;
            if a.command == "signals" {
                return Ok(signals_json(
                    &ev,
                    a.seed.unwrap_or(0),
                    a.duration.unwrap_or(8.0),
                ));
            }
            let mut m = Map::new();
            for (k, v) in ev.to_flat() {
                m.insert(k.to_string(), Json::from(v));
            }
            Ok(Json::Object(m))
        }
        "pk" => {
            let drug = a.drug.clone().unwrap_or_default();
            check_drug(&cat, &drug)?;
            let times = a.times.clone().unwrap_or_default();
            let route = match a.route.as_deref() {
                Some("po") => vetsim_core::Route::Po,
                _ => vetsim_core::Route::Iv,
            };
            let dose = vetsim_core::Dose {
                t_h: 0.0,
                drug: drug.clone(),
                mg_per_kg: a.dose.unwrap_or_default(),
                route,
            };
            let t_end = times.iter().copied().fold(0.0, f64::max);
            let tc = vetsim_core::timecourse(&pt.params, &cat.drugs, &[dose], t_end, &times)
                .map_err(|e| Failure::Engine(e.to_string()))?;
            let conc = tc.conc_of(&drug).unwrap_or_default();
            // Comme la référence : pour chaque instant demandé, l'enregistrement le plus proche.
            let mut ts = Vec::with_capacity(times.len());
            let mut cs = Vec::with_capacity(times.len());
            for t in &times {
                let i = (0..tc.t.len())
                    .min_by(|&x, &y| {
                        (tc.t[x] - t)
                            .abs()
                            .partial_cmp(&(tc.t[y] - t).abs())
                            .expect("fini")
                    })
                    .ok_or_else(|| Failure::Engine("pk : aucun instant enregistré".into()))?;
                ts.push(Json::from(tc.t[i]));
                cs.push(Json::from(conc[i]));
            }
            let mut m = Map::new();
            m.insert("t".into(), Json::Array(ts));
            m.insert("conc_mg_L".into(), Json::Array(cs));
            Ok(Json::Object(m))
        }
        "calibrate" => calibrate(&cat, &a),
        _ => unreachable!("commande validée par parse_args"),
    }
}

/// Signaux d'un patient (diagnostic hors contrat) : ECG, PCG, instants R, VFC, caractéristiques.
fn signals_json(ev: &vetsim_core::Evaluation, seed: u64, duration_s: f64) -> Json {
    use vetsim_core::signals::{patient_signals, SignalOptions};
    let s = patient_signals(
        ev,
        &SignalOptions {
            duration_s,
            ..SignalOptions::default()
        },
        seed,
    );
    let (e, p) = (&s.ecg, &s.pcg);
    let f = e.features;
    let pf = p.features;
    let hrv = s.autonomic.hrv.map(|h| {
        serde_json::json!({
            "fc_moy": h.fc_moy, "sdnn_ms": h.sdnn_ms, "rmssd_ms": h.rmssd_ms, "lf": h.lf, "hf": h.hf,
            "lf_hf": if h.lf_hf.is_finite() { Json::from(h.lf_hf) } else { Json::Null },
            "rsa_bpm": h.rsa_bpm, "f_peak": h.f_peak,
        })
    });
    serde_json::json!({
        "ecg": { "fs": 500.0, "v": e.v, "r_times": e.r_times,
                 "features": { "pr": f.pr, "qrs": f.qrs, "qt": f.qt, "p_amp": f.p_amp,
                               "t_amp": f.t_amp, "r_amp": f.r_amp, "t_w": f.t_w } },
        "pcg": { "fs": vetsim_core::signals::PCG_FS, "v": p.v,
                 "features": { "s1": pf.s1, "s2": pf.s2, "s3": pf.s3, "s3_amp": pf.s3_amp,
                               "murmur": pf.murmur.label_fr(), "murmur_peak_flow": pf.murmur_peak_flow,
                               "grade": pf.grade } },
        "hrv": hrv,
    })
}

/// Banc de calibration (diagnostic hors contrat) : même structure que
/// `calibration/calibration_run_python.json`.
fn calibrate(cat: &Catalog, a: &Args) -> Result<Json, Failure> {
    use vetsim_core::calibrate as cal;
    let engine = |e: vetsim_core::Error| Failure::Engine(e.to_string());
    let path = a
        .targets
        .clone()
        .unwrap_or_else(|| PathBuf::from("calibration/targets.json"));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Failure::Modules(format!("{} : lecture impossible ({e})", path.display())))?;
    let targets = cal::parse_targets(&text, &path.display().to_string())
        .map_err(|e| Failure::Modules(e.to_string()))?;
    let mut rows = cal::healthy_cohort(cat, &targets, a.n_healthy.unwrap_or(50)).map_err(engine)?;
    let hrv_targets = cal::parse_hrv_targets(&text, &path.display().to_string())
        .map_err(|e| Failure::Modules(e.to_string()))?;
    rows.extend(cal::hrv_cohort(cat, &hrv_targets, a.n_hrv.unwrap_or(20)).map_err(engine)?);
    let healthy: Vec<Json> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "module": r.target.module, "var": r.target.var, "target": r.target.target,
                "tol": r.target.tol, "unit": r.target.unit, "source": r.target.source,
                "model_mean": r.model_mean, "model_sd": r.model_sd, "ok": r.ok,
                "frac_in": r.frac_in, "n": r.n,
            })
        })
        .collect();
    let pk: Vec<Json> = cal::pk_checks(cat)
        .map_err(engine)?
        .iter()
        .map(|r| {
            serde_json::json!({
                "module": r.module, "var": r.var, "target": r.target, "unit": r.unit,
                "source": r.source, "model_mean": r.model,
            })
        })
        .collect();
    let mut disease = Map::new();
    for g in cal::disease_checks(cat, a.n_disease.unwrap_or(24), a.seed0.unwrap_or(100))
        .map_err(engine)?
    {
        let samples: Vec<Json> = g
            .samples
            .iter()
            .map(|v| {
                let mut m = Map::new();
                for (k, x) in cal::DISEASE_KEYS.iter().zip(v) {
                    m.insert((*k).into(), Json::from(*x));
                }
                Json::Object(m)
            })
            .collect();
        disease.insert(format!("{}|{}", g.disease, g.stage), Json::Array(samples));
    }
    Ok(serde_json::json!({ "healthy": healthy, "pk": pk, "disease": disease }))
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
