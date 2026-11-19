# vetsim-kit : lot de départ pour réaliser le moteur Rust + interface KMP

Contenu
- `PROMPT.md` : le prompt à donner à l'IA.
- `CLAUDE.md` : règles de projet lues automatiquement par Claude Code.
- `docs/` : architecture, spécification du modèle, DSL, contrat du CLI, API KMP, feuille de route, provenance des données.
- `schema/` : JSON Schema (expressions, maladies, médicaments, patient).
- `modules/` : patient sain, 4 maladies (MMVD, MRC, Addison, pneumonie), 5 médicaments ; `modules/anesthesia/` : module d'anesthésie en temps réel (12 agents, 2 halogénés, constantes, 2 scénarios chirurgicaux ; `docs/ANESTHESIE.tex`).
- `crates/` : moteur Rust (`vetsim-core`, référence normative), CLI, API, FFI, wasm.
- `reference/python/` : implémentation secondaire (contre-vérification indépendante) et scripts de calage.
- `tests/` : validation des schémas, équivalence JSON ↔ Python, tests golden et runner de conformité.
- `calibration/` : cibles publiées et résultats du banc de calibration Python.

Vérifications : d'abord le moteur Rust (normatif), puis les contre-vérifications Python (secondaires ; Python 3, numpy, scipy, jsonschema)
```
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
python tests/validate_modules.py      # schémas
python tests/check_modules.py         # JSON == Python (patient 5/20/40 kg, 4 maladies)
python tests/conformance.py --cmd "./target/release/vetsim-cli"   # le Rust contre golden (962 valeurs)
python tests/conformance.py           # contre-vérification : le Python secondaire contre golden
```

Avertissement : outil pédagogique. Paramètres partiellement calés sur des moyennes canines publiées (voir `docs/PROVENANCE.md`), pas validés cliniquement. Ne jamais l'utiliser pour décider d'un traitement.
