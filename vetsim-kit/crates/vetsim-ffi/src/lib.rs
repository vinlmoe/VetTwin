//! vetsim-ffi : liaison UniFFI de l'API par messages JSON (docs/KMP.md).
//!
//! Peu de fonctions, des chaînes JSON en entrée et en sortie ; pas d'état global (le moteur
//! est un objet). Aucune panique ne traverse la FFI : chaque appel est protégé par
//! `catch_unwind` et une panique devient `{"ok": false, "errors": [...]}`.
//!
//! `unsafe` : aucun dans ce crate ; le code `extern "C"` est généré par UniFFI.

pub use vetsim_api as api;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

uniffi::setup_scaffolding!();

/// Moteur exposé à Kotlin/Swift.
#[derive(Debug, uniffi::Object)]
pub struct Engine {
    inner: Mutex<api::Engine>,
}

fn guarded(f: impl FnOnce() -> String) -> String {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        api::ApiError(vec![(
            String::new(),
            "erreur interne du moteur (panique interceptée)".into(),
        )])
        .to_json()
    })
}

impl Engine {
    fn with(&self, f: impl FnOnce(&mut api::Engine) -> String) -> String {
        guarded(|| match self.inner.lock() {
            Ok(mut e) => f(&mut e),
            Err(poisoned) => {
                // Une panique précédente a empoisonné le verrou : on repart d'un moteur vide.
                let mut e = poisoned.into_inner();
                *e = api::Engine::new();
                api::ApiError(vec![(
                    String::new(),
                    "moteur réinitialisé après une erreur interne".into(),
                )])
                .to_json()
            }
        })
    }
}

#[uniffi::export]
impl Engine {
    /// Nouveau moteur vide.
    #[uniffi::constructor]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(api::Engine::new()),
        }
    }

    /// Version de l'API (`1.0.0`).
    pub fn api_version(&self) -> String {
        api::API_VERSION.to_string()
    }

    /// Charge les modules (`{"patient": ..., "diseases": [...], "drugs": [...]}`).
    pub fn load_modules(&self, json: String) -> String {
        self.with(|e| e.load_modules(&json))
    }

    /// Nouveau patient.
    pub fn new_patient(&self, options_json: String) -> String {
        self.with(|e| e.new_patient(&options_json))
    }

    /// Ajoute une maladie.
    pub fn add_disease(&self, patient_id: String, options_json: String) -> String {
        self.with(|e| e.add_disease(&patient_id, &options_json))
    }

    /// État du patient.
    pub fn state(&self, patient_id: String, options_json: String) -> String {
        self.with(|e| e.state(&patient_id, &options_json))
    }

    /// Signaux (ECG, PCG, instants R, pressions).
    pub fn signals(&self, patient_id: String, options_json: String) -> String {
        self.with(|e| e.signals(&patient_id, &options_json))
    }

    /// Évolution temporelle après des doses.
    pub fn timecourse(&self, patient_id: String, doses_json: String) -> String {
        self.with(|e| e.timecourse(&patient_id, &doses_json))
    }

    /// Trajectoires Monte-Carlo d'un même animal (variabilité intra-individuelle).
    pub fn montecarlo(&self, patient_id: String, options_json: String) -> String {
        self.with(|e| e.montecarlo(&patient_id, &options_json))
    }

    /// Auscultation (WAV en base64, grade, souffle, B3 ; ECG et pressions alignés en option).
    pub fn pcg(&self, patient_id: String, options_json: String) -> String {
        self.with(|e| e.pcg(&patient_id, &options_json))
    }

    /// Message JSON générique `{"op": "pcg", "patient": ..., "options": ...}`.
    pub fn message(&self, json: String) -> String {
        self.with(|e| e.message(&json))
    }

    /// Catalogue des modules.
    pub fn list_modules(&self) -> String {
        self.with(|e| e.list_modules())
    }
}
