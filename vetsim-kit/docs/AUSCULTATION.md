# Module auscultation (synthèse des bruits du cœur)

**Usage pédagogique uniquement.**

- Code : `crates/vetsim-core/src/pcg.rs` (`synthesize_pcg`, `patient_pcg`), port fidèle de `reference/python/signals_pcg_v5.py` (contre-vérification secondaire).
- Paramètres : `modules/auscultation.json`. Chaque groupe porte `source`, `evidence` et `calibre_sur`. Schéma : `schema/auscultation.schema.json`. Aucune constante numérique dans le code.
- Entrées : un battement simulé (`PcgInputs` : séries, période, durée de contraction, POG) et les instants de battement du module autonome. Avec la même graine et la même durée, ces instants sont ceux de l'ECG.
- Sortie : signal normalisé (crête à 1), WAV PCM 16 bits mono, grade de souffle indicatif, type de souffle, B3.
- Sites : `base` (B2 > B1, calé sur un enregistrement) et `apex` (B1 > B2, calé sur cinq enregistrements de régurgitation mitrale ; le souffle est calé à ce site).
- Vérifications :
  - `cargo test --test auscultation` : rejeu des tirages NumPy, identité avec la référence à 4e-15 ;
  - `cargo test --test auscultation_properties` : propriétés du prompt ;
  - `python tests/pcg_acceptance.py --cmd target/release/vetsim-cli --seeds 300 --ref-json tests/pcg_reference_python.json` : critères mesurés par `pcg_metrics.py`. Le rapport est dans `docs/PCG_CONFORMITE.md`.

## Comparaison avec de vrais enregistrements
`python tests/pcg_compare_real.py --real DOSSIER --cmd target/release/vetsim-cli [--args "--disease mmvd:0.3"]` mesure les enregistrements réels de `DOSSIER` et la synthèse par la même chaîne (mono, 4000 Hz, crête à 1), puis affiche les écarts : spectre et pente, B1→B2, rapport B2/B1, durée des bruits, variabilité d'amplitude, niveau et timbre du bruit de fond, niveau, forme et timbre de l'intervalle systolique (souffle). `DOSSIER` reste hors du dépôt (licence TiHo) ; seuls les chiffres en sortent. Le script mesure ; il ne modifie aucun paramètre.
- Calage : `python tests/pcg_calibration_real.py --real DOSSIER --cmd target/release/vetsim-cli` reproduit les tableaux de `docs/VERIFICATION_PCG.md` (bibliothèque TiHo, 17/11/2026). La version 1.1.0 de `modules/auscultation.json` en découle : B1 en deux composantes (durée mesurée), bruit de fond sans aigus, souffle mitral au niveau mesuré pour chaque grade, site apex calé.

## Limites de validité
- Calage sur la bibliothèque TiHo (Hanovre) : trois enregistrements sans souffle et cinq de régurgitation mitrale, **un chien par grade** (`docs/VERIFICATION_PCG.md`). Licence CC BY-NC-ND 4.0 : aucun enregistrement n'est copié, redistribué ni embarqué ; seuls les chiffres mesurés sont utilisés.
- Le site « base » reprend un enregistrement que la bibliothèque décrit comme pris à l'apex gauche : attribution à revoir. Aucun enregistrement de souffle à la base.
- B1→B2 est trop long : 0,26 s à FC 90, contre 0,18 s mesuré (médiane de 20 enregistrements). C'est l'écart le plus audible qui reste. La cause est la durée de contraction du modèle hémodynamique.
- Le garde-fou `t_ac ≥ t_mc + 0,7 ts` compense l'éjection trop courte de l'hémodynamique en régurgitation mitrale sévère. C'est une rustine documentée ; l'hémodynamique n'est pas corrigée dans le module audio.
- Les seuils de grade (10, 25, 50, 80, 120 mL/s) ne sont pas calibrés.
- Le préréglage de FC « repos » à 72/min est un réglage d'usage ; la valeur calibrée du chien sain est 87/min.
