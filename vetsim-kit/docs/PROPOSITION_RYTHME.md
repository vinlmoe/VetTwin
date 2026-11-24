# Proposition : troubles du rythme cardiaque, modélisés mécaniquement

Statut : **proposition, en attente de validation.** C'est un changement d'architecture : nouvelle couche de conduction, hémodynamique battement par battement, oreillette contractile. Rien n'est implémenté.

Principe (règle du projet) : rien n'est plaqué. Chaque trouble naît d'un mécanisme de génération ou de conduction de l'influx. L'ECG, l'hémodynamique, le pouls et les bruits du cœur en découlent battement par battement.

Les valeurs numériques citées ci-dessous sont des ordres de grandeur à caler. Elles viendront de la recherche bibliographique en cours ; tout ce qui reste non sourcé sera marqué placeholder D.

Usage pédagogique uniquement.

## État actuel

- **Nœud sinusal :** intégration de phase (IPFM) modulée par les tonus vagal et sympathique, la respiration et le baroréflexe → instants R (`autonomic.py`, `autonomic.rs`).
- **Hémodynamique :** un cycle **régulier**, de période fixe 60/FC, simulé jusqu'au régime périodique (`simulate_hemo`).
- **Pas d'oreillette contractile :** le compartiment veineux pulmonaire `pv` sert d'OG passive. La « contraction atriale » n'existe donc pas : on ne peut pas la perdre (fibrillation atriale) ni la désynchroniser (bloc AV complet).
- **ECG et PCG :** formes posées sur les instants R. Le PR est un paramètre fixe (`pr`), et le son reprend le battement simulé.

## Architecture proposée (4 couches)

### 1. Couche de conduction (nouveau module `rhythm.py`, puis `rhythm.rs`)

Simulation par événements. Elle produit une liste de battements `{t_A (activation auriculaire), t_V (activation ventriculaire), origine, contraction atriale oui/non, délai AV}`.

| Élément | Mécanisme | Paramètres (nouveaux, à caler) |
|---|---|---|
| **Nœud sinusal** | IPFM existant. Bloc de sortie sino-atrial : une impulsion n'active pas l'oreillette si l'excitabilité de sortie est insuffisante (pauses de la maladie du sinus) | `sa_exit` (0–1) |
| **Oreillettes** | Activation par le nœud sinusal, ou en **fibrillation** par un processus d'activations rapides et irrégulières (cycle moyen et dispersion). L'excitabilité atriale baisse avec K⁺ : arrêt atrial en hyperkaliémie | `af` (0/1), `af_cycle`, `af_cv`, `atrial_k_block` |
| **Nœud AV** | Délai qui dépend du temps de récupération : `AH = AH_min + β·exp(−récup/τ)`. Période réfractaire `ERP`. Une activation qui arrive pendant l'ERP est bloquée. **Wenckebach (Mobitz I)** et **filtrage de la FA** en émergent. Le vagal allonge `AH_min` et `ERP` (l'atropine les raccourcit). Un bloc infranodal (Mobitz II, 3e degré) vient d'une probabilité de conduction His-Purkinje | `av_ah_min`, `av_beta`, `av_tau`, `av_erp`, `hp_conduction` (0–1), gains vagaux |
| **Échappements** | Foyers jonctionnel et ventriculaire, avec automatisme propre (IPFM lent). Ils déchargent si aucune activation ventriculaire n'arrive dans leur cycle. Le **bloc de 3e degré** donne alors un rythme d'échappement, avec dissociation AV | `junct_rate`, `vent_escape_rate` |
| **Foyer ectopique ventriculaire** | Automatisme anormal (parasystolie), ou déclenchement couplé au battement précédent (intervalle de couplage). Il réinitialise ou non le nœud sinusal, d'où la **pause compensatrice**. L'hypokaliémie augmente son automatisme | `vpc_rate`, `vpc_coupling`, `vpc_k_gain` |

L'aléa (FA, décharges ectopiques) passe par un **nouveau flux ChaCha `RHYTHM = 5`**. Le flux est explicite (`docs/DETERMINISM.md`), et les flux existants ne bougent pas.

### 2. Hémodynamique battement par battement (`simulate_hemo`)

- Chaque battement a sa propre durée : l'élastance ventriculaire est activée à `t_V`, et non plus selon une période fixe.
- **Oreillettes contractiles :** élastance active des compartiments OG et OD, déclenchée à `t_A`, avec la même forme d'activation que les ventricules.
- **Ce qui doit émerger sans être imposé :**
  - la perte de la contribution atriale en FA (débit plus faible, POG plus haute) ;
  - les ondes « canon » de la dissociation AV ;
  - un volume d'éjection réduit après un intervalle court (Frank-Starling) ;
  - le déficit de pouls ;
  - la variabilité battement à battement de la PA en FA.
- **Battement ectopique ventriculaire :** contraction moins synchrone, avec une élastance efficace réduite pour ce battement (placeholder D) et un QRS large à l'ECG.
- **Régime permanent :** pour un rythme irrégulier, `evaluate` moyenne débit et PA sur N battements, et le baroréflexe travaille sur ces moyennes.
- **Rythme sinusal normal :** la couche est contournée et le chemin de calcul est **identique** à l'actuel. Le golden et la calibration ne doivent pas bouger, ce qui sera vérifié comme pour la décompensation.

### 3. Signaux issus de la séquence de battements

- **ECG :**
  - onde P à chaque `t_A` (ondes f en FA, absentes en arrêt atrial) ;
  - PR = délai AV de **ce** battement ;
  - QRS fin si conduit, large et de morphologie différente si ventriculaire ;
  - effets de K⁺ conservés.
- **Auscultation :**
  - B1 à la fermeture mitrale et tricuspide de chaque battement simulé, d'intensité liée au dP/dt et au PR (un PR court donne un B1 fort) ;
  - B2 à la fermeture sigmoïde ;
  - en FA, l'intensité variable de B1 et l'irrégularité émergent ;
  - le souffle reste lié au débit régurgitant de chaque battement.
- **Pouls et PA instantanée :** issus du battement simulé, d'où le déficit de pouls.

### 4. Maladies, médicaments, ionogramme

- **Nouvelles maladies** (décision du 17/11/2026) : chaque trouble est un module maladie à part entière (`modules/diseases/*.json`), avec stades et sévérité, combinable avec les autres (MMVD + FA, MRC + hyperkaliémie…). Modules (JSON, DSL) :

  | Module | Paramètres modifiés |
  |---|---|
  | `fibrillation_atriale` | `af`, `af_cycle`, `af_cv` |
  | `bloc_av` (degrés 1 à 3) | `av_ah_min`, `av_erp`, `hp_conduction` |
  | `extrasystoles_ventriculaires` | `vpc_rate`, `vpc_coupling` |
  | `maladie_du_sinus` | `sa_exit`, automatisme sinusal |

- **Atropine :** bloque la voie vagale sur le nœud sinusal **et** sur le nœud AV. Son effet sur la conduction et ses tachyarythmies découleraient de ces mêmes paramètres, d'où une « toxicité » mécanistique, sans terme plaqué.
- **K⁺ :**
  - l'hyperkaliémie ralentit la conduction et réduit l'excitabilité atriale, jusqu'à l'arrêt atrial et au rythme sino-ventriculaire ;
  - l'hypokaliémie augmente l'automatisme ectopique.

  Le frein actuel `kfac` sur la FC serait remplacé par ces mécanismes. Cela change la référence : nouvelle référence, et le golden n'est concerné que si K > 6.
- **Bêtabloquant :** ralentit le nœud sinusal et la conduction AV.

## Vérification prévue

- Python d'abord, puis Rust ; fixtures à 1e-9 sur les séquences de battements, en rejouant les tirages comme en M6.
- Critères chiffrés tirés de la littérature, quand elle sera disponible :
  - FC ventriculaire en FA ;
  - fréquences d'échappement ;
  - seuil de PR du BAV 1 ;
  - pause compensatrice ;
  - perte de débit en FA ;
  - pauses du chien sain au Holter.
- Contrôles d'échec : sans oreillette contractile, la FA ne doit plus réduire le débit ; sans ERP, le Wenckebach ne doit plus apparaître.
- Golden, calibration et décompensation inchangés en rythme sinusal.

## Ordre de travail

1. Retour de la bibliographie, puis tableau des cibles.
2. Oreillettes contractiles et hémodynamique à durée de battement variable, en rythme sinusal. Vérifier l'identité avec le modèle actuel, ou une nouvelle référence si l'oreillette change le chien sain : **décision à prendre**.
3. Couche de conduction (nœud sinusal, nœud AV, échappements, ectopie, FA).
4. ECG et auscultation dérivés de la séquence.
5. Modules maladie, effets de l'atropine, du bêtabloquant et de K⁺.
6. API, interfaces, journal.

## Décisions demandées

1. **Oreillette contractile chez le chien sain.** L'ajouter modifie un peu le chien sain (contribution atriale au remplissage), donc le golden. Faut-il la caler pour que les valeurs du chien sain restent dans les tolérances, ou accepter une nouvelle référence ?
2. ~~FA et dilatation de l'OG~~ : tranché, les troubles du rythme sont de nouvelles maladies (modules indépendants, combinables).
3. **Hyperkaliémie.** Faut-il remplacer le frein `kfac` actuel par les mécanismes de conduction ? C'est plus mécanistique, mais cela change la référence au-dessus de K = 6.
