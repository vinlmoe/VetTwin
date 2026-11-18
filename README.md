# VetTwins : application web (site statique)

Site construit par `vetsim-kit/web/build.sh` depuis la branche `ccr-1d9c6579-yurqrn` (commit d57ad02). Le moteur vetsim (Rust) est compilé en WebAssembly : tout le calcul se fait dans le navigateur, sans serveur de calcul. **Usage pédagogique uniquement.**

- `index.html` : application ; `selftest.html` : autotest (comparaison au bit près avec le binaire natif).
- Branche générée : ne pas modifier à la main, reconstruire avec `build.sh`.
