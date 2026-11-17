//! vetsim-core : moteur de patient virtuel vétérinaire (chien).
//!
//! Noyau sans I/O : l'appelant (CLI, FFI) lit les fichiers et passe des chaînes JSON.
//! Le moteur ne connaît aucune maladie ni aucun médicament : il lit des modules JSON
//! (paramètres et formules du DSL), applique leurs effets aux paramètres, puis simule.
//!
//! Usage pédagogique uniquement ; aucun résultat ne sert à décider d'un traitement réel.

pub mod calibrate;
pub mod case;
pub mod dsl;
pub mod engine;
pub mod error;
pub mod gas;
pub mod hemo;
pub mod math;
pub mod modules;
pub mod ode;
pub mod params;
pub mod patient;
pub mod pkpd;
pub mod rng;
pub mod roots;

pub use case::{apply_modifiers, evaluate_effects, Case, DrawSource, Modifier, NoNoise};
pub use engine::{evaluate, metabolic_state, Evaluation, HemoInputs, MetabolicState};
pub use error::{Error, EvalError, Issue, LoadError};
pub use modules::{Catalog, DiseaseModule, DrugModule, PatientModule};
pub use params::Params;
pub use patient::{healthy_params, Patient};
pub use pkpd::{apply_drug_effects, timecourse, Dose, Route, Timecourse};
pub use rng::SimRng;

/// Version du contrat d'API (CLI et messages JSON).
pub const API_VERSION: &str = "1.0.0";
