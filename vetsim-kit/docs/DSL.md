# DSL de formules (version 1.0.0)

Une expression est :
- un **nombre** (`2.4`), un **booléen**, ou un **nom de variable** (chaîne) ;
- ou un **objet à une seule clé** `{op: argument(s)}`.

Variables disponibles selon le contexte
| Contexte | Variables |
|---|---|
| patient sain (`params`) | `s = bw/20`, `bw` |
| maladie | `s` (sévérité 0–1), les noms de `draws`, les noms de `let` déjà définis |
| médicament (`pd.effects`) | `E` (effet Emax normalisé 0–1), `D` (déficit de volume, mL) et `U` (diurèse effective, mL/kg) pour le type `diuretic` |

Opérateurs (arguments listés dans un tableau sauf mention)
`add`, `mul` (n-aires), `sub`, `div`, `pow` (binaires), `exp`, `log`, `neg`, `abs` (unaires), `min`, `max` (n-aires), `clip [x, lo, hi]`, `if [cond, a, b]`, `lt`, `le`, `gt`, `ge`, `not`, `and`, `or`.

Règles
- Évaluation stricte en `f64`. Une variable inconnue, une division par zéro ou un `log` d'un nombre ≤ 0 est une **erreur de chargement ou d'évaluation**, jamais un `NaN` silencieux.
- Pas de boucle, pas de définition de fonction, pas d'accès aux paramètres du patient depuis une formule de maladie (les effets s'appliquent aux paramètres, ils ne les lisent pas).
- Les `let` sont évalués dans l'ordre d'écriture et peuvent référencer `s`, les `draws` et les `let` précédents.

## Tirages (`draws`)
- `{"dist": "lognormal_mean1", "sd": σ}` : valeur `exp(N(−σ²/2, σ²))`, de moyenne 1.
- `{"dist": "bernoulli", "p": expr}` : 1 avec probabilité `p`, sinon 0.
Chaque nom est tiré **une fois par cas** et réutilisable (ex. `z_c` partagé par Na et Cl dans l'Addison). Mode déterministe : lognormal → 1, bernoulli → 0 (c'est le mode du golden).

## Effets
`{param, op: "mul"|"add", expr, when?, clamp?}` appliqués **dans l'ordre** : `mul` donne `p ← p × expr`, `add` donne `p ← p + expr`. `when` (booléen) rend l'effet conditionnel. `clamp [lo, hi]` borne la valeur finale du paramètre.

## Sévérité
Un cas est tiré en choisissant un stade selon `prior`, puis `s` uniforme dans les bornes du stade; ou `s` est imposé (le stade se déduit des bornes).

## Interpréteur de référence
`tests/dsl_ref.py` (≈40 lignes). Le Rust doit donner les mêmes valeurs à 1e-9 près sur `check_modules.py` (ports de ce script à écrire en test Rust).
