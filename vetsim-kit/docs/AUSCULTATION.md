# Module auscultation (synthèse des bruits du cœur)

**Usage pédagogique uniquement.**

- Code : `crates/vetsim-core/src/pcg.rs` (`synthesize_pcg`, `patient_pcg`), port fidèle de `reference/python/signals_pcg_v5.py` (référence normative).
- Paramètres : `modules/auscultation.json`. Chaque groupe porte `source`, `evidence` et `calibre_sur`. Schéma : `schema/auscultation.schema.json`. Aucune constante numérique dans le code.
- Entrées : un battement simulé (`PcgInputs` : séries, période, durée de contraction, POG) et les instants de battement du module autonome. Avec la même graine et la même durée, ces instants sont ceux de l'ECG.
- Sortie : signal normalisé (crête à 1), WAV PCM 16 bits mono, grade de souffle indicatif, type de souffle, B3.
- Sites : `base` (B2 > B1, calé) et `apex` (B1 > B2, placeholder evidence D, à calibrer).
- Vérifications :
  - `cargo test --test auscultation` : rejeu des tirages NumPy, identité avec la référence à 4e-15 ;
  - `cargo test --test auscultation_properties` : propriétés du prompt ;
  - `python tests/pcg_acceptance.py --cmd target/release/vetsim-cli --ref-json tests/pcg_reference_python.json` : critères mesurés par `pcg_metrics.py`. Le rapport est dans `docs/PCG_CONFORMITE.md`.

## Limites de validité
- Calage sur **deux** enregistrements de la bibliothèque TiHo (Hanovre) : un rythme normal et un souffle mitral II/VI. Licence CC BY-NC-ND 4.0 : aucun enregistrement n'est copié, redistribué ni embarqué ; seuls les chiffres mesurés sont utilisés.
- Le souffle réel est plus grave et moins « plateau » que le modèle.
- B1→B2 est trop long : 0,27 s à FC 90 et 0,21 s selon le prompt, contre 0,18 s mesuré. La cause est la durée de contraction du modèle hémodynamique.
- Le garde-fou `t_ac ≥ t_mc + 0,7 ts` compense l'éjection trop courte de l'hémodynamique en régurgitation mitrale sévère. C'est une rustine documentée ; l'hémodynamique n'est pas corrigée dans le module audio.
- Les seuils de grade (10, 25, 50, 80, 120 mL/s) ne sont pas calibrés.
- Le préréglage de FC « repos » à 72/min est un réglage d'usage ; la valeur calibrée du chien sain est 87/min.
