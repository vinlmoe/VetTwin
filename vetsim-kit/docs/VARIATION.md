# Un chien suivi dans le temps (variabilité, Monte-Carlo)

Code : `crates/vetsim-core/src/variation.rs`. Données : `modules/variation.json` (schéma `schema/variation.schema.json`). API : `Engine::montecarlo`. Web : onglet Traitement, carte « Un chien suivi dans le temps ».

## Principe
1. **On tire le chien une fois.** Ses paramètres physiologiques sont tirés selon des lois normales sur leur logarithme (multiplicateurs lognormaux de moyenne 1, CV de `interindividual_variability` du module patient). Ses maladies sont tirées ensuite (sévérité, tirages du module). Le tirage dépend de la graine du chien.
2. **On tire sa PK/PD une fois.** Clairance, volume central et CE50 de chaque médicament sont multipliés par un lognormal de moyenne 1 (`drug` : `cl_cv`, `vc_cv`, `ec50_cv`), tirés une fois par chien.
3. **On le suit dans le temps.** Sans autre aléa, la courbe ne bouge qu'au gré de la maladie (progression, optionnelle) et du médicament (C(t), D(t), U(t)). **C'est le mode par défaut** (`scale` = `{natural: 0, disease: 0, drug: 1, measurement: 0}`).

## Sources optionnelles, avec un nouveau tirage à chaque instant
- `natural` : fluctuation des paramètres au fil des heures, processus d'Ornstein-Uhlenbeck sur ln(paramètre), écart-type `cv` et temps de corrélation `tau_h`, discrétisation exacte.
- `disease` : jours bons et mauvais. On a `s(t) = s0 + r·t/24 + sd·x(t)` borné à [0, 1], et les effets sont réévalués avec les mêmes tirages du cas.
- `measurement` : imprécision analytique, bruit multiplicatif indépendant à chaque prélèvement.

La progression `progression_per_day` (par maladie) est déterministe : elle s'applique quelle que soit `scale.disease`.

## Plusieurs réalisations
Avec `n > 1`, on obtient plusieurs trajectoires du même chien et des percentiles par instant (P5, P25, P50, P75, P95), calculés sur les réalisations vivantes, avec le décès absorbant. La réalisation r utilise la graine `seed + r`.

## Limites
- Toutes les amplitudes de `modules/variation.json` sont des placeholders (`evidence: "D"`), à caler sur des études de variation biologique intra-individuelle chez le chien et sur des données PK de population.
- La PK utilise les paramètres du chien à t = 0.
- Coût : environ 12 ms par instant évalué, et environ 120 ms en MMVD (orifice mitral dynamique).

Usage pédagogique uniquement.
