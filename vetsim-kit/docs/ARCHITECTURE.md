# Architecture

```
modules/*.json ──► vetsim-core (Rust) ◄── vetsim-cli (tests de conformité)
 (schema/*.json)        │
                        ├── vetsim-ffi (UniFFI, API par messages JSON)
                        │        └── kmp-demo (Kotlin Multiplatform, Compose)
                        └── (option) vetsim-server (axum) pour Moodle / jeux en ligne
```

## Principes
1. **Le moteur ne connaît aucune maladie ni aucun médicament.** Il lit des modules JSON, tire une sévérité, applique des modificateurs de paramètres, simule. Les signes cliniques émergent des couplages.
2. **JSON déclaratif pour les paramètres, code pour les équations.** Un nouveau mécanisme (bloc AV, fibrillation atriale, diabète) passe par un *hook* Rust nommé et référencé par le module (`"engine_hook"`), jamais par un langage dans le JSON.
3. **Déterminisme et reproductibilité** : graine explicite, mêmes sorties sur toutes les plateformes.
4. **Noyau sans I/O** : compatible Android, iOS, wasm32. Le chargement des fichiers est fait par l'appelant (le CLI ou KMP) qui passe des chaînes JSON au noyau.
5. **Provenance obligatoire** : chaque module porte `evidence` (A à D), `refs` et des `targets` de validation rejouées par les tests.

## Modules du noyau (miroir des fichiers Python)
| Rust | Python de référence | Rôle |
|---|---|---|
| `dsl` | `tests/dsl_ref.py` | interpréteur d'expressions |
| `modules` | `diseases.py`, `pkpd.py` | chargement, validation, application des effets, tirages |
| `patient` | `params.py`, `patient.py` | patient sain (allométrie), variabilité, état |
| `hemo` | `physio.py` (simulate_hemo) | boucle cardiovasculaire à 6 compartiments |
| `gas` | `physio.py` (stewart, gas_exchange…) | acide-base de Stewart, échanges gazeux |
| `engine` | `physio.py` (evaluate) | boucle baroréflexe, perfusion, lactate, rein |
| `pkpd` | `pkpd.py` | PK 2 compartiments + dépôt oral, effets, diurèse |
| `autonomic` | `autonomic.py` | tonus sympathique/vagal, RSA, nœud sinusal, VFC |
| `signals` | `signals.py` | ECG (gaussiennes), PCG |
| `calibrate` | `calibrate.py` | banc de cibles (option, en test) |

## Stabilité des interfaces
Le contrat CLI (`docs/CLI_CONTRACT.md`) et l'API JSON de KMP (`docs/KMP.md`) sont versionnés (`api_version`). Toute rupture incrémente la version majeure.
