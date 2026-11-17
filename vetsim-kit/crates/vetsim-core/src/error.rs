//! Erreurs typées du noyau.
//!
//! Trois familles :
//! - [`Issue`] : un problème de validation localisé par un chemin JSON (pointeur RFC 6901) ;
//! - [`LoadError`] : refus d'un module au chargement (JSON illisible, schéma ou sémantique) ;
//! - [`EvalError`] / [`Error`] : erreurs d'évaluation d'une formule ou d'une requête au moteur.

use std::fmt;

/// Problème de validation localisé : `path` est un pointeur JSON (`/effects/2/op`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Pointeur JSON vers l'élément fautif (`""` = racine du document).
    pub path: String,
    /// Raison lisible.
    pub message: String,
}

impl Issue {
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = if self.path.is_empty() {
            "/"
        } else {
            &self.path
        };
        write!(f, "{path} : {}", self.message)
    }
}

fn join_issues(issues: &[Issue]) -> String {
    issues
        .iter()
        .map(|i| format!("  - {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Refus d'un module au chargement.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// Le texte n'est pas du JSON valide.
    #[error("{origin} : JSON illisible ({source})")]
    Json {
        /// Origine du module (nom de fichier ou étiquette fournie par l'appelant).
        origin: String,
        /// Erreur du parseur.
        #[source]
        source: serde_json::Error,
    },
    /// Le module viole le schéma ou une règle sémantique (liste complète des problèmes).
    #[error("{origin} : module invalide\n{}", join_issues(.issues))]
    Invalid {
        /// Origine du module.
        origin: String,
        /// Tous les problèmes détectés (pas seulement le premier).
        issues: Vec<Issue>,
    },
    /// Deux modules du même type portent le même identifiant.
    #[error("module {kind} « {id} » défini deux fois")]
    Duplicate {
        /// `disease` ou `drug`.
        kind: &'static str,
        /// Identifiant dupliqué.
        id: String,
    },
}

/// Erreur d'évaluation d'une expression du DSL (jamais de `NaN` silencieux).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EvalError {
    /// Variable absente de l'environnement.
    #[error("variable inconnue « {0} »")]
    UnknownVariable(String),
    /// Division par zéro (`div`, ou `pow` d'une base nulle à exposant négatif).
    #[error("division par zéro dans `{0}`")]
    DivisionByZero(&'static str),
    /// `log` d'un nombre ≤ 0.
    #[error("log d'un nombre ≤ 0 ({0})")]
    LogDomain(f64),
    /// Résultat infini ou NaN (dépassement, puissance d'une base négative…).
    #[error("résultat non fini dans `{0}`")]
    NonFinite(&'static str),
}

/// Erreur d'une requête au moteur (patient, maladie, médicament).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    /// Poids corporel non fini ou ≤ 0.
    #[error("poids corporel invalide : {0} kg (doit être fini et > 0)")]
    InvalidBodyWeight(f64),
    /// Sévérité hors de [0, 1].
    #[error("sévérité invalide : {0} (doit être dans [0, 1])")]
    InvalidSeverity(f64),
    /// Aucun module maladie de cet identifiant.
    #[error("maladie inconnue « {0} »")]
    UnknownDisease(String),
    /// Aucun module médicament de cet identifiant.
    #[error("médicament inconnu « {0} »")]
    UnknownDrug(String),
    /// Probabilité d'un tirage de Bernoulli hors de [0, 1].
    #[error("{module} : tirage « {draw} » : probabilité {p} hors de [0, 1]")]
    InvalidProbability {
        /// Module concerné.
        module: String,
        /// Nom du tirage.
        draw: String,
        /// Valeur obtenue.
        p: f64,
    },
    /// Entrée du modèle invalide (valeur non finie, débit nul…).
    #[error("entrée invalide : {0}")]
    InvalidInput(String),
    /// Échec d'une recherche de racine (équilibre de Stewart, ventilation, inversion de la courbe d'O2).
    #[error("{what} : pas de solution dans [{lo}, {hi}] ({cause:?})")]
    Root {
        /// Équation concernée.
        what: &'static str,
        /// Borne basse.
        lo: f64,
        /// Borne haute.
        hi: f64,
        /// Cause.
        cause: crate::roots::RootError,
    },
    /// Une formule d'un module a échoué à l'évaluation.
    #[error("{module} : {location} : {source}")]
    Eval {
        /// Module concerné.
        module: String,
        /// Emplacement de la formule (pointeur JSON).
        location: String,
        /// Cause.
        #[source]
        source: EvalError,
    },
}
