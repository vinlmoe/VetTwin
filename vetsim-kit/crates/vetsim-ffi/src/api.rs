//! API par messages JSON (docs/KMP.md), en Rust pur : testable sans Kotlin.
//!
//! Chaque méthode reçoit et renvoie une chaîne JSON. Une entrée invalide renvoie
//! `{"ok": false, "errors": [{"path": ..., "message": ...}]}` ; aucune méthode ne panique
//! volontairement (la couche FFI ajoute un `catch_unwind` en dernier recours).
//!
//! Usage pédagogique uniquement : aucun résultat ne sert à décider d'un traitement réel.

use serde_json::{json, Map, Value as Json};
use std::collections::BTreeMap;
use vetsim_core::case::{Case, DrawSource, NoNoise};
use vetsim_core::pkpd::{apply_drug_effects, timecourse, Dose, Route};
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
        if !errors.is_empty() {
            return Err(ApiError(errors));
        }
        let catalog = Catalog::new(patient.expect("contrôlé"), diseases, drugs)
            .map_err(|e| ApiError::one("", e.to_string()))?;
        let counts = json!({ "diseases": catalog.diseases.len(), "drugs": catalog.drugs.len() });
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
                            "stages": stages, "refs": d.refs })
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
                            "pd_type": kind, "ec50_mg_L": d.pd.ec50_mg_l, "refs": d.refs })
                })
                .collect();
            Ok(json!({ "ok": true, "patient": { "id": c.patient.id, "version": c.patient.version },
                       "diseases": diseases, "drugs": drugs,
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
        let d = opt_f64(m, "D", 0.0)?;
        let u = opt_f64(m, "U", 0.0)?;
        if conc.is_empty() && d == 0.0 && u == 0.0 {
            return Ok(s.patient.params.clone());
        }
        Ok(apply_drug_effects(
            &s.patient.params,
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
            let ev = evaluate(&p)?;
            let mut out = Map::new();
            out.insert("ok".into(), Json::Bool(true));
            for (k, v) in ev.to_flat() {
                out.insert(k.into(), Json::from(v));
            }
            out.insert(
                "case".into(),
                Json::Array(s.patient.cases.iter().map(case_json).collect()),
            );
            out.insert("emergent_signs".into(), Json::Array(emergent_signs(&ev)));
            if with_signals {
                out.insert("signals".into(), signals_json(&ev, &m, s.seed)?);
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
            let ev = evaluate(&p)?;
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
            let list = m
                .get("doses")
                .and_then(Json::as_array)
                .ok_or_else(|| ApiError::one("/doses", "tableau de doses requis"))?;
            let mut ds = Vec::new();
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
            let mut out = json!({ "ok": true, "t": tc.t, "conc_mg_L": conc, "D": tc.d, "U": tc.u });
            if with_states {
                let mut states = Vec::with_capacity(tc.t.len());
                for i in 0..tc.t.len() {
                    let conc_i: Vec<(String, f64)> =
                        tc.conc.iter().map(|(d, v)| (d.clone(), v[i])).collect();
                    let p =
                        apply_drug_effects(&s.patient.params, &c.drugs, &conc_i, tc.d[i], tc.u[i])?;
                    let ev = evaluate(&p)?;
                    let mut st = Map::new();
                    for (k, v) in ev.to_flat() {
                        st.insert(k.into(), Json::from(v));
                    }
                    states.push(Json::Object(st));
                }
                out["states"] = Json::Array(states);
            }
            Ok(out)
        })
    }

    fn respond(&mut self, f: impl FnOnce(&mut Self) -> ApiResult) -> String {
        match f(self) {
            Ok(v) => v.to_string(),
            Err(e) => e.to_json(),
        }
    }
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
    json!({ "disease": c.disease, "severity": c.severity, "stage": c.stage, "flags": flags, "draws": draws })
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
    Ok(json!({
        "ecg": f32s(&s.ecg.v), "ecg_fs": fs, "r_times": s.ecg.r_times,
        "ecg_features": { "pr": f.pr, "qrs": f.qrs, "qt": f.qt, "p_amp": f.p_amp, "t_amp": f.t_amp, "r_amp": f.r_amp },
        "pcg": f32s(&s.pcg.v), "pcg_fs": PCG_FS,
        "pcg_features": { "s1": pf.s1, "s2": pf.s2, "s3": pf.s3, "murmur": pf.murmur.label_fr(),
                          "grade": pf.grade, "murmur_peak_flow": pf.murmur_peak_flow },
        "pressures": { "dt": w.t.get(1).copied().unwrap_or(0.002), "period": ev.hemo.timing.period,
                       "ao": f32s(&w.p_ao), "lv": f32s(&w.p_lv), "la": f32s(&w.p_pv),
                       "pa": f32s(&w.p_pa), "rv": f32s(&w.p_rv) },
        "hrv": hrv,
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
        let ds: Vec<Json> = ["mmvd", "ckd", "addison", "pneumonia"]
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
        json!({ "patient": read("healthy_dog.json"), "diseases": ds, "drugs": dr }).to_string()
    }

    fn j(s: &str) -> Json {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn full_session_matches_cli_contract() {
        let mut e = Engine::new();
        assert_eq!(e.api_version(), "1.0.0");
        assert_eq!(j(&e.load_modules(&bundle()))["ok"], true);
        let l = j(&e.list_modules());
        assert_eq!(l["diseases"].as_array().unwrap().len(), 4);
        let p = j(&e.new_patient(r#"{"bw": 20, "variability": false}"#));
        let id = p["patient_id"].as_str().unwrap().to_string();
        let c = j(&e.add_disease(&id, r#"{"id": "mmvd", "severity": 0.8}"#));
        assert_eq!(c["case"]["stage"], "C");
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
        let signs: Vec<&str> = st["emergent_signs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert!(
            signs.contains(&"murmur") && signs.contains(&"pulmonary_edema_risk"),
            "{signs:?}"
        );
        let sg = j(&e.signals(&id, r#"{"duration_s": 4}"#));
        assert_eq!(sg["ok"], true);
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
