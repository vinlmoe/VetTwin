//! vetsim-wasm : moteur vetsim dans le navigateur (WebAssembly, `wasm-bindgen`).
//!
//! Même API par messages JSON que la liaison UniFFI (`vetsim-api`, docs/KMP.md) : chaînes
//! JSON en entrée et en sortie, erreurs `{"ok": false, "errors": [...]}`. Le calcul se fait
//! entièrement dans le navigateur (aucun serveur, aucune donnée envoyée).
//!
//! Panique : sur wasm32 une panique interrompt le module (pas de déroulement). L'API est
//! écrite pour ne pas paniquer sur une entrée invalide (erreurs structurées, testées).

use wasm_bindgen::prelude::*;

/// Moteur exposé à JavaScript.
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct Engine {
    inner: vetsim_api::Engine,
}

#[wasm_bindgen]
impl Engine {
    /// Nouveau moteur vide.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Version de l'API.
    #[wasm_bindgen(js_name = apiVersion)]
    pub fn api_version(&self) -> String {
        self.inner.api_version()
    }

    /// Charge les modules (`{"patient", "diseases", "drugs", "auscultation"}`).
    #[wasm_bindgen(js_name = loadModules)]
    pub fn load_modules(&mut self, json: &str) -> String {
        self.inner.load_modules(json)
    }

    /// Catalogue des modules.
    #[wasm_bindgen(js_name = listModules)]
    pub fn list_modules(&mut self) -> String {
        self.inner.list_modules()
    }

    /// Nouveau patient.
    #[wasm_bindgen(js_name = newPatient)]
    pub fn new_patient(&mut self, options: &str) -> String {
        self.inner.new_patient(options)
    }

    /// Ajoute une maladie.
    #[wasm_bindgen(js_name = addDisease)]
    pub fn add_disease(&mut self, patient_id: &str, options: &str) -> String {
        self.inner.add_disease(patient_id, options)
    }

    /// État du patient.
    pub fn state(&mut self, patient_id: &str, options: &str) -> String {
        self.inner.state(patient_id, options)
    }

    /// Signaux (ECG, PCG simple, pressions, respiration).
    pub fn signals(&mut self, patient_id: &str, options: &str) -> String {
        self.inner.signals(patient_id, options)
    }

    /// Cinétique après un schéma d'administration.
    pub fn timecourse(&mut self, patient_id: &str, doses: &str) -> String {
        self.inner.timecourse(patient_id, doses)
    }

    /// Trajectoires Monte-Carlo d'un même animal (variabilité intra-individuelle).
    pub fn montecarlo(&mut self, patient_id: &str, options: &str) -> String {
        self.inner.montecarlo(patient_id, options)
    }

    /// Auscultation (WAV en base64, ECG et pressions alignés en option).
    pub fn pcg(&mut self, patient_id: &str, options: &str) -> String {
        self.inner.pcg(patient_id, options)
    }

    /// Message générique `{"op", "patient", "options"}`.
    pub fn message(&mut self, json: &str) -> String {
        self.inner.message(json)
    }
}
