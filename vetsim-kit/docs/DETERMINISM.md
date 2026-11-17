# Déterminisme et aléa (jalon M5)

- **Générateur** : ChaCha8 (`rand_chacha`), initialisé par une graine `u64` explicite (`SimRng::new(seed)`). Aucune source d'entropie système : le noyau reste compatible wasm32 sans `getrandom`.
- **Tirages** : uniforme sur 53 bits, normale par Box-Muller (les deux valeurs sont utilisées), lognormale de moyenne 1 `exp(N(−σ²/2, σ²))`, Bernoulli `u < p`, choix pondéré par cumul. Les fonctions transcendantes des tirages viennent du crate `libm` (Rust pur) : pour une graine donnée, les valeurs tirées sont identiques au bit près sur toutes les plateformes. Le test `rng::tests::stream_snapshot` fige le flux.
- **Ordre des tirages d'un patient** :
  1. variabilité interindividuelle, dans l'ordre de `interindividual_variability` de `healthy_dog.json` ;
  2. pour chaque maladie, dans l'ordre d'ajout : stade (si non imposé), sévérité uniforme dans le stade, puis tirages `draws` dans l'ordre du module.
- **Mode sans bruit** (`NoNoise`, CLI `eval`, golden) : lognormal → 1, Bernoulli → faux, sévérité au milieu du stade, stade de plus forte probabilité a priori.
- **Différence avec la référence Python** : NumPy (PCG64) et ChaCha8 ne produisent pas les mêmes nombres. Les cohortes se comparent donc statistiquement (`tests/compare_calibration.py`), pas valeur par valeur.
- **Limite** : le modèle (hémodynamique, gaz) utilise les fonctions mathématiques de la bibliothèque standard (`exp`, `powf`, `tanh`…), qui appellent la libm de la plateforme. Leur dernier bit peut différer entre plateformes (glibc, Apple, MSVC, wasm), ce qui peut décaler une sortie à ~1e-15 près. Près d'un seuil discontinu (critère d'arrêt du baroréflexe, voir `SOLVER.md`), ce décalage peut changer le nombre de passages. Pour une reproductibilité au bit près du modèle entier, il faudrait passer tout le noyau sur `libm` : c'est une décision à prendre (coût faible, gain de bits identiques partout).
