//! vetsim-core : moteur de patient virtuel vétérinaire (chien).
//!
//! Noyau sans I/O : l'appelant (CLI, FFI) lit les fichiers et passe des chaînes JSON.
//! Usage pédagogique uniquement ; aucun résultat ne sert à décider d'un traitement réel.

/// Version du contrat d'API (CLI et messages JSON).
pub const API_VERSION: &str = "1.0.0";

#[cfg(test)]
mod tests {
    #[test]
    fn api_version_is_semver() {
        assert_eq!(super::API_VERSION.split('.').count(), 3);
    }
}
