# Instructions de projet (vetsim)

- Le moteur Rust (`crates/vetsim-core`) est la référence normative (décision du 18/11/2026). Le Python de `reference/python/` est une référence **secondaire** : contre-vérification indépendante (fixtures, `tests/conformance.py`, calibration). Un écart Rust/Python se signale et s'analyse ; une fois tranché, c'est le Python qui est mis à jour (ou l'écart documenté), jamais le Rust aligné d'office.
- Toute nouveauté du modèle s'écrit d'abord en Rust, avec ses tests ; le portage Python suit quand la contre-vérification est utile.
- Ne modifie jamais `modules/`, `calibration/`, `tests/golden/` ni les valeurs de paramètres pour faire passer un test. Signale l'écart.
- Aucune valeur clinique ou pharmacologique inventée. Placeholder = `evidence: "D"`.
- Formules des modules : uniquement via le DSL (`docs/DSL.md`), jamais d'`eval`.
- Déterminisme : graine explicite; le CLI `eval` est sans bruit.
- Commandes de vérification (dans cet ordre) : `cargo test`, `cargo clippy -- -D warnings`, `python tests/validate_modules.py` ; contre-vérifications secondaires : `python tests/check_modules.py`, `python tests/conformance.py --cmd target/release/vetsim-cli`.
- Langue : réponses et commentaires physiologiques en français, identifiants en anglais.
- Usage pédagogique uniquement : aucun résultat ne sert à décider d'un traitement réel.
