# Instructions de projet (vetsim)

- Le Python de `reference/python/` est la référence normative; le Rust doit s'y conformer (`tests/conformance.py`).
- Ne modifie jamais `modules/`, `calibration/`, `tests/golden/` ni les valeurs de paramètres pour faire passer un test. Signale l'écart.
- Aucune valeur clinique ou pharmacologique inventée. Placeholder = `evidence: "D"`.
- Formules des modules : uniquement via le DSL (`docs/DSL.md`), jamais d'`eval`.
- Déterminisme : graine explicite; le CLI `eval` est sans bruit.
- Commandes de vérification : `python tests/validate_modules.py`, `python tests/check_modules.py`, `python tests/conformance.py [--cmd ...]`, `cargo test`, `cargo clippy -- -D warnings`.
- Langue : réponses et commentaires physiologiques en français, identifiants en anglais.
- Usage pédagogique uniquement : aucun résultat ne sert à décider d'un traitement réel.
