//! API par messages JSON (docs/KMP.md), en Rust pur : partagée par la liaison UniFFI
//! (`vetsim-ffi`, Android/JVM) et la liaison WebAssembly (`vetsim-wasm`, navigateur).
//!
//! Chaque méthode reçoit et renvoie une chaîne JSON. Une entrée invalide renvoie
//! `{"ok": false, "errors": [{"path": ..., "message": ...}]}` ; aucune méthode ne panique
//! volontairement (la couche FFI ajoute un `catch_unwind` en dernier recours).
//!
//! Usage pédagogique uniquement : aucun résultat ne sert à décider d'un traitement réel.

mod anesthesia;
pub use anesthesia::{parse_event, snapshot_json};

use serde_json::{json, Map, Value as Json};
use std::collections::BTreeMap;
use vetsim_core::blocks::{blocks_of, Block};
use vetsim_core::case::{Case, DrawSource, NoNoise};
use vetsim_core::dsp::{base64, wav_pcm16};
use vetsim_core::pcg::{patient_pcg, AuscultationConfig, PcgOptions};
use vetsim_core::pkpd::{apply_drug_effects_sites, emax_effect, timecourse, Dose, Route};
use vetsim_core::rng::NoiseSource;
use vetsim_core::signals::{make_pcg, patient_signals, streams, PcgInputs, SignalOptions, PCG_FS};
use vetsim_core::{
    evaluate, sample_trajectory, Catalog, DiseaseModule, DrugModule, Evaluation, LoadError, Params,
    Patient, PatientModule, Scale, SimRng, VariationConfig,
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
    variation: Option<VariationConfig>,
    patients: BTreeMap<String, Session>,
    next_id: u64,
    anesthesia: Option<anesthesia::AnesthesiaCatalog>,
    anesthesia_sessions: BTreeMap<String, vetsim_core::anesthesia::Session>,
    renal: Option<vetsim_core::renal::RenalModel>,
    echo: Option<vetsim_core::echo::EchoConfig>,
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
        // Variabilité intra-individuelle des trajectoires Monte-Carlo (optionnelle).
        let variation = match m.get("variation") {
            None | Some(Json::Null) => None,
            Some(a) => match VariationConfig::from_json(&a.to_string(), "variation") {
                Ok(c) => Some(c),
                Err(e) => {
                    errors.extend(load_issues("/variation", e));
                    None
                }
            },
        };
        // Modules de l'anesthésie (optionnels).
        let anesthesia = match m.get("anesthesia") {
            None | Some(Json::Null) => None,
            Some(a) => anesthesia::load(a, &mut errors),
        };
        // Rein dynamique (optionnel).
        let renal = match m.get("renal") {
            None | Some(Json::Null) => None,
            Some(a) => match vetsim_core::renal::RenalModel::from_json(&a.to_string(), "renal") {
                Ok(c) => Some(c),
                Err(e) => {
                    errors.extend(load_issues("/renal", e));
                    None
                }
            },
        };
        // Échocardiographie virtuelle (optionnelle).
        let echo = match m.get("echo") {
            None | Some(Json::Null) => None,
            Some(a) => match vetsim_core::echo::EchoConfig::from_json(&a.to_string(), "echo") {
                Ok(c) => Some(c),
                Err(e) => {
                    errors.extend(load_issues("/echo", e));
                    None
                }
            },
        };
        if !errors.is_empty() {
            return Err(ApiError(errors));
        }
        self.renal = renal;
        self.echo = echo;
        self.auscultation = auscultation;
        self.variation = variation;
        self.anesthesia = anesthesia;
        self.anesthesia_sessions.clear();
        let catalog = Catalog::new(patient.expect("contrôlé"), diseases, drugs)
            .map_err(|e| ApiError::one("", e.to_string()))?;
        let counts = json!({ "diseases": catalog.diseases.len(), "drugs": catalog.drugs.len(),
                             "auscultation": self.auscultation.is_some(),
                             "variation": self.variation.is_some(),
                             "anesthesia": self.anesthesia.is_some(),
                             "renal": self.renal.is_some(),
                             "echo": self.echo.is_some() });
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
        let mut conc_e: Vec<(String, f64)> = Vec::new();
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
            conc_e = tc.conc_e.iter().map(|(k, v)| (k.clone(), v[0])).collect();
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
        Ok(apply_drug_effects_sites(
            &base,
            &self.catalog()?.drugs,
            &conc,
            &conc_e,
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
                        let pi = effects_at(&base, &c.drugs, &tcs, i)?;
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
    /// concentrations, `D`, `U` et (si `states`) l'état plat à chaque instant. Avec
    /// `state_times_h`, l'option `thermal` intègre en plus le bilan thermique
    /// ([`vetsim_core::periph::thermal_course`]) et injecte la température centrale dans chaque
    /// état renvoyé (`temp_c`, et ses effets sur la FC intrinsèque, la VO2 et la P50).
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
                // Température centrale : intégrée séparément (échelle de temps lente), puis
                // injectée dans chaque état (FC intrinsèque, VO2, P50 ; periph.rs).
                let thermal = if opt_bool(m, "thermal", false)? {
                    Some(vetsim_core::periph::thermal_course(
                        &s.patient.params,
                        &c.drugs,
                        &ds,
                        &times,
                    )?)
                } else {
                    None
                };
                let mut states = Vec::with_capacity(tcs.t.len());
                let mut death: Option<f64> = None;
                let mut cause = Json::Null;
                for i in 0..tcs.t.len() {
                    // décès absorbant : plus d'évaluation après le premier instant de décès
                    if death.is_some() {
                        states.push(dead_entry(&cause));
                        continue;
                    }
                    let mut p = effects_at(&s.patient.params, &c.drugs, &tcs, i)?;
                    if let Some(th) = &thermal {
                        p.temp_c = th[i].tc;
                    }
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
                    let p = effects_at(&s.patient.params, &c.drugs, &tc, i)?;
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

    /// Rein dynamique (`vetsim_core::renal`) : `{"doses": [...] (optionnel), "t_end_h": 24,
    /// "dt_h": 0.25, "ace_inhibition": 0, "na_intake_factor": 1, "k_intake_factor": 1,
    /// "k_loads": [{"t_h": 0, "mmol_per_kg": 2.5}],
    /// "every_h": 1}` → séries aux instants multiples de
    /// `every_h` : bilans (ΔVEC, Δplasma, K, K intracellulaire), activité RAAS, diurèse,
    /// natriurèse, kaliurèse, FE_Na, FE_K, créatinine et urée cinétiques, et l'état
    /// physiologique (PAM, FC, débit, DFG, Ht). Décès absorbant (`death_t_h`, `cause`).
    pub fn renal_course(&mut self, patient_id: &str, options: &str) -> String {
        self.respond(|e| {
            let o = parse(options, "options")?;
            let m = obj(&o, "")?;
            let model = e
                .renal
                .clone()
                .ok_or_else(|| ApiError::one("", "module « renal » non chargé (loadModules)"))?;
            let ds = if m.contains_key("doses") {
                parse_doses(m)?
            } else {
                Vec::new()
            };
            let opt = vetsim_core::renal::RenalOptions {
                t_end_h: opt_f64(m, "t_end_h", 24.0)?,
                dt_h: opt_f64(m, "dt_h", 0.25)?,
                ace_inhibition: opt_f64(m, "ace_inhibition", 0.0)?,
                na_intake_factor: opt_f64(m, "na_intake_factor", 1.0)?,
                k_intake_factor: opt_f64(m, "k_intake_factor", 1.0)?,
                k_loads: match m.get("k_loads") {
                    None => Vec::new(),
                    Some(Json::Array(a)) => a
                        .iter()
                        .enumerate()
                        .map(|(i, x)| {
                            let o = obj(x, &format!("/k_loads/{i}"))?;
                            let g = |k: &str| {
                                o.get(k).and_then(Json::as_f64).ok_or_else(|| {
                                    ApiError::one(format!("/k_loads/{i}/{k}"), "nombre attendu")
                                })
                            };
                            Ok((g("t_h")?, g("mmol_per_kg")?))
                        })
                        .collect::<Result<_, ApiError>>()?,
                    Some(_) => return Err(ApiError::one("/k_loads", "tableau attendu")),
                },
            };
            if opt.t_end_h > 24.0 * 60.0 {
                return Err(ApiError::one("/t_end_h", "au plus 60 jours"));
            }
            let every = opt_f64(m, "every_h", 1.0)?;
            if !(every.is_finite() && every > 0.0) {
                return Err(ApiError::one("/every_h", "nombre > 0 attendu"));
            }
            let s = e.session(patient_id)?.clone();
            let c = e.catalog()?;
            let rc =
                vetsim_core::renal::renal_course(&model, &s.patient.params, &c.drugs, &ds, &opt)?;
            // domaine validé : furosémide, horizon ≤ 14 j, sans levier (vetsim_core::renal)
            let validated = ds.iter().all(|d| d.drug == "furosemide")
                && opt.t_end_h <= 336.0
                && opt.ace_inhibition == 0.0
                && opt.na_intake_factor == 1.0
                && opt.k_intake_factor == 1.0
                && opt.k_loads.is_empty();
            let stride = ((every / opt.dt_h).round() as usize).max(1);
            let pts: Vec<_> = rc
                .points
                .iter()
                .enumerate()
                .filter(|(i, _)| i % stride == 0 || *i + 1 == rc.points.len())
                .map(|(_, p)| p)
                .collect();
            let col = |f: &dyn Fn(&vetsim_core::renal::RenalPoint) -> f64| -> Json {
                json!(pts.iter().map(|p| f(p)).collect::<Vec<_>>())
            };
            let ev = |f: &dyn Fn(&Evaluation) -> f64| -> Json {
                json!(pts
                    .iter()
                    .map(|p| p.eval.as_ref().map(|e| f(e)))
                    .collect::<Vec<_>>())
            };
            Ok(json!({
                "ok": true,
                "t": col(&|p| p.t_h),
                "d_ecf_mL_kg": col(&|p| p.d_ecf_ml_kg),
                "d_plasma_mL_kg": col(&|p| p.d_plasma_ml_kg),
                "k": col(&|p| p.k),
                "d_k_ic_mmol_kg": col(&|p| p.d_k_ic_mmol_kg),
                "raas": col(&|p| p.aldo),
                "creat": col(&|p| p.creat),
                "urea": col(&|p| p.urea),
                "urine_mL_kg_h": col(&|p| p.urine_ml_kg_h),
                "urine_cum_mL_kg": col(&|p| p.urine_cum_ml_kg),
                "una_mmol_kg_h": col(&|p| p.una_mmol_kg_h),
                "uk_mmol_kg_h": col(&|p| p.uk_mmol_kg_h),
                "una_mmol_L": col(&|p| p.una_mmol_l),
                "uk_mmol_L": col(&|p| p.uk_mmol_l),
                "uk_cum_mmol_kg": col(&|p| p.uk_cum_mmol_kg),
                "fe_na": col(&|p| p.fe_na),
                "fe_k": col(&|p| p.fe_k),
                "e_loop": col(&|p| p.e_loop),
                "map": ev(&|e| e.hemo.summary.map),
                "hr": ev(&|e| e.params.hr),
                "co": ev(&|e| e.hemo.summary.co),
                "cvp": ev(&|e| e.hemo.summary.cvp),
                "gfr": col(&|p| p.gfr),
                "hct": ev(&|e| e.metabolic.chem.hct),
                "death_t_h": rc.death.map(|d| d.0),
                "cause": rc.death.map(|d| d.1),
                "model_version": model.version,
                "validated": validated,
                "warning": (!validated).then_some(
                    "hors du domaine validé (furosémide seul, ≤ 14 j, sans levier alimentaire, charge de K ni IEC)"),
                "note": "rein dynamique (bilans Na, K, RAAS) ; usage pédagogique uniquement",
            }))
        })
    }

    /// Trajectoires Monte-Carlo d'un même animal (`modules/variation.json`) :
    /// `{"state_times_h": [0, 6, 12, ...], "doses": [...] (optionnel), "n": 50, "seed": 1,
    /// "keys": ["hr", "map", "creat"], "scale": {"natural": 0, "disease": 0, "drug": 1,
    /// "measurement": 0} (défaut : tirage unique par chien), "progression_per_day": {"ckd": 0.005}, "trajectories": 5}` →
    /// `{"t", "summary": {clé: {"p05","p25","p50","p75","p95","mean"}}, "alive_fraction",
    /// "severity", "trajectories": [...]}`. Les percentiles portent sur les réalisations vivantes
    /// à l'instant considéré ; le décès est absorbant. Reproductible pour une graine donnée.
    pub fn montecarlo(&mut self, patient_id: &str, options: &str) -> String {
        self.respond(|e| {
            let o = parse(options, "options")?;
            let m = obj(&o, "")?;
            let base_cfg = e.variation.clone().ok_or_else(|| {
                ApiError::one("", "configuration « variation » non chargée (loadModules)")
            })?;
            let times: Vec<f64> = m
                .get("state_times_h")
                .and_then(Json::as_array)
                .ok_or_else(|| ApiError::one("/state_times_h", "tableau d'instants (h) requis"))?
                .iter()
                .enumerate()
                .map(|(i, x)| {
                    x.as_f64()
                        .filter(|t| *t >= 0.0 && *t <= 24.0 * 365.0)
                        .ok_or_else(|| {
                            ApiError::one(format!("/state_times_h/{i}"), "instant entre 0 et 1 an")
                        })
                })
                .collect::<Result<_, _>>()?;
            if times.is_empty() || times.windows(2).any(|w| w[1] <= w[0]) {
                return Err(ApiError::one(
                    "/state_times_h",
                    "instants strictement croissants requis",
                ));
            }
            let n = opt_u64(m, "n", 30)? as usize;
            if !(1..=500).contains(&n) {
                return Err(ApiError::one("/n", "entre 1 et 500 trajectoires"));
            }
            if n * times.len() > 20_000 {
                return Err(ApiError::one(
                    "/n",
                    "au plus 20 000 évaluations (n × nombre d'instants)",
                ));
            }
            let seed = opt_u64(m, "seed", 0)?;
            let n_keep = opt_u64(m, "trajectories", 0)?.min(n as u64) as usize;
            // Par défaut : paramètres tirés une fois par chien (patient et PK/PD), sans nouveau
            // tirage instant par instant ; les fluctuations se demandent par `scale`.
            let mut scale = Scale {
                natural: 0.0,
                disease: 0.0,
                drug: 1.0,
                measurement: 0.0,
            };
            if let Some(sv) = m.get("scale").filter(|v| !v.is_null()) {
                let sm = obj(sv, "/scale")?;
                for (k, slot) in [
                    ("natural", &mut scale.natural),
                    ("disease", &mut scale.disease),
                    ("drug", &mut scale.drug),
                    ("measurement", &mut scale.measurement),
                ] {
                    let x = opt_f64(sm, k, *slot)?;
                    if !(0.0..=5.0).contains(&x) {
                        return Err(ApiError::one(format!("/scale/{k}"), "facteur entre 0 et 5"));
                    }
                    *slot = x;
                }
            }
            let mut rates = Vec::new();
            if let Some(pv) = m.get("progression_per_day").filter(|v| !v.is_null()) {
                for (k, v) in obj(pv, "/progression_per_day")? {
                    let r = v.as_f64().filter(|r| r.is_finite() && r.abs() <= 1.0).ok_or_else(
                        || {
                            ApiError::one(
                                format!("/progression_per_day/{k}"),
                                "sévérité par jour entre −1 et 1",
                            )
                        },
                    )?;
                    rates.push((k.clone(), r));
                }
            }
            let cfg = base_cfg.scaled(scale).with_progression(&rates);
            let doses = if m.contains_key("doses") {
                parse_doses(m)?
            } else {
                Vec::new()
            };
            let s = e.session(patient_id)?.clone();
            let c = e.catalog()?;
            let mods: Vec<&DiseaseModule> = s
                .patient
                .cases
                .iter()
                .map(|k| c.disease(&k.disease).expect("maladie du catalogue"))
                .collect();
            let default_keys = [
                "hr", "map", "co", "la_mean", "ph", "pao2", "lactate", "na", "k", "glucose",
                "creat", "urea", "hct",
            ];
            let keys: Vec<String> = match m.get("keys") {
                None | Some(Json::Null) => default_keys.iter().map(|k| k.to_string()).collect(),
                Some(Json::Array(a)) => a
                    .iter()
                    .enumerate()
                    .map(|(i, k)| {
                        k.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| ApiError::one(format!("/keys/{i}"), "chaîne attendue"))
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err(ApiError::one("/keys", "tableau attendu")),
            };
            let nt = times.len();
            // valeurs[clé][instant] = réalisations vivantes
            let mut values: Vec<Vec<Vec<f64>>> = vec![vec![Vec::with_capacity(n); nt]; keys.len()];
            let mut alive = vec![0usize; nt];
            let mut sev_all: Vec<Vec<Vec<f64>>> = vec![vec![Vec::with_capacity(n); nt]; mods.len()];
            let mut kept = Vec::with_capacity(n_keep);
            let mut deaths = Vec::with_capacity(n);
            for r in 0..n {
                let mut rng = SimRng::for_stream(seed.wrapping_add(r as u64), streams::VARIATION);
                let mut noise =
                    SimRng::for_stream(seed.wrapping_add(r as u64), streams::MEASUREMENT);
                let tr =
                    sample_trajectory(&s.patient, &mods, &c.drugs, &doses, &times, &cfg, &mut rng)?;
                let mut states = Vec::with_capacity(nt);
                let mut cause: Option<Json> = None;
                for p in &tr.params {
                    if let Some(c) = &cause {
                        states.push(dead_entry(c));
                        continue;
                    }
                    let st = state_entry(p)?;
                    if st["outcome"] == "dead" {
                        cause = Some(st["cause"].clone());
                    }
                    states.push(st);
                }
                apply_marker_kinetics(&mut states, &times, &tr.start)?;
                let mut death_t = Json::Null;
                for (i, st) in states.iter_mut().enumerate() {
                    if st["outcome"] == "dead" {
                        if death_t.is_null() {
                            death_t = json!(times[i]);
                        }
                        continue;
                    }
                    alive[i] += 1;
                    for (ki, k) in keys.iter().enumerate() {
                        if let Some(v) = st[k.as_str()].as_f64() {
                            let measured = cfg.measure(k, v, &mut noise);
                            st[k.as_str()] = Json::from(measured);
                            values[ki][i].push(measured);
                        }
                    }
                }
                for (di, sv) in tr.severity.iter().enumerate() {
                    for (i, x) in sv.iter().enumerate() {
                        sev_all[di][i].push(*x);
                    }
                }
                deaths.push(death_t.clone());
                if r < n_keep {
                    let mut series = Map::new();
                    for k in &keys {
                        series.insert(
                            k.clone(),
                            json!(states
                                .iter()
                                .map(|st| st[k.as_str()].as_f64())
                                .collect::<Vec<_>>()),
                        );
                    }
                    let conc: Map<String, Json> =
                        tr.conc.iter().map(|(d, v)| (d.clone(), json!(v))).collect();
                    let factors: Vec<Json> = tr
                        .drug_factors
                        .iter()
                        .map(|(d, a, b, g)| json!({ "drug": d, "cl": a, "vc": b, "ec50": g }))
                        .collect();
                    kept.push(json!({ "values": series, "severity": tr.severity,
                        "conc_mg_L": conc, "drug_factors": factors, "death_t_h": death_t,
                        "outcome": states.iter().map(|s| s["outcome"].clone()).collect::<Vec<_>>() }));
                }
            }
            let band = |v: &mut Vec<f64>| {
                v.sort_by(|a, b| a.partial_cmp(b).expect("valeurs finies"));
                let mean = if v.is_empty() {
                    f64::NAN
                } else {
                    v.iter().sum::<f64>() / v.len() as f64
                };
                let q = |x| vetsim_core::variation::percentile(v, x);
                let nan = |x: f64| if x.is_finite() { json!(x) } else { Json::Null };
                (nan(q(0.05)), nan(q(0.25)), nan(q(0.5)), nan(q(0.75)), nan(q(0.95)), nan(mean))
            };
            let mut summary = Map::new();
            for (ki, k) in keys.iter().enumerate() {
                let mut cols: [Vec<Json>; 6] = Default::default();
                for v in values[ki].iter_mut() {
                    let (a, b, c2, d, e2, f) = band(v);
                    for (col, x) in cols.iter_mut().zip([a, b, c2, d, e2, f]) {
                        col.push(x);
                    }
                }
                let [p05, p25, p50, p75, p95, mean] = cols;
                summary.insert(k.clone(), json!({ "p05": p05, "p25": p25, "p50": p50,
                    "p75": p75, "p95": p95, "mean": mean }));
            }
            let severity: Vec<Json> = s
                .patient
                .cases
                .iter()
                .zip(sev_all.iter_mut())
                .map(|(case, per_t)| {
                    let mut cols: [Vec<Json>; 6] = Default::default();
                    for v in per_t.iter_mut() {
                        let (a, b, c2, d, e2, f) = band(v);
                        for (col, x) in cols.iter_mut().zip([a, b, c2, d, e2, f]) {
                            col.push(x);
                        }
                    }
                    let [p05, p25, p50, p75, p95, mean] = cols;
                    json!({ "disease": case.disease, "s0": case.severity, "p05": p05, "p25": p25,
                            "p50": p50, "p75": p75, "p95": p95, "mean": mean })
                })
                .collect();
            let alive_fraction: Vec<f64> = alive.iter().map(|a| *a as f64 / n as f64).collect();
            Ok(json!({ "ok": true, "t": times, "n": n, "seed": seed, "keys": keys,
                "summary": summary, "alive_fraction": alive_fraction, "severity": severity,
                "death_t_h": deaths, "trajectories": kept,
                "scale": { "natural": scale.natural, "disease": scale.disease,
                           "drug": scale.drug, "measurement": scale.measurement },
                "evidence": "D",
                "warning": "Amplitudes de variabilité intra-individuelle non calées (placeholders, preuve D). Usage pédagogique uniquement." }))
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
                .map(|(a, x)| json!({ "t": a.t, "organized": a.organized, "on": x.on, "off": x.off, "ta_end": x.ta_end }))
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
                .map(|i| json!({ "t": i.t, "t_avn": i.t_avn, "fate": i.fate.as_str(), "t_his": i.t_his }))
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
                "intervals": { "pr": q.pr, "qrs": q.qrs, "hv": q.hv, "qt": q.qt, "pa_ia": q.pa_ia },
                "timing": { "emd_v": q.emd_v, "emd_a": q.emd_a, "act_gamma": q.act_gamma, "tr_frac": q.tr_frac },
                "note": "séquence de conduction, ondes de l'ECG, activations, volumes, pressions et débits du modèle ; géométrie schématique ; usage pédagogique uniquement",
            }))
        })
    }

    /// Échocardiographie virtuelle : sorties du cœur animé (`heart`) et géométrie des cavités
    /// tirée des volumes simulés (VG obus à volume myocardique constant, OG et OD en
    /// observateurs, VD en croissant), ouvertures valvulaires suivant les débits, mesures
    /// « vraies » sur coupe parfaite et scène schématique du rendu (`modules/echo.json`).
    pub fn echo(&mut self, patient_id: &str, options: &str) -> String {
        let heart = self.heart(patient_id, options);
        self.respond(|e| {
            use vetsim_core::echo::{doppler_series, doppler_truth, echo_series, echo_truth, EchoInputs, Pressures};
            let cfg = e
                .echo
                .clone()
                .ok_or_else(|| ApiError::one("/echo", "module echo non chargé"))?;
            let mut h: Json = serde_json::from_str(&heart)
                .map_err(|_| ApiError::one("", "réponse heart illisible"))?;
            if h["ok"] != true {
                return Ok(h);
            }
            let o = parse(options, "options")?;
            let m = obj(&o, "")?.clone();
            let sess = e.session(patient_id)?.clone();
            let p = e.drugged_params(&sess, &m)?;
            let arr = |k: &str| -> Vec<f64> {
                h["series"][k]
                    .as_array()
                    .map(|a| a.iter().filter_map(Json::as_f64).collect())
                    .unwrap_or_default()
            };
            let (v_lv, v_rv, v_pv, v_sv) = (arr("v_lv"), arr("v_rv"), arr("v_la"), arr("v_ra"));
            let (q_mi, q_ao, q_tr, q_pu) = (arr("q_mi"), arr("q_ao"), arr("q_tr"), arr("q_pu"));
            let inp = EchoInputs {
                v_lv: &v_lv, v_rv: &v_rv, v_pv: &v_pv, v_sv: &v_sv,
                q_mi: &q_mi, q_ao: &q_ao, q_tr: &q_tr, q_pu: &q_pu,
            };
            let (st, s) = echo_series(&cfg, &p, &inp);
            let fs = h["fs"].as_f64().unwrap_or(100.0);
            let beats: Vec<f64> = h["mechanics"]["beats"]
                .as_array()
                .map(|a| a.iter().filter_map(|b| b["t"].as_f64()).collect())
                .unwrap_or_default();
            let (p_lv, p_la, p_rv, p_ra, p_ao, p_pa) = (arr("p_lv"), arr("p_la"), arr("p_rv"), arr("p_ra"), arr("p_ao"), arr("p_pa"));
            let pr = Pressures { lv: &p_lv, la: &p_la, rv: &p_rv, ra: &p_ra, ao: &p_ao, pa: &p_pa };
            let (dst, ds) = doppler_series(&cfg, &st, &s, &inp, &pr);
            let act_a = arr("act_a");
            let tr = echo_truth(&st, &s, &v_lv, &q_ao, &beats, 1.0 / fs, vetsim_core::math::pow(p.bw, cfg.lvidd_exp));
            let dtr = doppler_truth(&ds, &act_a, &pr, &beats, 1.0 / fs);
            let f32s = |v: &[f64]| v.iter().map(|x| *x as f32 as f64).collect::<Vec<_>>();
            let col = |i: usize, v: &[[f64; 3]]| v.iter().map(|a| a[i] as f32 as f64).collect::<Vec<_>>();
            h["echo"] = json!({
                "bw": p.bw,
                "static": { "ao_d": st.ao_d, "mitral_annulus_d": st.mitral_annulus_d,
                            "leaflet_l": st.leaflet_l, "rv_wall": st.rv_wall,
                            "lv_ref": { "v": st.lv.v_ref, "d": st.lv.d_ref, "l": st.lv.l_ref,
                                        "w": st.lv.w_ref, "v_myo": st.lv.v_myo } },
                "series": {
                    "lv_d": f32s(&s.lv_d), "lv_l": f32s(&s.lv_l), "lv_w": f32s(&s.lv_w),
                    "rv_t": f32s(&s.rv_t),
                    "la_v": f32s(&s.la_v), "la_ax": [col(0, &s.la_axes), col(1, &s.la_axes), col(2, &s.la_axes)],
                    "ra_v": f32s(&s.ra_v), "ra_ax": [col(0, &s.ra_axes), col(1, &s.ra_axes), col(2, &s.ra_axes)],
                    "mi_open": f32s(&s.mi_open), "ao_open": f32s(&s.ao_open),
                    "tr_open": f32s(&s.tr_open), "pu_open": f32s(&s.pu_open),
                },
                "truth": {
                    "n_beats": tr.n_beats, "lvidd": tr.lvidd, "lvids": tr.lvids, "fs": tr.fs,
                    "ef": tr.ef, "wall_d": tr.wall_d, "wall_s": tr.wall_s, "lvidd_n": tr.lvidd_n,
                    "ao": tr.ao, "la_d": tr.la_d, "la_ao": tr.la_ao, "la_max": tr.la_max,
                    "la_min": tr.la_min, "lv_mass_g": tr.lv_mass_g, "instants": tr.instants,
                },
                "doppler": {
                    "static": { "a_ao": dst.a_ao, "a_pu": dst.a_pu, "a_mi_open": dst.a_mi_open, "a_tv_open": dst.a_tv_open },
                    "series": {
                        "v_mi": f32s(&ds.v_mi), "v_ao": f32s(&ds.v_ao), "v_tr": f32s(&ds.v_tr), "v_pu": f32s(&ds.v_pu),
                        "a_mi": f32s(&ds.a_mi), "a_tv": f32s(&ds.a_tv),
                        "v_mr": f32s(&ds.v_mr), "v_trj": f32s(&ds.v_trj), "v_ar": f32s(&ds.v_ar), "v_pr": f32s(&ds.v_pr),
                        "eroa_mr": f32s(&ds.eroa_mr), "eroa_tr": f32s(&ds.eroa_tr),
                    },
                    "truth": {
                        "n_beats": dtr.n_beats, "e": dtr.e, "a": dtr.a, "e_a": dtr.e_a, "ao_peak": dtr.ao_peak,
                        "ao_vti": dtr.ao_vti, "pu_peak": dtr.pu_peak, "mr_peak": dtr.mr_peak, "tr_peak": dtr.tr_peak,
                        "eroa_mr": dtr.eroa_mr, "pasp_est": dtr.pasp_est, "pasp_true": dtr.pasp_true,
                    },
                },
                "geometry": cfg.geometry_json(),
                "render": cfg.render,
                "note": "géométrie tirée des volumes du modèle ; OG et OD observées (sans rétroaction) ; scène et acoustique schématiques (evidence D) ; usage pédagogique uniquement",
            });
            Ok(h)
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
            "echo" => self.echo(&pid, &opts),
            "state" => self.state(&pid, &opts),
            "signals" => self.signals(&pid, &opts),
            "timecourse" => self.timecourse(&pid, &opts),
            "montecarlo" => self.montecarlo(&pid, &opts),
            "renal_course" => self.renal_course(&pid, &opts),
            "add_disease" => self.add_disease(&pid, &opts),
            "new_patient" => self.new_patient(&opts),
            "list_modules" => self.list_modules(),
            "anesthesia_list" => self.respond(|e| e.anesthesia_list()),
            "anesthesia_start" => {
                let o = msg.get("options").cloned().unwrap_or_else(|| json!({}));
                self.respond(|e| e.anesthesia_start(&pid, &o))
            }
            "anesthesia_event" | "anesthesia_step" | "anesthesia_record" => {
                let o = msg.get("options").cloned().unwrap_or_else(|| json!({}));
                self.respond(|e| match op {
                    "anesthesia_event" => e.anesthesia_event(&o),
                    "anesthesia_step" => e.anesthesia_step(&o),
                    _ => e.anesthesia_record(&o),
                })
            }
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
/// Paramètres sous traitement à l'instant `i` d'une évolution temporelle (concentrations
/// plasmatiques et au site d'effet, état de volume).
fn effects_at(
    base: &Params,
    drugs: &[vetsim_core::DrugModule],
    tc: &vetsim_core::Timecourse,
    i: usize,
) -> Result<Params, vetsim_core::Error> {
    let at = |v: &[(String, Vec<f64>)]| -> Vec<(String, f64)> {
        v.iter().map(|(d, x)| (d.clone(), x[i])).collect()
    };
    apply_drug_effects_sites(
        base,
        drugs,
        &at(&tc.conc),
        &at(&tc.conc_e),
        tc.d[i],
        tc.u[i],
    )
}

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
            Some("im") | Some("sc") => Route::Im,
            _ => {
                return Err(ApiError::one(
                    path("route"),
                    "« iv », « po » ou « im » (« sc ») attendu",
                ))
            }
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
                "auscultation": read("auscultation.json"), "variation": read("variation.json"),
                "echo": read("echo.json") })
        .to_string()
    }

    fn echo_truth_of(e: &mut Engine, bw: f64, dis: &str, sev: f64) -> Json {
        let id = j(&e.new_patient(&json!({"bw": bw, "variability": false}).to_string()))
            ["patient_id"]
            .as_str()
            .unwrap()
            .to_string();
        if !dis.is_empty() {
            e.add_disease(&id, &json!({"id": dis, "severity": sev}).to_string());
        }
        let r = j(&e.echo(&id, r#"{"duration_s": 6, "fs": 200, "seed": 3}"#));
        assert_eq!(r["ok"], true, "{r}");
        let t = r["echo"]["truth"].clone();
        println!("{bw} kg {dis}:{sev} → {t}");
        println!("   Doppler → {}", r["echo"]["doppler"]["truth"]);
        t
    }

    /// Échocardiographie, chien sain de 5, 20 et 40 kg : LVIDd dans l'intervalle de prédiction
    /// de Cornell 2004 (1,27–1,85 × BW^0,294), FR de 25 à 45 %, OG/Ao < 1,6 (Rishniw et Erb
    /// 2000), volume maximal de l'OG dans l'IR 95 % de Visser 2019 (0,68–1,62 mL/kg) ;
    /// épaississement systolique de la paroi (volume myocardique conservé).
    #[test]
    fn echo_matches_published_healthy_targets() {
        let mut e = Engine::new();
        assert_eq!(j(&e.load_modules(&bundle()))["loaded"]["echo"], true);
        for bw in [5.0, 20.0, 40.0] {
            let t = echo_truth_of(&mut e, bw, "", 0.0);
            let f = |k: &str| t[k].as_f64().unwrap();
            assert!(t["n_beats"].as_u64().unwrap() >= 3);
            let lo = 1.27 * vetsim_core::math::pow(bw, 0.294);
            let hi = 1.85 * vetsim_core::math::pow(bw, 0.294);
            assert!(
                (lo..hi).contains(&f("lvidd")),
                "{bw} kg : LVIDd {}",
                f("lvidd")
            );
            assert!((0.25..0.45).contains(&f("fs")), "{bw} kg : FR {}", f("fs"));
            assert!(f("la_ao") < 1.6, "{bw} kg : OG/Ao {}", f("la_ao"));
            let la_kg = f("la_max") / bw;
            assert!(
                (0.68..1.62).contains(&la_kg),
                "{bw} kg : OG max {la_kg} mL/kg"
            );
            assert!(f("la_min") < f("la_max"));
            assert!(f("wall_s") > 1.2 * f("wall_d"));
            assert!(f("lvidd_n") < 1.7, "{bw} kg : LVIDdN {}", f("lvidd_n"));
        }
    }

    /// MMVD : l'OG et le VG se dilatent avec la sévérité (OG observée sur la pression du
    /// modèle) ; à s = 0,9 les deux critères échographiques du stade B2 (Keene 2019 :
    /// OG/Ao ≥ 1,6 et LVIDdN ≥ 1,7) sont atteints. FA : plus de vidange active de l'OG.
    #[test]
    fn echo_follows_disease_mechanics() {
        let mut e = Engine::new();
        e.load_modules(&bundle());
        let h = echo_truth_of(&mut e, 20.0, "", 0.0);
        let mut prev = h["la_ao"].as_f64().unwrap();
        for sev in [0.3, 0.6, 0.9] {
            let t = echo_truth_of(&mut e, 20.0, "mmvd", sev);
            let la_ao = t["la_ao"].as_f64().unwrap();
            assert!(la_ao > prev, "OG/Ao croissant avec la sévérité");
            prev = la_ao;
            if sev == 0.9 {
                assert!(la_ao >= 1.6 && t["lvidd_n"].as_f64().unwrap() >= 1.7, "{t}");
            }
        }
        let af = echo_truth_of(&mut e, 20.0, "atrial_fibrillation", 0.8);
        let frac = |t: &Json| 1.0 - t["la_min"].as_f64().unwrap() / t["la_max"].as_f64().unwrap();
        assert!(
            frac(&af) < 0.7 * frac(&h),
            "fraction de vidange de l'OG en FA {} vs {}",
            frac(&af),
            frac(&h)
        );
    }

    /// Doppler tiré du modèle, comparé aux valeurs publiées (chien sain de 20 kg) :
    /// Bagardi et al. 2025 (J Vet Med Sci, PMC11964857 ; IR 95 %) : E 0,43–1,05 m/s, A 0,31–0,86,
    /// E/A 0,8–2,25 ; Favier et al. 2025 (Animals, PMC12153602) : aortique en CW 1,10–2,33 m/s,
    /// pulmonaire 0,59–1,41. MMVD : vitesse du jet d'IM entre 4,5 et 6,5 m/s (médianes
    /// 5,3–5,9 m/s, Koch et al. 2024, Animals, PMC11482544), EROA croissante avec la sévérité et
    /// au-delà du seuil B1→B2 (0,09 cm², Koch 2024) quand OG/Ao ≥ 1,6. FA : pas d'onde A.
    #[test]
    fn echo_doppler_matches_published_values() {
        let mut e = Engine::new();
        e.load_modules(&bundle());
        let dop = |e: &mut Engine, dis: &str, sev: f64| -> (Json, Json) {
            let id = j(&e.new_patient(r#"{"bw": 20, "variability": false}"#))["patient_id"]
                .as_str()
                .unwrap()
                .to_string();
            if !dis.is_empty() {
                e.add_disease(&id, &json!({"id": dis, "severity": sev}).to_string());
            }
            let r = j(&e.echo(&id, r#"{"duration_s": 6, "fs": 200, "seed": 3}"#));
            (
                r["echo"]["doppler"]["truth"].clone(),
                r["echo"]["truth"].clone(),
            )
        };
        let (h, _) = dop(&mut e, "", 0.0);
        let v = |t: &Json, k: &str| t[k].as_f64().unwrap();
        println!("sain : {h}");
        assert!((43.0..105.0).contains(&v(&h, "e")), "E {}", v(&h, "e"));
        assert!((31.0..86.0).contains(&v(&h, "a")), "A {}", v(&h, "a"));
        assert!((0.8..2.25).contains(&v(&h, "e_a")), "E/A {}", v(&h, "e_a"));
        assert!(
            (110.0..233.0).contains(&v(&h, "ao_peak")),
            "Ao {}",
            v(&h, "ao_peak")
        );
        assert!(
            (59.0..141.0).contains(&v(&h, "pu_peak")),
            "P {}",
            v(&h, "pu_peak")
        );
        assert_eq!(v(&h, "mr_peak"), 0.0, "pas de jet d'IM chez le chien sain");
        let mut prev = 0.0;
        for sev in [0.3, 0.6, 0.9] {
            let (d, g) = dop(&mut e, "mmvd", sev);
            println!("MMVD {sev} : {d}");
            assert!(
                (450.0..650.0).contains(&v(&d, "mr_peak")),
                "IM {}",
                v(&d, "mr_peak")
            );
            assert!(v(&d, "eroa_mr") > prev, "EROA croissante");
            prev = v(&d, "eroa_mr");
            if v(&g, "la_ao") >= 1.6 {
                assert!(
                    v(&d, "eroa_mr") > 0.09,
                    "EROA {} avec OG/Ao {}",
                    v(&d, "eroa_mr"),
                    v(&g, "la_ao")
                );
            }
        }
        let (af, _) = dop(&mut e, "atrial_fibrillation", 0.8);
        assert_eq!(v(&af, "a"), 0.0, "FA : pas d'onde A");
        assert!(v(&af, "e") > 0.0);
    }

    /// Monte-Carlo : un même chien MMVD C sous traitement, 20 trajectoires ; les bandes ne sont
    /// pas dégénérées, la médiane reste proche de la courbe déterministe, et le résultat est
    /// reproductible. Sources coupées : bande nulle, égale au déterministe.
    #[test]
    fn montecarlo_bands_around_the_deterministic_course() {
        let mut e = Engine::new();
        assert_eq!(j(&e.load_modules(&bundle()))["loaded"]["variation"], true);
        let id = j(&e.new_patient(r#"{"variability": false}"#))["patient_id"]
            .as_str()
            .unwrap()
            .to_string();
        e.add_disease(&id, r#"{"id": "mmvd", "stage": "C"}"#);
        let doses = json!([{"t_h": 0, "drug": "furosemide", "mg_per_kg": 2.0, "route": "iv"}]);
        let times = json!([0.0, 2.0, 6.0, 12.0, 24.0]);
        let opts = json!({"state_times_h": times, "doses": doses, "n": 20, "seed": 3,
                          "keys": ["hr", "map", "creat"], "trajectories": 2,
                          "scale": {"natural": 1, "disease": 1, "drug": 1, "measurement": 1}});
        let a = j(&e.montecarlo(&id, &opts.to_string()));
        assert_eq!(a["ok"], true, "{a}");
        let b = j(&e.montecarlo(&id, &opts.to_string()));
        assert_eq!(a, b, "reproductible");
        assert_eq!(a["trajectories"].as_array().unwrap().len(), 2);
        let det = j(&e.timecourse(
            &id,
            &json!({"doses": doses, "t_end_h": 24, "t_eval_h": [0], "state_times_h": times})
                .to_string(),
        ));
        for i in 0..5 {
            let hr = &a["summary"]["hr"];
            let (lo, med, hi) = (
                hr["p05"][i].as_f64().unwrap(),
                hr["p50"][i].as_f64().unwrap(),
                hr["p95"][i].as_f64().unwrap(),
            );
            let d = det["states_at"][i]["hr"].as_f64().unwrap();
            assert!(hi > lo, "bande FC dégénérée à l'instant {i}");
            assert!(
                (med / d - 1.0).abs() < 0.1,
                "médiane {med} vs déterministe {d}"
            );
        }
        // Défaut (tirage unique) sans médicament : aucun aléa au fil du temps, la trajectoire est
        // celle du chien, identique d'une réalisation à l'autre.
        let once = j(&e.montecarlo(
            &id,
            &json!({"state_times_h": times, "n": 3, "seed": 1, "keys": ["map"]}).to_string(),
        ));
        for i in 0..5 {
            assert_eq!(
                once["summary"]["map"]["p05"][i],
                once["summary"]["map"]["p95"][i]
            );
        }
        let off = json!({"state_times_h": times, "doses": doses, "n": 3, "seed": 3,
            "keys": ["map"], "scale": {"natural": 0, "disease": 0, "drug": 0, "measurement": 0}});
        let z = j(&e.montecarlo(&id, &off.to_string()));
        for i in 0..5 {
            let m = &z["summary"]["map"];
            let d = det["states_at"][i]["map"].as_f64().unwrap();
            assert_eq!(m["p05"][i], m["p95"][i]);
            assert!((m["p50"][i].as_f64().unwrap() - d).abs() < 1e-9);
        }
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
        // les battements sonores suivent le début mécanique (début du QRS + délai
        // électromécanique) : au pic R à 1 ms près pour un QRS fin
        assert!(
            !beats.is_empty()
                && beats.len() <= rt.len()
                && beats.iter().zip(&rt).all(|(b, r)| (b - r).abs() < 1e-3),
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
        assert_eq!(l["blocks"].as_array().unwrap().len(), 19);
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
