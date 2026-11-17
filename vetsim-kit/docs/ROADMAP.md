# Feuille de route : jalons et critères d'acceptation

Chaque jalon se termine par des commandes de vérification. Ne pas passer au suivant sans les avoir exécutées.

| Jalon | Contenu | Critère d'acceptation |
|---|---|---|
| **M0** | Workspace Cargo (`vetsim-core`, `vetsim-cli`), CI, lint | `cargo test`, `clippy -D warnings`, `fmt --check` verts |
| **M1** | DSL, chargement + validation des modules (schémas), patient sain, effets maladie sans bruit | Tests Rust équivalents à `tests/check_modules.py` : patient 5/20/40 kg et 4 maladies à 1e-9 |
| **M2** | Acide-base de Stewart, gaz, Hill/Bohr, rein, lactate (en algébrique, avec `co` et `la_mean` d'entrée) | Sur le golden, sorties gaz et chimie conformes quand on injecte l'hémodynamique de référence |
| **M3** | Hémodynamique ODE (RK45 adaptatif) + boucle baroréflexe | `conformance.py` : cas `healthy_*` et `mmvd_*`, `ckd_*`, `addison_*` 100 % |
| **M4** | PK/PD, médicaments, états avec concentrations, diurèse | `conformance.py` : tous les cas `pk_*`, `*_pimobendan`, `*_atropine`, `*_betabloquant`, `*_opioide`, `*_furosemide_volume` |
| **M5** | Population virtuelle + banc de calibration en Rust | `calibrate` Rust reproduit le tableau `calibration/calibration_run_python.json` (écarts moyens < 5 %) |
| **M6** | Autonome, VFC, ECG, PCG | Cibles `hrv` dans ±1 ET (≥ 20 graines); propriétés qualitatives ECG/PCG |
| **M7** | `vetsim-ffi` + `kmp-demo` (Android + desktop), option wasm | La démo affiche ECG, pressions et gaz du sang pour un patient MMVD stade C |
| **M8** | Nouveaux modules (diabète, bloc AV, fibrillation atriale, sepsis) via hooks | Chacun avec `targets` publiées, schéma valide, test de conformité |

## Après M8 : idées
- Inversion probabiliste (ABC/MCMC) : à partir de gaz et de biochimie, estimer maladie et sévérité (diagnostic différentiel pondéré).
- Format d'échange versionné et API pour Moodle (`local_simhub`), le jeu de clinique, le jeu de cartes et VetNutri.
- Éditeur de modules (formulaire qui produit le JSON validé).
