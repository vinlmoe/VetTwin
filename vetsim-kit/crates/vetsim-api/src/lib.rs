//! API par messages JSON (docs/KMP.md), en Rust pur : partagée par la liaison UniFFI
//! (`vetsim-ffi`, Android/JVM) et la liaison WebAssembly (`vetsim-wasm`, navigateur).
//!
//! Chaque méthode reçoit et renvoie une chaîne JSON. Une entrée invalide renvoie
//! `{"ok": false, "errors": [{"path": ..., "message": ...}]}` ; aucune méthode ne panique
//! volontairement (la couche FFI ajoute un `catch_unwind` en dernier recours).
//!
//! Usage pédagogique uniquement : aucun résultat ne sert à décider d'un traitement réel.

use serde_json::{json, Map, Value as Json};
use std::collections::BTreeMap;
use vetsim_core::blocks::{blocks_of, Block};
use vetsim_core::case::{Case, DrawSource, NoNoise};
use vetsim_core::dsp::{base64, wav_pcm16};
use vetsim_core::pcg::{patient_pcg, AuscultationConfig, PcgOptions};
use vetsim_core::pkpd::{apply_drug_effects, emax_effect, timecourse, Dose, Route};
use vetsim_core::rng::NoiseSource;
use vetsim_core::signals::{make_pcg, patient_signals, streams, PcgInputs, SignalOptions, PCG_FS};
use vetsim_core::{
    evaluate, Catalog, DiseaseModule, DrugModule, Evaluation, LoadError, Params, Patient,
    PatientModule, SimRng,
};

/// Version de l'API par messages.
pub const API_VERSION: &str = vetsim_core::API_VERSION;

/// Erreur d'API : liste de (chemin, message).
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError(pub Vec<(String, String)>);

impl ApiError {
    fn one(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self(vec![(path.into(), message.into())])
    }

    /// Réponse JSON `{"ok": false, "errors": [...]}`.
    pub fn to_json(&self) -> String {
        let errors: Vec<Json> = self
            .0
            .iter()
            .map(|(p, m)| json!({ "path": p, "message": m }))
            .collect();
        json!({ "ok": false, "errors": errors }).to_string()
    }
}

impl From<vetsim_core::Error> for ApiError {
    fn from(e: vetsim_core::Error) -> Self {
        ApiError::one("", e.to_string())
    }
}

type ApiResult = Result<Json, ApiError>;

fn parse(text: &str, what: &str) -> Result<Json, ApiError> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(text)
        .map_err(|e| ApiError::one("", format!("{what} : JSON illisible ({e})")))
}

fn obj<'a>(v: &'a Json, path: &str) -> Result<&'a Map<String, Json>, ApiError> {
    v.as_object()
        .ok_or_else(|| ApiError::one(path, "objet JSON attendu"))
}

fn opt_f64(m: &Map<String, Json>, key: &str, default: f64) -> Result<f64, ApiError> {
    match m.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .ok_or_else(|| ApiError::one(format!("/{key}"), "nombre attendu")),
    }
}

fn opt_u64(m: &Map<String, Json>, key: &str, default: u64) -> Result<u64, ApiError> {
    match m.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .ok_or_else(|| ApiError::one(format!("/{key}"), "entier ≥ 0 attendu")),
    }
}

fn opt_bool(m: &Map<String, Json>, key: &str, default: bool) -> Result<bool, ApiError> {
    match m.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| ApiError::one(format!("/{key}"), "booléen attendu")),
    }
}

/// Patient d'une session.
#[derive(Debug, Clone)]
struct Session {
    patient: Patient,
    rng: SimRng,
    seed: u64,
    variability: bool,
}

/// Moteur : catalogue de modules et patients (pas d'état global).
#[derive(Debug, Default)]
pub struct Engine {
    catalog: Option<Catalog>,
    auscultation: Option<AuscultationConfig>,
    patients: BTreeMap<String, Session>,
    next_id: u64,
}

fn load_issues(prefix: &str, e: LoadError) -> Vec<(String, String)> {
    match e {
        LoadError::Invalid { issues, .. } => issues
            .into_iter()
            .map(|i| (format!("{prefix}{}", i.path), i.message))
            .collect(),
        other => vec![(prefix.to_string(), other.to_string())],
    }
}

impl Engine {
    /// Moteur vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Version de l'API.
    pub fn api_version(&self) -> String {
        API_VERSION.to_string()
    }

    /// Charge les modules : `{"patient": {...}, "diseases": [{...}], "drugs": [{...}]}`
    /// (contenus de `healthy_dog.json`, `diseases/*.json`, `drugs/*.json`). Remplace le
    /// catalogue courant et efface les patients.
    pub fn load_modules(&mut self, text: &str) -> String {
        self.respond(|e| e.load_modules_impl(text))
    }

    fn load_modules_impl(&mut self, text: &str) -> ApiResult {
        let root = parse(text, "modules")?;
        let m = obj(&root, "")?;
        let mut errors = Vec::new();
        let patient = match m.get("patient") {
            None => {
                errors.push(("/patient".to_string(), "module patient requis".to_string()));
                None
            }
            Some(p) => match PatientModule::from_json(&p.to_string(), "patient") {
                Ok(p) => Some(p),
                Err(e) => {
                    errors.extend(load_issues("/patient", e));
                    None
                }
            },
        };
        let list = |key: &str, errors: &mut Vec<(String, String)>| -> Vec<(String, String)> {
            match m.get(key) {
                None => Vec::new(),
                Some(Json::Array(a)) => a
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (format!("/{key}/{i}"), v.to_string()))
                    .collect(),
                Some(_) => {
                    errors.push((format!("/{key}"), "tableau attendu".to_string()));
                    Vec::new()
                }
            }
        };
        let mut diseases = Vec::new();
        for (path, src) in list("diseases", &mut errors) {
            match DiseaseModule::from_json(&src, &path) {
                Ok(d) => diseases.push(d),
                Err(e) => errors.extend(load_issues(&path, e)),
            }
        }
        let mut drugs = Vec::new();
        for (path, src) in list("drugs", &mut errors) {
            match DrugModule::from_json(&src, &path) {
                Ok(d) => drugs.push(d),
                Err(e) => errors.extend(load_issues(&path, e)),
            }
        }
        // Configuration du module auscultation (optionnelle).
        let auscultation = match m.get("auscultation") {
            None | Some(Json::Null) => None,
            Some(a) => match AuscultationConfig::from_json(&a.to_string(), "auscultation") {
                Ok(c) => Some(c),
                Err(e) => {
                    errors.extend(load_issues("/auscultation", e));
                    None
                }
            },
        };
        if !errors.is_empty() {
            return Err(ApiError(errors));
        }
        self.auscultation = auscultation;
        let catalog = Catalog::new(patient.expect("contrôlé"), diseases, drugs)
            .map_err(|e| ApiError::one("", e.to_string()))?;
        let counts = json!({ "diseases": catalog.diseases.len(), "drugs": catalog.drugs.len(),
                             "auscultation": self.auscultation.is_some() });
        self.catalog = Some(catalog);
        self.patients.clear();
        Ok(json!({ "ok": true, "loaded": counts }))
    }

    fn catalog(&self) -> Result<&Catalog, ApiError> {
        self.catalog
            .as_ref()
            .ok_or_else(|| ApiError::one("", "aucun module chargé (appeler loadModules)"))
    }

    /// Catalogue : identifiants, noms, stades, niveaux de preuve.
    pub fn list_modules(&mut self) -> String {
        self.respond(|e| {
            let c = e.catalog()?;
            let diseases: Vec<Json> = c
                .diseases
                .iter()
                .map(|d| {
                    let stages: Vec<Json> = d
                        .stages
                        .iter()
                        .map(|s| json!({ "name": s.name, "lo": s.lo, "hi": s.hi, "prior": s.prior }))
                        .collect();
                    json!({ "id": d.id, "name": d.name.get("fr"), "evidence": d.evidence.as_str(),
                            "stages": stages, "refs": d.refs, "effects": effects_json(&d.effects) })
                })
                .collect();
            let drugs: Vec<Json> = c
                .drugs
                .iter()
                .map(|d| {
                    let kind = match d.pd.kind {
                        vetsim_core::modules::PdKind::Emax => "emax",
                        vetsim_core::modules::PdKind::Diuretic { .. } => "diuretic",
                    };
                    json!({ "id": d.id, "name": d.name.get("fr"), "evidence": d.evidence.as_str(),
                            "pd_type": kind, "ec50_mg_L": d.pd.ec50_mg_l, "refs": d.refs,
                            "effects": effects_json(&d.pd.effects) })
                })
                .collect();
            let blocks: Vec<Json> = Block::ALL
                .iter()
                .map(|b| {
                    let params: Vec<&str> = Params::NAMES
                        .iter()
                        .copied()
                        .filter(|n| blocks_of(n).contains(b))
                        .collect();
                    json!({ "id": b.id(), "label_fr": b.label_fr(), "params": params })
                })
                .collect();
            Ok(json!({ "ok": true, "patient": { "id": c.patient.id, "version": c.patient.version },
                       "diseases": diseases, "drugs": drugs, "blocks": blocks,
                       "warning": "Usage pédagogique uniquement : ne jamais utiliser pour décider d'une dose ou d'un traitement réel." }))
        })
    }

    /// Nouveau patient : `{"bw": 20, "seed": 42, "variability": true}` → `{"patient_id": "p1"}`.
    pub fn new_patient(&mut self, options: &str) -> String {
        self.respond(|e| {
            let o = parse(options, "options")?;
            let m = obj(&o, "")?;
            let bw = opt_f64(m, "bw", 20.0)?;
            let seed = opt_u64(m, "seed", 0)?;
            let variability = opt_bool(m, "variability", true)?;
            let c = e.catalog()?;
            let mut rng = SimRng::for_stream(seed, streams::PATIENT);
            let patient = if variability {
                Patient::with_variability(&c.patient, bw, &mut rng)?
            } else {
                Patient::healthy(&c.patient, bw)?
            };
            e.next_id += 1;
            let id = format!("p{}", e.next_id);
            e.patients.insert(
                id.clone(),
                Session {
                    patient,
                    rng,
                    seed,
                    variability,
                },
            );
            Ok(json!({ "ok": true, "patient_id": id }))
        })
    }

    fn session(&mut self, id: &str) -> Result<&mut Session, ApiError> {
        self.patients
            .get_mut(id)
            .ok_or_else(|| ApiError::one("/patient_id", format!("patient inconnu « {id} »")))
    }

    /// Ajoute une maladie : `{"id": "mmvd", "stage": "C"}` ou `{"id": "mmvd", "severity": 0.7}`
    /// (sans stade ni sévérité : stade tiré selon le prior). Patient sans variabilité : tirages
    /// sans bruit (mode du CLI).
    pub fn add_disease(&mut self, patient_id: &str, options: &str) -> String {
        self.respond(|e| {
            let o = parse(options, "options")?;
            let m = obj(&o, "")?;
            let id = m
                .get("id")
                .and_then(Json::as_str)
                .ok_or_else(|| ApiError::one("/id", "identifiant de maladie requis"))?
                .to_string();
            let stage = match m.get("stage") {
                None | Some(Json::Null) => None,
                Some(v) => Some(
                    v.as_str()
                        .ok_or_else(|| ApiError::one("/stage", "chaîne attendue"))?
                        .to_string(),
                ),
            };
            let severity = match m.get("severity") {
                None | Some(Json::Null) => None,
                Some(_) => Some(opt_f64(m, "severity", 0.0)?),
            };
            if stage.is_some() && severity.is_some() {
                return Err(ApiError::one(
                    "",
                    "donner « stage » ou « severity », pas les deux",
                ));
            }
            let module = e
                .catalog()?
                .disease(&id)
                .cloned()
                .ok_or_else(|| ApiError::one("/id", format!("maladie inconnue « {id} »")))?;
            let s = e.session(patient_id)?;
            let mut no_noise = NoNoise;
            let draws: &mut dyn DrawSource = if s.variability {
                &mut s.rng
            } else {
                &mut no_noise
            };
            let case = match severity {
                Some(sev) => s.patient.add_disease(&module, sev, draws)?,
                None => s
                    .patient
                    .add_disease_staged(&module, stage.as_deref(), draws)?,
            };
            Ok(json!({ "ok": true, "case": case_json(case) }))
        })
    }

    fn drugged_params(&self, s: &Session, m: &Map<String, Json>) -> Result<Params, ApiError> {
        let conc: Vec<(String, f64)> = match m.get("conc") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Object(c)) => c
                .iter()
                .map(|(k, v)| {
                    v.as_f64()
                        .filter(|x| x.is_finite())
                        .map(|x| (k.clone(), x))
                        .ok_or_else(|| ApiError::one(format!("/conc/{k}"), "nombre attendu (mg/L)"))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(ApiError::one("/conc", "objet {médicament: mg/L} attendu")),
        };
        let mut d = opt_f64(m, "D", 0.0)?;
        let mut u = opt_f64(m, "U", 0.0)?;
        let mut conc = conc;
        // Examen à l'instant `at_h` d'un schéma d'administration : concentrations, D et U
        // simulés (PK du patient, donc avec sa fonction rénale et hépatique).
        if m.contains_key("doses") {
            if !conc.is_empty() || d != 0.0 || u != 0.0 {
                return Err(ApiError::one(
                    "/doses",
                    "donner « doses » + « at_h » OU « conc »/« D »/« U », pas les deux",
                ));
            }
            let at = opt_f64(m, "at_h", f64::NAN)?;
            if !(0.0..=24.0 * 60.0).contains(&at) {
                return Err(ApiError::one(
                    "/at_h",
                    "instant d'examen (h) requis, entre 0 et 60 jours",
                ));
            }
            let ds = parse_doses(m)?;
            let tc = timecourse(&s.patient.params, &self.catalog()?.drugs, &ds, at, &[at])?;
            conc = tc.conc.iter().map(|(k, v)| (k.clone(), v[0])).collect();
            d = tc.d[0];
            u = tc.u[0];
        }
        let mut base = s.patient.params.clone();
        // Préréglage de FC (réglage d'usage, ex. « repos » 72/min) : FC de repos imposée.
        if let Some(v) = m.get("hr").filter(|v| !v.is_null()) {
            let hr = v
                .as_f64()
                .filter(|h| *h > 20.0 && *h < 300.0)
                .ok_or_else(|| ApiError::one("/hr", "FC entre 20 et 300 /min attendue"))?;
            base.hr = hr;
        }
        if conc.is_empty() && d == 0.0 && u == 0.0 {
            return Ok(base);
        }
        Ok(apply_drug_effects(
            &base,
            &self.catalog()?.drugs,
            &conc,
            d,
            u,
        )?)
    }

    /// État : mêmes clés plates que le CLI `eval`, plus `case` et `emergent_signs` ; options
    /// `{"conc": {...}, "D": 0, "U": 0, "signals": false}` (avec `signals`, ajoute la réponse
    /// de [`Engine::signals`] sous la clé `signals`).
    pub fn state(&mut self, patient_id: &str, options: &str) -> String {
        self.respond(|e| {
            let o = parse(options, "options")?;
            let m = obj(&o, "")?.clone();
            let with_signals = opt_bool(&m, "signals", false)?;
            let s = e.session(patient_id)?.clone();
            let p = e.drugged_params(&s, &m)?;
            let (ev, outcome) = assess(&p)?;
            let mut out = Map::new();
            out.insert("ok".into(), Json::Bool(true));
            if let Some(ev) = &ev {
                for (k, v) in ev.to_flat() {
                    out.insert(k.into(), Json::from(v));
                }
            }
            out.insert(
                "case".into(),
                Json::Array(s.patient.cases.iter().map(case_json).collect()),
            );
            let alive = outcome["state"] != "dead";
            // Examen à l'instant `at_h` d'un schéma : créatinine et urée cinétiques (grille de
            // 12 pas sur [0, at_h]) au lieu des valeurs d'équilibre.
            if alive && m.contains_key("doses") {
                let at = opt_f64(&m, "at_h", 0.0)?;
                if at > 0.0 {
                    let mut base = s.patient.params.clone();
                    if let Some(hr) = m.get("hr").and_then(Json::as_f64) {
                        base.hr = hr;
                    }
                    let grid: Vec<f64> = (0..=12).map(|i| at * i as f64 / 12.0).collect();
                    let c = e.catalog()?;
                    let ds = parse_doses(&m)?;
                    let tcs = timecourse(&base, &c.drugs, &ds, at, &grid)?;
                    let mut states = Vec::with_capacity(tcs.t.len());
                    for i in 0..tcs.t.len() {
                        let conc_i: Vec<(String, f64)> =
                            tcs.conc.iter().map(|(d, v)| (d.clone(), v[i])).collect();
                        let pi = apply_drug_effects(&base, &c.drugs, &conc_i, tcs.d[i], tcs.u[i])?;
                        states.push(state_entry(&pi)?);
                    }
                    apply_marker_kinetics(&mut states, &tcs.t, &base)?;
                    if let Some(last) = states.last() {
                        for key in ["creat", "urea"] {
                            if let Some(v) = last[key].as_f64() {
                                out.insert(key.into(), Json::from(v));
                            }
                        }
                    }
                }
            }
            out.insert("outcome".into(), outcome);
            let signs = match &ev {
                Some(ev) if alive => emergent_signs(ev),
                _ => Vec::new(),
            };
            out.insert("emergent_signs".into(), Json::Array(signs));
            // Paramètres d'entrée du modèle (après maladies et médicaments, avant baroréflexe).
            let params: Map<String, Json> = p
                .to_vec()
                .into_iter()
                .map(|(k, v)| (k.to_string(), Json::from(v)))
                .collect();
            out.insert("params".into(), Json::Object(params));
            if with_signals {
                // Pas de signal chez un patient décédé (aucun battement).
                let sig = match &ev {
                    Some(ev) if alive => signals_json(ev, &m, s.seed)?,
                    _ => Json::Null,
                };
                out.insert("signals".into(), sig);
            }
            Ok(Json::Object(out))
        })
    }

    /// Signaux : `{"duration_s": 8, "fs": 500, "seed": ...}` (+ `conc`, `D`, `U`) →
    /// `{"ecg": [...], "pcg": [...], "r_times": [...], ...}`. Reproductible pour un patient.
    pub fn signals(&mut self, patient_id: &str, options: &str) -> String {
        self.respond(|e| {
            let o = parse(options, "options")?;
            let m = obj(&o, "")?.clone();
            let s = e.session(patient_id)?.clone();
            let p = e.drugged_params(&s, &m)?;
            let ev = alive(&p)?;
            let mut out = signals_json(&ev, &m, s.seed)?;
            out["ok"] = Json::Bool(true);
            Ok(out)
        })
    }

    /// Évolution temporelle : `{"doses": [{"t_h": 0, "drug": "furosemide", "mg_per_kg": 2,
    /// "route": "iv"}], "t_end_h": 12, "t_eval_h": [0, 1, 2], "states": true}` →
    /// concentrations, `D`, `U` et (si `states`) l'état plat à chaque instant.
    pub fn timecourse(&mut self, patient_id: &str, doses: &str) -> String {
        self.respond(|e| {
            let o = parse(doses, "doses")?;
            let m = obj(&o, "")?;
            let ds = parse_doses(m)?;
            let t_end = opt_f64(m, "t_end_h", 24.0)?;
            let t_eval: Vec<f64> = match m.get("t_eval_h") {
                None => (0..=t_end.ceil() as usize)
                    .map(|h| h as f64)
                    .filter(|h| *h <= t_end)
                    .collect(),
                Some(Json::Array(a)) => a
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        v.as_f64().ok_or_else(|| {
                            ApiError::one(format!("/t_eval_h/{i}"), "nombre attendu")
                        })
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err(ApiError::one("/t_eval_h", "tableau attendu")),
            };
            if t_eval.len() > 2000 {
                return Err(ApiError::one("/t_eval_h", "au plus 2000 instants"));
            }
            let with_states = opt_bool(m, "states", false)?;
            let s = e.session(patient_id)?.clone();
            let c = e.catalog()?;
            let tc = timecourse(&s.patient.params, &c.drugs, &ds, t_end, &t_eval)?;
            let mut conc = Map::new();
            for (d, v) in &tc.conc {
                conc.insert(d.clone(), json!(v));
            }
            // Effet Emax normalisé E(t) de chaque médicament (0–1 × emax).
            let mut effects = Map::new();
            for (d, v) in &tc.conc {
                let module = c.drug(d).expect("médicament validé par timecourse");
                effects.insert(
                    d.clone(),
                    json!(v
                        .iter()
                        .map(|x| emax_effect(module, *x))
                        .collect::<Vec<_>>()),
                );
            }
            let mut out = json!({ "ok": true, "t": tc.t, "conc_mg_L": conc, "effect": effects,
                                  "D": tc.d, "U": tc.u });
            // États physiologiques à des instants choisis (moins nombreux que la grille PK).
            if let Some(v) = m.get("state_times_h") {
                let times: Vec<f64> = v
                    .as_array()
                    .ok_or_else(|| ApiError::one("/state_times_h", "tableau attendu"))?
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        x.as_f64()
                            .filter(|t| *t >= 0.0 && *t <= t_end)
                            .ok_or_else(|| {
                                ApiError::one(
                                    format!("/state_times_h/{i}"),
                                    "instant entre 0 et t_end_h",
                                )
                            })
                    })
                    .collect::<Result<_, _>>()?;
                if times.len() > 400 {
                    return Err(ApiError::one("/state_times_h", "au plus 400 instants"));
                }
                let tcs = timecourse(&s.patient.params, &c.drugs, &ds, t_end, &times)?;
                let mut states = Vec::with_capacity(tcs.t.len());
                let mut death: Option<f64> = None;
                let mut cause = Json::Null;
                for i in 0..tcs.t.len() {
                    // décès absorbant : plus d'évaluation après le premier instant de décès
                    if death.is_some() {
                        states.push(dead_entry(&cause));
                        continue;
                    }
                    let conc_i: Vec<(String, f64)> =
                        tcs.conc.iter().map(|(d, v)| (d.clone(), v[i])).collect();
                    let p = apply_drug_effects(
                        &s.patient.params,
                        &c.drugs,
                        &conc_i,
                        tcs.d[i],
                        tcs.u[i],
                    )?;
                    let st = state_entry(&p)?;
                    if st["outcome"] == "dead" {
                        death = Some(tcs.t[i]);
                        cause = st["cause"].clone();
                    }
                    states.push(st);
                }
                out["state_t"] = json!(tcs.t);
                apply_marker_kinetics(&mut states, &tcs.t, &s.patient.params)?;
                out["states_at"] = Json::Array(states);
                out["death_t_h"] = json!(death);
            }
            if with_states {
                let mut states = Vec::with_capacity(tc.t.len());
                let mut cause: Option<Json> = None;
                for i in 0..tc.t.len() {
                    if let Some(c) = &cause {
                        states.push(dead_entry(c));
                        continue;
                    }
                    let conc_i: Vec<(String, f64)> =
                        tc.conc.iter().map(|(d, v)| (d.clone(), v[i])).collect();
                    let p =
                        apply_drug_effects(&s.patient.params, &c.drugs, &conc_i, tc.d[i], tc.u[i])?;
                    let st = state_entry(&p)?;
                    if st["outcome"] == "dead" {
                        cause = Some(st["cause"].clone());
                    }
                    states.push(st);
                }
                apply_marker_kinetics(&mut states, &tc.t, &s.patient.params)?;
                out["states"] = Json::Array(states);
            }
            Ok(out)
        })
    }

    /// Auscultation : `{"site": "base"|"apex", "hr": 72, "fs": 4000, "duration_s": 8, "seed": ...,
    /// "conc": {...}, "with_ecg": true, "with_pressures": true}` →
    /// `{"wav_base64", "fs", "grade", "murmur", "s3", ...}`. Avec la même graine et la même
    /// durée, l'ECG renvoyé (`with_ecg`) a les mêmes battements que le son.
    pub fn pcg(&mut self, patient_id: &str, options: &str) -> String {
        self.respond(|e| {
            let o = parse(options, "options")?;
            let m = obj(&o, "")?.clone();
            let cfg = e
                .auscultation
                .clone()
                .ok_or_else(|| ApiError::one("", "configuration auscultation non chargée (clé « auscultation » de loadModules)"))?;
            let s = e.session(patient_id)?.clone();
            let p = e.drugged_params(&s, &m)?;
            let ev = alive(&p)?;
            let seed = opt_u64(&m, "seed", s.seed)?;
            let site = match m.get("site") {
                None | Some(Json::Null) => None,
                Some(v) => Some(v.as_str().ok_or_else(|| ApiError::one("/site", "chaîne attendue"))?.to_string()),
            };
            let dur = opt_f64(&m, "duration_s", cfg.duration_s)?;
            if !(1.0..=60.0).contains(&dur) {
                return Err(ApiError::one("/duration_s", "durée entre 1 et 60 s"));
            }
            let fs = opt_f64(&m, "fs", cfg.fs)?;
            if !(1000.0..=48000.0).contains(&fs) {
                return Err(ApiError::one("/fs", "fréquence entre 1000 et 48000 Hz"));
            }
            let opts = PcgOptions { fs: Some(fs), duration_s: Some(dur), resp_hz: None, site };
            let r = patient_pcg(&ev, &cfg, &opts, seed)?;
            let wav = wav_pcm16(&r.samples, r.fs as u32);
            let f = r.features;
            let mut out = json!({
                "ok": true, "wav_base64": base64(&wav), "fs": r.fs, "grade": f.grade,
                "murmur": f.murmur.label_fr(), "s3": f.s3, "s1": f.s1, "s2": f.s2,
                "murmur_peak_flow": f.murmur_peak_flow, "beats": r.beats, "t_mc": r.t_mc, "t_ac": r.t_ac,
                "duration_s": dur, "hr": ev.params.hr, "rr_per_min": ev.metabolic.art.rr,
                "site": opts.site.clone().unwrap_or_else(|| "base".into()),
                "config_version": cfg.version,
            });
            if opt_bool(&m, "with_ecg", false)? {
                let sg = patient_signals(&ev, &SignalOptions { duration_s: dur, ecg_fs: 500.0, pcg_beats: 1 }, seed);
                out["ecg"] = json!(sg.ecg.v.iter().map(|x| *x as f32 as f64).collect::<Vec<_>>());
                out["ecg_fs"] = json!(500.0);
                out["r_times"] = json!(sg.ecg.r_times);
            }
            if opt_bool(&m, "with_pressures", false)? {
                // mêmes battements que le son et l'ECG (pressions simulées battement par battement)
                let sg = patient_signals(&ev, &SignalOptions { duration_s: dur, ecg_fs: 500.0, pcg_beats: 1 }, seed);
                out["pressures"] = beat_pressures(&ev, &sg.autonomic.beats, seed, dur, 250.0)?;
            }
            Ok(out)
        })
    }

    /// Cœur animé : `{"duration_s": 8, "fs": 100, "seed": ..., "conc"|"doses"+"at_h", "hr"}`.
    ///
    /// Tout ce que l'animation montre vient du modèle, sur la même fenêtre et les mêmes
    /// battements que l'ECG et le son :
    /// - `series` (échantillonnées à `fs`) : volumes `v_lv`, `v_rv`, `v_la` (OG + veines
    ///   pulmonaires), `v_ra` (OD + veines systémiques) ; pressions `p_lv`, `p_rv`, `p_ao`,
    ///   `p_pa`, `p_la`, `p_ra` ; débits valvulaires `q_mi`, `q_ao`, `q_tr`, `q_pu` (mL/s, négatif
    ///   = régurgitation) ; activations mécaniques `act_v` (élastance active × amplitude du
    ///   battement) et `act_a` ;
    /// - `electrical` : activations atriales (`on`/`off` de l'onde P ou f), ventriculaires
    ///   (origine, QRS, T, QT, instant du His), influx au nœud AV et leur devenir (conduit,
    ///   réfractaire, conduction cachée, collision, bloc infranodal, ventricules réfractaires),
    ///   décharges sinusales bloquées à la sortie ;
    /// - `mechanics` : battements de la simulation (début `t`, systole `ts`, relaxation `tr`,
    ///   amplitude calcique `c`) et durées de l'activation atriale.
    pub fn heart(&mut self, patient_id: &str, options: &str) -> String {
        self.respond(|e| {
            use vetsim_core::signals::{
                ecg_windows, patient_conduction, shift_conduction, streams, SIGNAL_WARM_S,
            };
            let o = parse(options, "options")?;
            let m = obj(&o, "")?.clone();
            let s = e.session(patient_id)?.clone();
            let p = e.drugged_params(&s, &m)?;
            let ev = alive(&p)?;
            let seed = opt_u64(&m, "seed", s.seed)?;
            let dur = opt_f64(&m, "duration_s", 8.0)?;
            if !(1.0..=30.0).contains(&dur) {
                return Err(ApiError::one("/duration_s", "durée entre 1 et 30 s"));
            }
            let fs = opt_f64(&m, "fs", 100.0)?;
            if !(20.0..=500.0).contains(&fs) {
                return Err(ApiError::one("/fs", "fréquence entre 20 et 500 Hz"));
            }
            let sg = patient_signals(
                &ev,
                &SignalOptions { duration_s: dur, ecg_fs: 500.0, pcg_beats: 1 },
                seed,
            );
            let w = SIGNAL_WARM_S;
            let q = &ev.params;
            let cond =
                patient_conduction(&ev, &sg.autonomic.beats, seed.wrapping_add(streams::RHYTHM));
            let r = vetsim_core::hemo::simulate_hemo_beats(q, &cond, w + dur + 0.5, w, 60.0 / q.hr)?;
            let wf = &r.waveforms;
            let dt = wf.t[1] - wf.t[0];
            let at = |series: &[f64], t: f64| -> f64 {
                let x = ((t - wf.t[0]) / dt).clamp(0.0, (series.len() - 1) as f64);
                let i = x.floor() as usize;
                let j = (i + 1).min(series.len() - 1);
                series[i] + (series[j] - series[i]) * (x - i as f64)
            };
            let n = (dur * fs) as usize;
            let pick = |v: &[f64]| -> Vec<f64> {
                (0..n).map(|i| at(v, w + i as f64 / fs) as f32 as f64).collect()
            };
            // Séquence de l'ECG (décalée) et bornes de ses ondes.
            let sc = shift_conduction(&cond, w);
            let (aw, vw) = ecg_windows(q, &sc, q.hr);
            let inside = |t: f64| (-1.0..dur + 0.5).contains(&t);
            let atrial: Vec<Json> = sc
                .atrial
                .iter()
                .zip(&aw)
                .filter(|(a, _)| inside(a.t))
                .map(|(a, x)| json!({ "t": a.t, "organized": a.organized, "on": x.on, "off": x.off }))
                .collect();
            let ventricular: Vec<Json> = sc
                .ventricular
                .iter()
                .zip(&vw)
                .filter(|(b, _)| inside(b.t))
                .map(|(b, x)| {
                    // His : influx supraventriculaire ou foyer jonctionnel (au-dessus du His)
                    let t_his = (!b.wide).then_some(b.t - q.hv);
                    json!({ "t": b.t, "origin": b.origin.as_str(), "wide": b.wide,
                            "t_a": b.t_a, "pr": b.pr, "t_his": t_his,
                            "qrs_on": x.qrs_on, "qrs_off": x.qrs_off, "t_peak": x.t_peak,
                            "t_end": x.t_end, "qt": x.qt })
                })
                .collect();
            let impulses: Vec<Json> = sc
                .impulses
                .iter()
                .filter(|i| inside(i.t))
                .map(|i| json!({ "t": i.t, "fate": i.fate.as_str(), "t_his": i.t_his }))
                .collect();
            let (beats, _) = vetsim_core::hemo::beat_schedule(&cond, 60.0 / q.hr, q);
            let mech: Vec<Json> = beats
                .iter()
                .filter(|b| inside(b.t - w))
                .map(|b| json!({ "t": b.t - w, "ts": b.ts, "tr": b.tr, "c": b.c }))
                .collect();
            Ok(json!({
                "ok": true, "duration_s": dur, "fs": fs, "hr": q.hr,
                "series": {
                    "v_lv": pick(&wf.v_lv), "v_rv": pick(&wf.v_rv), "v_la": pick(&wf.v_pv), "v_ra": pick(&wf.v_sv),
                    "p_lv": pick(&wf.p_lv), "p_rv": pick(&wf.p_rv), "p_ao": pick(&wf.p_ao),
                    "p_pa": pick(&wf.p_pa), "p_la": pick(&wf.p_pv), "p_ra": pick(&wf.p_sv),
                    "q_mi": pick(&wf.q_mi), "q_ao": pick(&wf.q_ao), "q_tr": pick(&wf.q_tr), "q_pu": pick(&wf.q_pu),
                    "act_v": pick(&wf.act_v), "act_a": pick(&wf.act_a),
                },
                "ecg": sg.ecg.v.iter().map(|x| *x as f32 as f64).collect::<Vec<_>>(), "ecg_fs": 500.0,
                "electrical": {
                    "atrial": atrial, "ventricular": ventricular, "impulses": impulses,
                    "sa_exit_blocked": sc.sa_exit_blocked.iter().copied().filter(|&t| inside(t)).collect::<Vec<_>>(),
                },
                "mechanics": { "beats": mech, "ta_atr": q.ta_atr, "tra_atr": q.tra_atr },
                "intervals": { "pr": q.pr, "qrs": q.qrs, "hv": q.hv, "qt": q.qt },
                "note": "séquence de conduction, ondes de l'ECG, activations, volumes, pressions et débits du modèle ; géométrie schématique ; usage pédagogique uniquement",
            }))
        })
    }

    /// Message JSON générique `{"op": ..., "patient": ..., "options": {...}}` : `pcg`, `state`,
    /// `signals`, `timecourse`, `add_disease`, `new_patient`, `list_modules`, `api_version`.
    pub fn message(&mut self, text: &str) -> String {
        let msg = match parse(text, "message") {
            Ok(m) => m,
            Err(e) => return e.to_json(),
        };
        let op = msg.get("op").and_then(Json::as_str).unwrap_or("");
        let pid = msg
            .get("patient")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        let opts = msg
            .get("options")
            .map(|o| o.to_string())
            .unwrap_or_else(|| "{}".into());
        match op {
            "pcg" => self.pcg(&pid, &opts),
            "heart" => self.heart(&pid, &opts),
            "state" => self.state(&pid, &opts),
            "signals" => self.signals(&pid, &opts),
            "timecourse" => self.timecourse(&pid, &opts),
            "add_disease" => self.add_disease(&pid, &opts),
            "new_patient" => self.new_patient(&opts),
            "list_modules" => self.list_modules(),
            "api_version" => json!({ "ok": true, "api_version": API_VERSION }).to_string(),
            other => ApiError::one("/op", format!("opération inconnue « {other} »")).to_json(),
        }
    }

    fn respond(&mut self, f: impl FnOnce(&mut Self) -> ApiResult) -> String {
        match f(self) {
            Ok(v) => v.to_string(),
            Err(e) => e.to_json(),
        }
    }
}

fn effects_json(effects: &[vetsim_core::modules::Effect]) -> Vec<Json> {
    effects
        .iter()
        .map(|e| {
            let blocks: Vec<&str> = blocks_of(e.param).iter().map(|b| b.id()).collect();
            json!({ "param": e.param, "op": e.op.as_str(), "conditional": e.when.is_some(), "blocks": blocks })
        })
        .collect()
}

/// Pressions aorte, VG et OG sur la fenêtre du son : le battement simulé est posé à chaque
/// instant de battement (avant le premier, la fin d'un battement virtuel à b0 − T). Approximation
/// documentée : la forme du battement n'est pas recalculée battement par battement.
/// Pressions simulées battement par battement sur la séquence de conduction des signaux (mêmes
/// battements que l'ECG et le son, chauffe `SIGNAL_WARM_S`), échantillonnées à `fs` sur
/// `[0, dur)` : alignées sur l'ECG dans le temps, déficit de pouls visible en cas d'arythmie.
fn beat_pressures(
    ev: &Evaluation,
    sinus_times: &[f64],
    seed: u64,
    dur: f64,
    fs: f64,
) -> Result<Json, ApiError> {
    use vetsim_core::signals::{patient_conduction, streams, SIGNAL_WARM_S};
    let w = SIGNAL_WARM_S;
    let cond = patient_conduction(ev, sinus_times, seed.wrapping_add(streams::RHYTHM));
    let r = vetsim_core::hemo::simulate_hemo_beats(
        &ev.params,
        &cond,
        w + dur + 0.5,
        w,
        60.0 / ev.params.hr,
    )?;
    let wf = &r.waveforms;
    let n = (dur * fs) as usize;
    let at = |series: &[f64], t: f64| -> f64 {
        // séries échantillonnées à pas constant depuis w
        let dt = wf.t[1] - wf.t[0];
        let x = ((t - wf.t[0]) / dt).clamp(0.0, (series.len() - 1) as f64);
        let i = x.floor() as usize;
        let j = (i + 1).min(series.len() - 1);
        series[i] + (series[j] - series[i]) * (x - i as f64)
    };
    let pick = |v: &[f64]| -> Vec<f64> {
        (0..n)
            .map(|i| at(v, w + i as f64 / fs) as f32 as f64)
            .collect()
    };
    Ok(
        json!({ "fs": fs, "ao": pick(&wf.p_ao), "lv": pick(&wf.p_lv), "la": pick(&wf.p_pv),
               "note": "pressions simulées battement par battement sur la séquence de conduction de l'ECG" }),
    )
}

/// Schéma d'administration : `"doses": [{"t_h", "drug", "mg_per_kg", "route": "iv"|"po"}]`.
fn parse_doses(m: &Map<String, Json>) -> Result<Vec<Dose>, ApiError> {
    let list = m
        .get("doses")
        .and_then(Json::as_array)
        .ok_or_else(|| ApiError::one("/doses", "tableau de doses requis"))?;
    if list.len() > 500 {
        return Err(ApiError::one("/doses", "au plus 500 administrations"));
    }
    let mut ds = Vec::with_capacity(list.len());
    for (i, d) in list.iter().enumerate() {
        let dm = obj(d, &format!("/doses/{i}"))?;
        let path = |k: &str| format!("/doses/{i}/{k}");
        let route = match dm.get("route").and_then(Json::as_str) {
            Some("iv") => Route::Iv,
            Some("po") => Route::Po,
            _ => return Err(ApiError::one(path("route"), "« iv » ou « po » attendu")),
        };
        ds.push(Dose {
            t_h: opt_f64(dm, "t_h", 0.0)
                .map_err(|_| ApiError::one(path("t_h"), "nombre attendu"))?,
            drug: dm
                .get("drug")
                .and_then(Json::as_str)
                .ok_or_else(|| ApiError::one(path("drug"), "identifiant requis"))?
                .to_string(),
            mg_per_kg: dm
                .get("mg_per_kg")
                .and_then(Json::as_f64)
                .ok_or_else(|| ApiError::one(path("mg_per_kg"), "nombre requis"))?,
            route,
        });
    }
    Ok(ds)
}

fn case_json(c: &Case) -> Json {
    let flags: Map<String, Json> = c
        .flags
        .iter()
        .map(|(k, v)| (k.clone(), Json::Bool(*v)))
        .collect();
    let draws: Map<String, Json> = c
        .draws
        .iter()
        .map(|(k, v)| (k.clone(), Json::from(*v)))
        .collect();
    let modifiers: Vec<Json> = c
        .modifiers
        .iter()
        .map(|m| {
            let blocks: Vec<&str> = blocks_of(m.param).iter().map(|b| b.id()).collect();
            json!({ "param": m.param, "op": m.op.as_str(), "value": m.value, "blocks": blocks })
        })
        .collect();
    json!({ "disease": c.disease, "severity": c.severity, "stage": c.stage, "flags": flags,
            "draws": draws, "modifiers": modifiers })
}

/// Source de bruit nulle (caractéristiques du PCG sans tirage).
struct Silence;
impl NoiseSource for Silence {
    fn uniform01(&mut self) -> f64 {
        0.0
    }
    fn standard_normal(&mut self) -> f64 {
        0.0
    }
}

/// Signes cliniques dérivés des variables, **uniquement** avec des seuils déjà présents dans
/// les fichiers du projet (chaque signe porte sa source). Liste volontairement courte.
/// Libellé d'un mécanisme de décompensation (`engine::Mechanism::id`).
fn mechanism_label_fr(id: &str) -> &'static str {
    match id {
        "acidemia_depression" => "dépression myocardique et vasoplégie par acidémie",
        "hypoxic_depression" => "dépression myocardique par hypoxie tissulaire",
        "coronary_ischemia" => "ischémie myocardique (pression de perfusion coronaire basse)",
        "hypokalemic_weakness" => "faiblesse des muscles respiratoires (hypokaliémie)",
        "atrial_standstill" => {
            "arrêt atrial hyperkaliémique (pas d'onde P ni de contraction atriale)"
        }
        "conduction_block" => {
            "trouble de conduction (FC ventriculaire inférieure à la FC sinusale)"
        }
        "anaerobic_metabolism" => "métabolisme anaérobie (extraction d'O₂ élevée, lactate)",
        "pulmonary_edema" => "œdème pulmonaire (POG élevée)",
        "renal_hypoperfusion" => "hypoperfusion rénale (PAM basse)",
        "baroreflex_exhausted" => "baroréflexe épuisé (tonus sympathique maximal)",
        _ => "mécanisme",
    }
}

/// Libellé d'une cause de décès (`Error::NonViable::cause`).
fn death_label_fr(cause: &str) -> &'static str {
    match cause {
        "cardiac_arrest" => "arrêt cardiaque (FC sous le seuil de pompe)",
        "circulatory_collapse" => "effondrement circulatoire (débit cardiaque effondré)",
        "irreversible_shock" => "choc irréversible (PAM sous le seuil de perfusion)",
        "oxygen_debt" => "dette d'oxygène (consommation d'O₂ supérieure à l'apport)",
        "no_steady_state" => "aucun état stationnaire viable",
        "vicious_circle" => "spirale de décompensation non résolue",
        _ => "décès",
    }
}

/// Évaluation avec issue mécanistique : `(dernière évaluation viable, issue JSON)`.
/// Issue : `{"state": "stable"|"decompensated"|"dead", "mechanisms": [...], "cause", "label_fr"}`.
/// Les seuils et pentes des couplages sont des placeholders (evidence D).
fn assess(p: &vetsim_core::Params) -> Result<(Option<Evaluation>, Json), ApiError> {
    match evaluate(p) {
        Ok(ev) => {
            let mechanisms: Vec<Json> = ev
                .outcome
                .mechanisms
                .iter()
                .map(|m| {
                    json!({ "id": m.id, "label_fr": mechanism_label_fr(m.id), "value": m.value,
                            "threshold": m.threshold, "factor": m.factor, "evidence": "D" })
                })
                .collect();
            let state = if ev.outcome.decompensated {
                "decompensated"
            } else {
                "stable"
            };
            let outcome = json!({ "state": state, "mechanisms": mechanisms,
                "f_contract": ev.decomp.f_contract, "f_rsys": ev.decomp.f_rsys,
                "f_vent": ev.decomp.f_vent, "iterations": ev.decomp.iterations });
            Ok((Some(ev), outcome))
        }
        Err(vetsim_core::Error::NonViable { cause, last }) => Ok((
            last.map(|b| *b),
            json!({ "state": "dead", "cause": cause, "label_fr": death_label_fr(cause),
                    "mechanisms": [], "evidence": "D" }),
        )),
        Err(e) => Err(e.into()),
    }
}

/// Évaluation d'un patient vivant ; erreur lisible s'il est décédé (pas de son, pas de signal).
fn alive(p: &vetsim_core::Params) -> Result<Evaluation, ApiError> {
    match evaluate(p) {
        Ok(ev) => Ok(ev),
        Err(vetsim_core::Error::NonViable { cause, .. }) => Err(ApiError::one(
            "",
            format!("patient décédé : {}", death_label_fr(cause)),
        )),
        Err(e) => Err(e.into()),
    }
}

/// Entrée d'une série d'états : valeurs plates (absentes en cas de décès), `outcome` (état),
/// `mechanisms` (identifiants) et `cause` si décès.
/// Créatinine et urée cinétiques le long d'une série d'états (`kinetics::lagged_marker`) :
/// départ à l'équilibre du patient sans médicament, production constante, élimination par le
/// DFG de chaque état. Remplace les valeurs d'équilibre `creat` et `urea` des états (l'état
/// stationnaire, CLI `eval` et golden, n'est pas concerné). Patient non viable sans
/// médicament : séries laissées telles quelles.
fn apply_marker_kinetics(
    states: &mut [Json],
    t: &[f64],
    base: &vetsim_core::Params,
) -> Result<(), ApiError> {
    let ev0 = match evaluate(base) {
        Ok(ev) => ev,
        Err(vetsim_core::Error::NonViable { .. }) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let gfr: Vec<Option<f64>> = states.iter().map(|s| s["gfr"].as_f64()).collect();
    for (key, c0, vd) in [
        ("creat", ev0.metabolic.chem.creat, base.creat_vd),
        ("urea", ev0.metabolic.chem.urea, base.urea_vd),
    ] {
        let css: Vec<Option<f64>> = states.iter().map(|s| s[key].as_f64()).collect();
        let lagged = vetsim_core::kinetics::lagged_marker(t, c0, &gfr, &css, vd * base.bw);
        for (st, c) in states.iter_mut().zip(lagged) {
            if let Some(c) = c {
                st[key] = Json::from(c);
            }
        }
    }
    Ok(())
}

fn state_entry(p: &vetsim_core::Params) -> Result<Json, ApiError> {
    let (ev, outcome) = assess(p)?;
    if outcome["state"] == "dead" {
        return Ok(dead_entry(&outcome["cause"]));
    }
    let mut st = Map::new();
    if let Some(ev) = &ev {
        for (k, v) in ev.to_flat() {
            st.insert(k.into(), Json::from(v));
        }
    }
    st.insert("outcome".into(), outcome["state"].clone());
    let ids: Vec<Json> = outcome["mechanisms"]
        .as_array()
        .map(|a| a.iter().map(|m| m["id"].clone()).collect())
        .unwrap_or_default();
    st.insert("mechanisms".into(), Json::Array(ids));
    if outcome["state"] == "dead" {
        st.insert("cause".into(), outcome["cause"].clone());
    }
    Ok(Json::Object(st))
}

/// Entrée d'une série d'états après le décès (absorbant) : pas de valeurs physiologiques.
fn dead_entry(cause: &Json) -> Json {
    json!({ "outcome": "dead", "mechanisms": [], "cause": cause })
}

fn emergent_signs(ev: &Evaluation) -> Vec<Json> {
    let mut out = Vec::new();
    let (h, m) = (&ev.hemo.summary, &ev.metabolic);
    let pcg = make_pcg(&PcgInputs::from(&ev.hemo), 1, &mut Silence).features;
    if pcg.grade > 0 {
        out.push(json!({ "id": "murmur", "label_fr": format!("souffle {} grade {}/6", pcg.murmur.label_fr(), pcg.grade),
            "value": pcg.murmur_peak_flow, "unit": "mL/s", "threshold": 3.0,
            "source": "signals.py : souffle si débit régurgitant > 3 mL/s ; grade heuristique (à calibrer)" }));
    }
    if pcg.s3 {
        out.push(json!({ "id": "s3_gallop", "label_fr": "bruit de galop (B3)", "value": h.la_mean, "unit": "mmHg",
            "threshold": 15.0, "source": "signals.py : B3 si POG > 15 mmHg et remplissage rapide" }));
    }
    if h.la_mean > 20.0 {
        out.push(json!({ "id": "pulmonary_edema_risk", "label_fr": "œdème pulmonaire probable (POG élevée)",
            "value": h.la_mean, "unit": "mmHg", "threshold": 20.0,
            "source": "MSD Vet Manual (modules/diseases/mmvd.json) : œdème si POG > ~20 mmHg" }));
    }
    let na_k = m.chem.na / m.chem.k;
    if na_k < 27.0 {
        out.push(json!({ "id": "low_na_k_ratio", "label_fr": "rapport Na:K bas (évocateur d'hypoadrénocorticisme)",
            "value": na_k, "unit": "", "threshold": 27.0,
            "source": "MSD Vet Manual (modules/diseases/addison.json) : Na:K < 27" }));
    }
    if m.chem.creat > 125.0 {
        out.push(
            json!({ "id": "azotemia", "label_fr": "azotémie (créatininémie ≥ stade IRIS 2)",
            "value": m.chem.creat, "unit": "µmol/L", "threshold": 125.0,
            "source": "IRIS (modules/diseases/ckd.json) : stade 2 dès 125 µmol/L" }),
        );
    }
    if m.chem.k > 5.5 {
        out.push(json!({ "id": "hyperkalemia_ecg", "label_fr": "hyperkaliémie avec modifications de l'ECG (T ample)",
            "value": m.chem.k, "unit": "mmol/L", "threshold": 5.5,
            "source": "signals.py : effets de K+ sur l'ECG au-delà de 5,5 mmol/L" }));
    }
    out
}

fn signals_json(
    ev: &Evaluation,
    m: &Map<String, Json>,
    default_seed: u64,
) -> Result<Json, ApiError> {
    let duration_s = opt_f64(m, "duration_s", 8.0)?;
    if !(0.5..=300.0).contains(&duration_s) {
        return Err(ApiError::one("/duration_s", "durée entre 0,5 et 300 s"));
    }
    let fs = opt_f64(m, "fs", 500.0)?;
    if !(50.0..=2000.0).contains(&fs) {
        return Err(ApiError::one("/fs", "fréquence ECG entre 50 et 2000 Hz"));
    }
    let seed = opt_u64(m, "seed", default_seed)?;
    let s = patient_signals(
        ev,
        &SignalOptions {
            duration_s,
            ecg_fs: fs,
            pcg_beats: 4,
        },
        seed,
    );
    let f = s.ecg.features;
    let pf = s.pcg.features;
    let w = &ev.hemo.waveforms;
    let hrv = s.autonomic.hrv.map(|h| {
        json!({ "fc_moy": h.fc_moy, "sdnn_ms": h.sdnn_ms, "rmssd_ms": h.rmssd_ms,
                "lf_hf": if h.lf_hf.is_finite() { Json::from(h.lf_hf) } else { Json::Null },
                "rsa_bpm": h.rsa_bpm })
    });
    let f32s =
        |v: &[f64]| -> Vec<Json> { v.iter().map(|x| Json::from(*x as f32 as f64)).collect() };
    // Respiration (modèle autonome, 20 Hz) sur la même fenêtre que l'ECG : volume pulmonaire
    // au-dessus de la fin d'expiration = profil normalisé × volume courant alvéolaire (VA/FR ;
    // l'espace mort n'est pas modélisé), débit = dérivée du volume, FC et PAM instantanées.
    let au = &s.autonomic;
    let n_resp = au.t.iter().take_while(|t| **t < duration_s).count();
    let vt_alv_ml = ev.metabolic.art.va / ev.metabolic.art.rr * 1000.0;
    let volume: Vec<f64> = au.rho[..n_resp].iter().map(|r| r * vt_alv_ml).collect();
    let fs_resp = if au.t.len() > 1 {
        1.0 / (au.t[1] - au.t[0])
    } else {
        20.0
    };
    let flow: Vec<f64> = (0..n_resp)
        .map(|i| {
            let (a, b) = (i.saturating_sub(1), (i + 1).min(n_resp - 1));
            if b > a {
                (volume[b] - volume[a]) / ((b - a) as f64) * fs_resp
            } else {
                0.0
            }
        })
        .collect();
    // PAS et PAD instantanées : PAM instantanée du modèle autonome décalée de l'écart PAS − PAM
    // et PAD − PAM du battement simulé (pression pulsée supposée constante sur la fenêtre :
    // le modèle hémodynamique ne simule pas les battements un par un).
    let hs = &ev.hemo.summary;
    let sap: Vec<f64> = au.map[..n_resp]
        .iter()
        .map(|m| m + (hs.sap - hs.map))
        .collect();
    let dap: Vec<f64> = au.map[..n_resp]
        .iter()
        .map(|m| m + (hs.dap - hs.map))
        .collect();
    Ok(json!({
        "ecg": f32s(&s.ecg.v), "ecg_fs": fs, "r_times": s.ecg.r_times,
        "ecg_features": { "pr": f.pr, "qrs": f.qrs, "qt": f.qt, "p_amp": f.p_amp, "t_amp": f.t_amp, "r_amp": f.r_amp },
        "pcg": f32s(&s.pcg.v), "pcg_fs": PCG_FS,
        "pcg_features": { "s1": pf.s1, "s2": pf.s2, "s3": pf.s3, "murmur": pf.murmur.label_fr(),
                          "grade": pf.grade, "murmur_peak_flow": pf.murmur_peak_flow },
        "pressures": { "dt": w.t.get(1).copied().unwrap_or(0.002), "period": ev.hemo.timing.period,
                       "ao": f32s(&w.p_ao), "lv": f32s(&w.p_lv), "la": f32s(&w.p_pv),
                       "pa": f32s(&w.p_pa), "rv": f32s(&w.p_rv) },
        "pressures_aligned": beat_pressures(ev, &s.autonomic.beats, seed, duration_s, 250.0)?,
        "hrv": hrv,
        "respiration": { "fs": fs_resp, "rr_per_min": ev.metabolic.art.rr, "vt_alv_mL": vt_alv_ml,
                         "volume_mL": f32s(&volume), "flow_mL_s": f32s(&flow),
                         "hr": f32s(&au.hr[..n_resp]), "map": f32s(&au.map[..n_resp]),
                         "sap": f32s(&sap), "dap": f32s(&dap) },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let read = |p: &str| -> Json {
            serde_json::from_str(&std::fs::read_to_string(root.join(p)).unwrap()).unwrap()
        };
        let ds: Vec<Json> = [
            "mmvd",
            "ckd",
            "addison",
            "pneumonia",
            "atrial_fibrillation",
            "av_block",
            "ventricular_premature",
            "sick_sinus",
        ]
        .iter()
        .map(|d| read(&format!("diseases/{d}.json")))
        .collect();
        let dr: Vec<Json> = [
            "atropine",
            "betabloquant",
            "furosemide",
            "opioide",
            "pimobendan",
        ]
        .iter()
        .map(|d| read(&format!("drugs/{d}.json")))
        .collect();
        json!({ "patient": read("healthy_dog.json"), "diseases": ds, "drugs": dr,
                "auscultation": read("auscultation.json") })
        .to_string()
    }

    #[test]
    fn repeated_dosing_and_examination_at_any_time() {
        let mut e = Engine::new();
        e.load_modules(&bundle());
        let id = j(&e.new_patient(r#"{"variability": false}"#))["patient_id"]
            .as_str()
            .unwrap()
            .to_string();
        e.add_disease(&id, r#"{"id": "mmvd", "stage": "C"}"#);
        // Furosémide et pimobendane per os toutes les 12 h pendant 3 jours.
        let doses: Vec<Json> = (0..6)
            .flat_map(|k| {
                let t = 12.0 * k as f64;
                [
                    json!({"t_h": t, "drug": "furosemide", "mg_per_kg": 2.0, "route": "po"}),
                    json!({"t_h": t, "drug": "pimobendan", "mg_per_kg": 0.25, "route": "po"}),
                ]
            })
            .collect();
        let t_eval: Vec<f64> = (0..=288).map(|i| i as f64 * 0.25).collect();
        let tc = j(&e.timecourse(
            &id,
            &json!({ "doses": doses, "t_end_h": 72.0, "t_eval_h": t_eval,
                                                "state_times_h": [1.0, 11.0, 61.0, 71.0] })
            .to_string(),
        ));
        assert_eq!(tc["ok"], true, "{tc}");
        assert_eq!(tc["effect"]["furosemide"].as_array().unwrap().len(), 289);
        assert_eq!(tc["states_at"].as_array().unwrap().len(), 4);
        let c = |d: &str, h: f64| tc["conc_mg_L"][d][(h * 4.0) as usize].as_f64().unwrap();
        // Pic après chaque prise, creux avant la suivante ; demi-vie courte : pas d'accumulation.
        assert!(c("pimobendan", 1.0) > 5.0 * c("pimobendan", 11.75));
        assert!((c("pimobendan", 61.0) - c("pimobendan", 1.0)).abs() < 0.05 * c("pimobendan", 1.0));
        assert!(
            tc["D"][288].as_f64().unwrap() > 0.0,
            "déficit de volume du diurétique"
        );
        // Examen au pic et au creux : mêmes concentrations que la cinétique, états différents.
        let mut at = |h: f64| j(&e.state(&id, &json!({ "doses": doses, "at_h": h }).to_string()));
        let (peak, trough) = (at(61.0), at(71.0));
        assert_eq!(peak["ok"], true, "{peak}");
        let states_at = tc["states_at"].as_array().unwrap();
        for (st, sa) in [(&peak, &states_at[2]), (&trough, &states_at[3])] {
            // Même modèle, mais segments d'intégration PK différents ([0, t] contre la grille) :
            // concentrations égales à ~1e-14, puis le solveur hémodynamique adaptatif (rtol 1e-6)
            // suit une autre séquence de pas, d'où un accord à la précision du solveur.
            for (k, tol) in [
                ("k", 1e-9),
                ("na", 1e-9),
                ("hr", 1e-4),
                ("map", 1e-4),
                ("cvp", 1e-4),
            ] {
                let (a, b) = (st[k].as_f64().unwrap(), sa[k].as_f64().unwrap());
                assert!(
                    (a - b).abs() <= tol * a.abs().max(b.abs()),
                    "{k} : {a} / {b}"
                );
            }
        }
        assert_ne!(peak["map"], trough["map"]);
        // Auscultation au pic
        let p = j(&e.pcg(
            &id,
            &json!({ "doses": doses, "at_h": 61.0, "duration_s": 3 }).to_string(),
        ));
        assert_eq!(p["ok"], true, "{p}");
        // Erreurs structurées
        let both = json!({ "doses": doses, "at_h": 1.0, "conc": {"pimobendan": 0.03} });
        assert_eq!(j(&e.state(&id, &both.to_string()))["ok"], false);
        assert_eq!(
            j(&e.state(&id, &json!({ "doses": doses }).to_string()))["ok"],
            false
        );
    }

    #[test]
    fn heart_animation_is_driven_by_model_outputs() {
        let mut e = Engine::new();
        e.load_modules(&bundle());
        let arr = |v: &Json| -> Vec<f64> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect()
        };
        for (dis, sev) in [
            ("", 0.0),
            ("av_block", 0.85),
            ("av_block", 0.5),
            ("atrial_fibrillation", 0.8),
            ("mmvd", 0.8),
        ] {
            let id = j(&e.new_patient(r#"{"variability": false}"#))["patient_id"]
                .as_str()
                .unwrap()
                .to_string();
            if !dis.is_empty() {
                e.add_disease(&id, &json!({"id": dis, "severity": sev}).to_string());
            }
            let r = j(&e.heart(&id, r#"{"duration_s": 6, "fs": 200, "seed": 3}"#));
            assert_eq!(r["ok"], true, "{dis}: {r}");
            let sr = &r["series"];
            let (lv, act, q_ao, q_mi) = (
                arr(&sr["v_lv"]),
                arr(&sr["act_v"]),
                arr(&sr["q_ao"]),
                arr(&sr["q_mi"]),
            );
            assert_eq!(lv.len(), 1200);
            let (lo, hi) = lv
                .iter()
                .fold((f64::MAX, f64::MIN), |a, &x| (a.0.min(x), a.1.max(x)));
            assert!(hi - lo > 5.0, "{dis}: contraction VG {lo}–{hi}");
            // Éjection aortique seulement pendant l'activation ventriculaire.
            let qmax = q_ao.iter().copied().fold(0.0, f64::max);
            for i in 0..lv.len() {
                if q_ao[i] > 0.05 * qmax {
                    assert!(
                        act[i] > 0.0,
                        "{dis}: éjection sans activation à {}",
                        i as f64 / 200.0
                    );
                }
            }
            if dis == "mmvd" {
                assert!(
                    q_mi.iter().any(|&x| x < -10.0),
                    "{dis}: régurgitation mitrale"
                );
            }
            let ecg = arr(&r["ecg"]);
            let el = &r["electrical"];
            let vent = el["ventricular"].as_array().unwrap();
            assert!(vent.len() >= 3, "{dis}: {vent:?}");
            for b in vent {
                let t = b["t"].as_f64().unwrap();
                let (on, off) = (
                    b["qrs_on"].as_f64().unwrap(),
                    b["qrs_off"].as_f64().unwrap(),
                );
                assert!(on < t && t < off && b["t_end"].as_f64().unwrap() > off);
                // le pic R de l'ECG est dans la fenêtre du QRS
                let (i0, i1) = (
                    (on * 500.0).max(0.0) as usize,
                    ((off * 500.0) as usize).min(ecg.len()),
                );
                if i1 > i0 + 2 && t > 0.1 && t < 5.9 {
                    let k = (i0..i1)
                        .max_by(|&a, &c| ecg[a].abs().total_cmp(&ecg[c].abs()))
                        .unwrap();
                    assert!((k as f64 / 500.0 - t).abs() < 0.03, "{dis}: R hors QRS");
                }
                // la mécanique démarre au pic R : activation nulle juste avant
                let i = (t * 200.0) as isize - 2;
                if i > 0 && b["wide"] == false && (i as usize) < act.len() {
                    let prev = vent
                        .iter()
                        .map(|x| x["t"].as_f64().unwrap())
                        .filter(|&x| x < t)
                        .fold(f64::MIN, f64::max);
                    if t - prev > 0.5 {
                        assert!(act[i as usize] < 0.05, "{dis}: activation avant l'onde R");
                    }
                }
                if b["origin"] == "sinus" {
                    assert!(b["t_his"].as_f64().unwrap() < on);
                }
            }
            let fates: Vec<&str> = el["impulses"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x["fate"].as_str().unwrap())
                .collect();
            if dis == "av_block" {
                assert!(r["electrical"]["atrial"].as_array().unwrap().len() > vent.len());
                assert!(
                    fates.iter().any(|f| *f != "conducted"),
                    "{dis} {sev}: {fates:?}"
                );
            }
            if dis.is_empty() {
                assert!(fates.iter().all(|f| *f == "conducted"), "{fates:?}");
            }
            if dis == "atrial_fibrillation" {
                assert!(el["atrial"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|a| a["organized"] == false));
                assert!(fates.contains(&"concealed") || fates.contains(&"nodal_refractory"));
            }
        }
    }

    #[test]
    fn pcg_message_with_aligned_ecg_and_pressures() {
        let mut e = Engine::new();
        assert_eq!(
            j(&e.load_modules(&bundle()))["loaded"]["auscultation"],
            true
        );
        let id = j(&e.new_patient(r#"{"variability": false}"#))["patient_id"]
            .as_str()
            .unwrap()
            .to_string();
        e.add_disease(&id, r#"{"id": "mmvd", "severity": 0.3}"#);
        let msg = json!({ "op": "pcg", "patient": id,
                          "options": { "seed": 4, "duration_s": 6, "with_ecg": true, "with_pressures": true, "hr": 90 } });
        let r = j(&e.message(&msg.to_string()));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["murmur"], "holosystolique apexien gauche");
        assert!(r["grade"].as_u64().unwrap() >= 2);
        // préréglage : FC intrinsèque du nœud sinusal ; le baroréflexe l'ajuste ensuite (et,
        // depuis l'effet dromotrope du sympathique, le PR suit le tonus : 92,2 à 90 imposés)
        assert!(
            (r["hr"].as_f64().unwrap() - 90.0).abs() < 3.0,
            "{}",
            r["hr"]
        );
        let wav = r["wav_base64"].as_str().unwrap();
        assert!(wav.starts_with("UklGR")); // « RIFF »
        assert_eq!(wav.len(), (44 + 6 * 4000 * 2usize).div_ceil(3) * 4);
        // Mêmes battements pour le son et l'ECG (le son s'arrête 0,5 s avant la fin).
        let beats: Vec<f64> = r["beats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let rt: Vec<f64> = r["r_times"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert!(
            !beats.is_empty() && rt.starts_with(&beats),
            "{beats:?} / {rt:?}"
        );
        assert_eq!(r["ecg"].as_array().unwrap().len(), 3000);
        assert_eq!(r["pressures"]["ao"].as_array().unwrap().len(), 1500);
        // Erreurs structurées
        assert_eq!(j(&e.message(r#"{"op": "rien"}"#))["ok"], false);
        assert_eq!(j(&e.pcg(&id, r#"{"site": "lune"}"#))["ok"], false);
        assert_eq!(j(&e.pcg(&id, r#"{"hr": 500}"#))["ok"], false);
        let mut b: Json = serde_json::from_str(&bundle()).unwrap();
        b.as_object_mut().unwrap().remove("auscultation");
        e.load_modules(&b.to_string());
        let id = j(&e.new_patient("{}"))["patient_id"]
            .as_str()
            .unwrap()
            .to_string();
        let r = j(&e.pcg(&id, "{}"));
        assert!(
            r["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("auscultation"),
            "{r}"
        );
    }

    fn j(s: &str) -> Json {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn outcome_reports_decompensation_and_death() {
        let mut e = Engine::new();
        assert_eq!(j(&e.load_modules(&bundle()))["ok"], true);
        let p = j(&e.new_patient(r#"{"bw": 20, "variability": false}"#));
        let id = p["patient_id"].as_str().unwrap().to_string();
        let st = j(&e.state(&id, "{}"));
        assert_eq!(st["outcome"]["state"], "stable");
        let st = j(&e.state(&id, r#"{"conc": {"pimobendan": 3}}"#));
        assert_eq!(st["outcome"]["state"], "decompensated");
        let ids: Vec<&str> = st["outcome"]["mechanisms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"coronary_ischemia"), "{ids:?}");
        // chien vigile : l'opioïde massif ne tue plus (ni apnée ni PaCO2 > 67 mmHg, Bailey 1987)
        let st = j(&e.state(&id, r#"{"conc": {"opioide": 2}}"#));
        assert_ne!(st["outcome"]["state"], "dead", "{st}");
        // crise addisonienne + pimobendane massif : choc irréversible (PAM < 40 mmHg),
        // dernière passe viable rapportée
        let p = j(&e.new_patient(r#"{"bw": 20, "variability": false}"#));
        let id = p["patient_id"].as_str().unwrap().to_string();
        e.add_disease(&id, r#"{"id": "addison", "severity": 0.85}"#);
        let st = j(&e.state(&id, r#"{"conc": {"pimobendan": 3}, "signals": true}"#));
        assert_eq!(st["ok"], true);
        assert_eq!(st["outcome"]["state"], "dead");
        assert_eq!(st["outcome"]["cause"], "irreversible_shock");
        assert!(st["map"].is_number());
        assert!(st["signals"].is_null());
        let pcg = j(&e.pcg(&id, r#"{"conc": {"pimobendan": 3}}"#));
        assert_eq!(pcg["ok"], false);
        // au cours d'un traitement : l'instant du décès est daté
        let tc = j(&e.timecourse(
            &id,
            r#"{"doses": [{"t_h": 1, "drug": "pimobendan", "mg_per_kg": 20, "route": "iv"}],
                "t_end_h": 6, "t_eval_h": [0, 1], "state_times_h": [0, 1.05, 6]}"#,
        ));
        assert_eq!(tc["ok"], true, "{tc}");
        let sa = tc["states_at"].as_array().unwrap();
        assert_eq!(sa[0]["outcome"], "stable");
        assert_eq!(sa[1]["outcome"], "dead");
        assert_eq!(tc["death_t_h"], 1.05);
        // décès absorbant : à 6 h la concentration a baissé mais le patient reste décédé
        assert_eq!(sa[2]["outcome"], "dead");
        assert!(sa[2].get("map").is_none());
    }

    #[test]
    fn full_session_matches_cli_contract() {
        let mut e = Engine::new();
        assert_eq!(e.api_version(), "1.1.0");
        assert_eq!(j(&e.load_modules(&bundle()))["ok"], true);
        let l = j(&e.list_modules());
        assert_eq!(l["diseases"].as_array().unwrap().len(), 8);
        let p = j(&e.new_patient(r#"{"bw": 20, "variability": false}"#));
        let id = p["patient_id"].as_str().unwrap().to_string();
        let c = j(&e.add_disease(&id, r#"{"id": "mmvd", "severity": 0.8}"#));
        assert_eq!(c["case"]["stage"], "C");
        // Modificateurs calculés et blocs où ils interviennent.
        let m0 = &c["case"]["modifiers"][0];
        assert_eq!(
            (m0["param"].as_str(), m0["op"].as_str()),
            (Some("g_mr"), Some("add"))
        );
        assert_eq!(m0["blocks"][0], "valves");
        assert_eq!(l["blocks"].as_array().unwrap().len(), 15);
        let ckd = l["diseases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == "ckd")
            .unwrap();
        assert!(ckd["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["conditional"] == true));
        let st = j(&e.state(&id, "{}"));
        // Mêmes valeurs que le golden du CLI (mmvd_s0.8) à la tolérance près.
        let g: Json = serde_json::from_str(
            &std::fs::read_to_string(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/golden/cases.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let golden = g["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "mmvd_s0.8")
            .unwrap();
        for (k, v) in golden["expected"].as_object().unwrap() {
            let (a, b) = (st[k].as_f64().unwrap(), v.as_f64().unwrap());
            assert!(
                (a - b).abs() <= 0.03 * a.abs().max(b.abs()) + 0.02,
                "{k}: {a} / {b}"
            );
        }
        // conductance de base de la MMVD 0,8 (0,9458·0,8^1,111 ≈ 0,74) ; la conductance effective,
        // fixée par battement selon le VTD (orifice dynamique), est plus élevée
        assert!(st["params"]["g_mr"].as_f64().unwrap() > 0.5);
        assert_eq!(st["params"].as_object().unwrap().len(), Params::NAMES.len());
        let signs: Vec<&str> = st["emergent_signs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        // le signe d'œdème suit la POG du patient (seuil 20 mmHg), quelle que soit sa valeur
        let edema = st["la_mean"].as_f64().unwrap() > 20.0;
        assert!(
            signs.contains(&"murmur") && signs.contains(&"pulmonary_edema_risk") == edema,
            "{signs:?}"
        );
        let sg = j(&e.signals(&id, r#"{"duration_s": 4}"#));
        assert_eq!(sg["ok"], true);
        // Respiration : 4 s à 20 Hz, volume entre 0 et le volume courant alvéolaire.
        let r = &sg["respiration"];
        let vol: Vec<f64> = r["volume_mL"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(vol.len(), 80);
        assert_eq!(r["hr"].as_array().unwrap().len(), 80);
        let (sap0, dap0, map0) = (
            r["sap"][0].as_f64().unwrap(),
            r["dap"][0].as_f64().unwrap(),
            r["map"][0].as_f64().unwrap(),
        );
        assert!(sap0 > map0 && map0 > dap0, "{sap0} {map0} {dap0}");
        let vt = r["vt_alv_mL"].as_f64().unwrap();
        assert!(
            vol.iter().all(|v| *v >= -1e-3 && *v <= vt + 1e-3),
            "{vt} {vol:?}"
        );
        assert!(
            vol.iter().copied().fold(0.0, f64::max) > 0.9 * vt,
            "au moins une inspiration complète"
        );
        assert_eq!(sg["ecg"].as_array().unwrap().len(), 2000);
        assert!(sg["pcg_features"]["grade"].as_u64().unwrap() >= 5);
        let tc = j(&e.timecourse(
            &id,
            r#"{"doses": [{"t_h": 0, "drug": "furosemide", "mg_per_kg": 2, "route": "iv"}],
                "t_end_h": 4, "t_eval_h": [0, 1, 4], "states": true}"#,
        ));
        assert_eq!(tc["states"].as_array().unwrap().len(), 3);
        assert!(tc["D"][2].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn errors_are_structured_never_panics() {
        let mut e = Engine::new();
        let r = j(&e.new_patient("{}"));
        assert_eq!(r["ok"], false);
        assert!(r["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("loadModules"));
        let mut b = j(&bundle());
        b["diseases"][1]["effects"][0]["op"] = json!("pow");
        let r = j(&e.load_modules(&b.to_string()));
        assert_eq!(r["ok"], false);
        assert_eq!(r["errors"][0]["path"], "/diseases/1/effects/0/op");
        assert_eq!(j(&e.load_modules("not json"))["ok"], false);
        e.load_modules(&bundle());
        for bad in [r#"{"bw": -3}"#, r#"{"bw": "x"}"#, "[1]"] {
            assert_eq!(j(&e.new_patient(bad))["ok"], false, "{bad}");
        }
        let id = j(&e.new_patient("{}"))["patient_id"]
            .as_str()
            .unwrap()
            .to_string();
        for bad in [
            r#"{"id": "x"}"#,
            r#"{"id": "ckd", "stage": "IRIS9"}"#,
            r#"{"id": "ckd", "severity": 2}"#,
            r#"{"id": "ckd", "stage": "IRIS2", "severity": 0.3}"#,
        ] {
            assert_eq!(j(&e.add_disease(&id, bad))["ok"], false, "{bad}");
        }
        assert_eq!(j(&e.state("p999", "{}"))["ok"], false);
        assert_eq!(
            j(&e.state(&id, r#"{"conc": {"aspirine": 1}}"#))["ok"],
            false
        );
        assert_eq!(j(&e.signals(&id, r#"{"fs": 1e6}"#))["ok"], false);
        assert_eq!(
            j(&e.timecourse(
                &id,
                r#"{"doses": [{"drug": "x", "mg_per_kg": 1, "route": "iv"}]}"#
            ))["ok"],
            false
        );
    }

    #[test]
    fn same_seed_same_patient() {
        let mut e = Engine::new();
        e.load_modules(&bundle());
        let run = |e: &mut Engine| {
            let id = j(&e.new_patient(r#"{"seed": 42}"#))["patient_id"]
                .as_str()
                .unwrap()
                .to_string();
            e.add_disease(&id, r#"{"id": "ckd"}"#);
            let st = j(&e.state(&id, "{}"));
            (st["creat"].clone(), st["case"].clone())
        };
        assert_eq!(run(&mut e), run(&mut e));
    }
}
