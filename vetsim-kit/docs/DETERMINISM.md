# Déterminisme et aléa (jalon M5)

- **Générateur** : ChaCha8 (`rand_chacha`), initialisé par une graine `u64` explicite (`SimRng::new(seed)`). Aucune source d'entropie système : le noyau reste compatible wasm32 sans `getrandom`.
- **Tirages** : uniforme sur 53 bits, normale par Box-Muller (les deux valeurs sont utilisées), lognormale de moyenne 1 `exp(N(−σ²/2, σ²))`, Bernoulli `u < p`, choix pondéré par cumul. Les fonctions transcendantes des tirages viennent du crate `libm` (Rust pur) : pour une graine donnée, les valeurs tirées sont identiques au bit près sur toutes les plateformes. Le test `rng::tests::stream_snapshot` fige le flux.
- **Ordre des tirages d'un patient** :
  1. variabilité interindividuelle, dans l'ordre de `interindividual_variability` de `healthy_dog.json` ;
  2. pour chaque maladie, dans l'ordre d'ajout : stade (si non imposé), sévérité uniforme dans le stade, puis tirages `draws` dans l'ordre du module.
- **Flux par usage** : pour une graine donnée, chaque usage a son propre flux ChaCha (`SimRng::for_stream`, constantes `signals::streams`) : 0 patient (variabilité, maladies), 1 système autonome, 2 ECG, 3 PCG. Ajouter une maladie ne change donc pas le bruit de l'ECG.
- **Mode sans bruit** (`NoNoise`, CLI `eval`, golden) : lognormal → 1, Bernoulli → faux, sévérité au milieu du stade, stade de plus forte probabilité a priori.
- **Différence avec la référence Python** : NumPy (PCG64) et ChaCha8 ne produisent pas les mêmes nombres. Les cohortes se comparent donc statistiquement (`tests/compare_calibration.py`), pas valeur par valeur.
- **Modèle entier** : toutes les fonctions transcendantes du noyau (`exp`, `ln`, `pow`, `tanh`, `cos`, `sin`) passent par `vetsim_core::math`, qui s'appuie sur `libm`. `sqrt` et les opérations arithmétiques sont correctement arrondies par IEEE 754. Les méthodes `f64::exp`, `powf`, etc. sont interdites par `clippy.toml` (`disallowed-methods`, vérifié en CI). Une même entrée donne donc les mêmes bits partout, y compris près des seuils discontinus (`SOLVER.md`).
- **Réserve** : le compilateur ne fusionne pas `a*b + c` en FMA sans demande explicite (Rust n'active pas la contraction flottante), donc l'arithmétique reste identique entre cibles. Aucune comparaison réelle entre plateformes n'a encore été faite : à vérifier au jalon M7 en comparant les sorties Android/JVM/wasm aux sorties macOS.
