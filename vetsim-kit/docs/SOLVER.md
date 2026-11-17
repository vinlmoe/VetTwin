# Solveur de l'hémodynamique (jalon M3)

## Choix
La référence Python intègre la boucle cardiovasculaire avec LSODA (`scipy.integrate.solve_ivp`, `rtol = atol = 1e-6`, `max_step = 0,004 s`). Il n'existe pas d'équivalent direct en Rust. Le moteur utilise **Dormand-Prince 5(4)** à pas adaptatif (`crates/vetsim-core/src/ode.rs`), avec le contrôle de pas, le choix du pas initial et l'interpolant dense d'ordre 4 de `scipy.integrate.RK45`, et les mêmes tolérances que la référence.

- Les indicateurs sont calculés sur des échantillons à `dt = 2 ms` des deux derniers battements, obtenus par interpolation dense. Les instants sont générés comme `np.arange` (longueur `ceil((stop − start)/dt)`, pas `(start + dt) − start`).
- Aucune allocation dans la boucle d'intégration : l'état est un `[f64; 6]`.
- Pas de repli implicite : il n'a pas été nécessaire. Les constantes de temps les plus courtes du modèle (élastance ventriculaire / résistance valvulaire, quelques ms) restent stables avec le pas maximal de 4 ms. Le balayage de robustesse (3 poids × 4 maladies × 11 sévérités) n'a produit aucun échec ni régime non périodique.

## Limites
1. **Méthode explicite** : un module qui rendrait le système raide (résistance valvulaire beaucoup plus petite, élastance beaucoup plus grande) obligerait à réduire `max_step` ou à ajouter un solveur implicite (TR-BDF2 ou Rosenbrock). Symptôme attendu : `OdeError::StepTooSmall` ou un temps de calcul qui explose.
2. **Écart avec LSODA** : environ 1e-5 en relatif sur les indicateurs. C'est le même ordre que l'écart entre les solveurs de SciPy (LSODA, RK45, DOP853) sur ce modèle.
3. **Discontinuité du critère d'arrêt du baroréflexe (référence)** : la boucle s'arrête dès que `|d_t − d| < 0,04`. Quand la référence passe ce seuil avec une marge plus petite que l'écart entre solveurs (~1e-4), le Rust peut faire un passage de plus ou de moins. Sa sortie saute alors vers le point fixe voisin.
   - Cas observé : MMVD `s = 1,0` à 20 kg. Marge de la référence 6,7e-5 au 4e passage, `|d_t − d|` = 0,039933 (Python) contre 0,040001 (Rust), d'où 5 passages au lieu de 4.
   - Effets : `lv_edp` 1,326 contre 1,368 (−3,1 %, 1,02 × la tolérance du golden) ; FC 196,4 contre 195,5 ; PAM 93,83 contre 93,52.
   - Au 4e passage, l'état Rust est identique à la référence (`lv_edp` 1,367690 contre 1,367684).
   - Ce cas n'est pas dans le golden (100 % conforme). Il est signalé, non masqué, par `tests/hemodynamics.rs`.
   - Correction possible, qui relève d'une décision de modèle et non du port : remplacer l'arrêt sur seuil par une convergence plus serrée ou un nombre fixe d'itérations. Cela changerait la référence et le golden.
