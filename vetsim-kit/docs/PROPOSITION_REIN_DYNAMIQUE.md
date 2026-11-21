# Proposition : rein dynamique (21/11/2026)

**Statut (21/11/2026) : implémenté (1.3.0, aigu et chronique ≤ 12 j)** (`crates/vetsim-core/src/renal.rs`, `modules/renal/renal_model.json`, message API `renal_course`). Bilan et écarts : `docs/VALIDATION_HORS_CALIBRAGE.tex`, section « Rein dynamique ».

| Étape | État |
|---|---|
| 1. Filtration et autorégulation | Autorégulation vis-à-vis du débit ajoutée au rein stationnaire (`renal_co_auto`) ; le DFG reste celui du moteur |
| 2. Sodium et eau | Bilan du Na et VEC iso-osmotique ; pas d'osmolalité, de vasopressine ni de natrémie de dilution |
| 3. Potassium | Deux compartiments, sécrétion distale portée par le débit distal, l'aldostérone et la kaliémie |
| 4. RAAS | Indice unique d'activité, piloté par la déplétion du VEC ; pas de rénine ni d'angiotensine séparées |
| 5. Diurétiques et IEC | Furosémide par sa pharmacodynamie existante ; pas de module IEC ni spironolactone (données canines contraires, voir le rapport) |

Domaine validé : l'aigu (une administration, ≤ 24 h). En prises répétées, la déplétion sodée est surestimée ; la voie stationnaire `D`/`U` reste la référence pour le chronique.

Proposition initiale ci-dessous.

## Pourquoi

Aujourd'hui le rein est **stationnaire**. Le DFG est un produit de facteurs (masse néphronique, PAM, débit, axe cardio-rénal). La créatinine et l'urée sont des concentrations à l'équilibre (`creat0 / ratio_DFG`). Le sodium, l'eau et le potassium ne sont pas bilancés.

Trois conséquences pour la pédagogie :

- **Effets diurétiques calés, pas calculés.** Les effets du furosémide (pertes de K, Cl, Na, déficit volémique D, azotémie prérénale) sont des coefficients calés (`A_K`, `K_GFR_D`, adaptation φ). Ils ne découlent pas d'un bilan.
- **Médicaments impossibles à représenter.** Les IEC (bénazépril), la spironolactone et le torasémide n'ont pas de cible dans le modèle.
- **Cercle vicieux absent.** Le cercle cardio-rénal (rétention sodée → précharge ↑ → congestion) et l'expansion du volume plasmatique de la MRC avancée (Pelander 2017 : 63,7 → 139,9 mL/kg en IRIS III, mesure à confirmer) ne peuvent pas émerger.

## Ce que serait le modèle

Un compartiment rénal **en temps** (heures–jours), branché sur la cinétique existante (`kinetics.rs`, `pkpd::timecourse`) :

1. **Filtration.**
   - DFG = DFG₀ · masse néphronique · f(pression de perfusion rénale = PAM − pression veineuse rénale) · f(tonus rénal sympathique et angiotensine II).
   - Autorégulation entre 75 et 160 mmHg.
2. **Réabsorption fractionnelle du sodium**, par segment :
   - proximal, modulé par l'angiotensine II et le sympathique ;
   - anse de Henle, cible du furosémide et du torasémide ;
   - tube collecteur, modulé par l'aldostérone, cible de la spironolactone.
3. **Eau** : réabsorption par la vasopressine. Osmolalité, natrémie de dilution.
4. **Potassium** : sécrétion distale = f(débit tubulaire distal, aldostérone, K plasmatique). La perte de K sous diurétique émerge du débit distal accru.
5. **RAAS** :
   - rénine = f(pression de perfusion, sodium distal, sympathique) ;
   - angiotensine II, aldostérone (constantes de temps en heures) ;
   - les IEC bloquent la conversion.
6. **Bilan hydrosodé** :
   - variation du volume extracellulaire, répartie plasma/interstitium par le remplissage transcapillaire déjà présent dans la session ;
   - variation du volume plasmatique → `vol_shift`.

Les sorties existantes restent : DFG, créatinine, urée, Na, K, Cl, volémie. S'y ajoutent la diurèse (mL/kg/h), la natriurèse, la densité urinaire, la FE_Na et la FE_K (déjà publiées chez le chien, Adin 2020).

## Données disponibles (déjà relevées dans les recherches de ce projet)

| Donnée | Source |
|---|---|
| FE_Na et FE_K en stades C et D | Adin 2020, lu |
| Aldostérone, angiotensine II, rénine, UAldo:C selon le stade ACVIM | Hammond 2023, Galizzi 2021, lus |
| Diurèse et natriurèse sous furosémide chez le chien sain | Koh 2021, lu |
| Créatinine, urée, électrolytes sous furosémide chronique | Adin 2020, TEST, Giorgi 2022, lus |
| Pression veineuse rénale et DFG | Fiksen-Olsen 1991, résumé |
| DFG au iohexol, volume plasmatique, NT-proBNP par stade IRIS | Pelander 2017, lu |

À chercher : bilans sodés canins (rétention en ICC), cinétique de la rénine et de l'aldostérone sous IEC chez le chien, pharmacologie du bénazépril et de la spironolactone.

## Coût et risques

- **Taille.** Le module est nouveau (Rust, puis miroir Python pour la contre-vérification). Il touche le CLI `eval`, qui doit rester stationnaire. Le rein dynamique ne vivrait que dans les simulations temporelles (`timecourse`, sessions), et l'état stationnaire de `eval` serait son point fixe.
- **Recalage.**
  - Furosémide : ses coefficients actuels seraient **remplacés** par le mécanisme, à recaler sur Koh, Adin, TEST et Giorgi.
  - MRC : sa créatinine par stade IRIS est à garder.
  - Golden et fixtures seraient régénérés.
- **Ordre proposé.** Chaque étape est validée avant la suivante, avec ses tests dans `tests/litterature.rs` :
  1. filtration et autorégulation ;
  2. sodium et eau ;
  3. potassium ;
  4. RAAS ;
  5. diurétiques et IEC.
