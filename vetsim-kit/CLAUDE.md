# Instructions de projet (vetsim)

- La bibliographie vérifiée prime (décision du 17/11/2026). Le Python de `reference/python/` est historique : il reste la base du golden et des fixtures (`tests/conformance.py`) tant qu'il est tenu à jour, mais une correction fondée sur une source peut s'en écarter ; déclarer alors l'écart Rust/Python.
- Ne modifie jamais `modules/`, `calibration/`, `tests/golden/` ni les valeurs de paramètres pour faire passer un test : seulement pour suivre une source, avec une entrée au journal.
- Aucune valeur clinique ou pharmacologique inventée. Placeholder = `evidence: "D"`.
- Formules des modules : uniquement via le DSL (`docs/DSL.md`), jamais d'`eval`.
- Déterminisme : graine explicite; le CLI `eval` est sans bruit.
- Commandes de vérification : `python tests/validate_modules.py`, `python tests/check_modules.py`, `python tests/conformance.py [--cmd ...]`, `cargo test`, `cargo clippy -- -D warnings`.
- Langue : réponses et commentaires physiologiques en français, identifiants en anglais.
- Usage pédagogique uniquement : aucun résultat ne sert à décider d'un traitement réel.
