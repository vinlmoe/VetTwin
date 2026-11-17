# Proposition : toxicité, décompensation et risque de décès

Statut : **validée le 17/11/2026 et implémentée** avec les choix de l'utilisateur : termes toxiques séparés, placeholders D acceptés, référence Python étendue, **tout mécanistique** (pas de risque instantané statistique : le décès est l'absence d'état viable). Spécification normative : `docs/MODEL_SPEC.md` §6 bis. La section C ci-dessous (risque instantané) est donc **abandonnée**.

Usage pédagogique uniquement. Les seuils toxiques ne doivent jamais servir à choisir une dose réelle.

## Constat (état actuel du dépôt)

1. **La PD sature.** `pkpd.rs::emax_effect` calcule E = Emax·Cʰ/(EC50ʰ + Cʰ).
   - Le schéma accepte déjà `emax > 1` (contrainte « ≥ 0 », `modules.rs`), donc E peut techniquement dépasser 1.
   - Mais au-delà de quelques EC50, doubler la dose ne change plus rien. Un surdosage est donc invisible.
   - Les effets sont en plus bornés (`vagal_block` et `beta_block` : `clamp [0, 1]`).
2. **Aucun effet toxique n'est décrit dans les modules.** Les PD sont toutes des placeholders (evidence D).
3. **Les « signes émergents »** (`vetsim-api::emergent_signs`) sont des seuils d'alerte : souffle, B3, POG > 20, Na:K < 27, créatinine > 125, K⁺ > 5,5.
   - Ils ne font pas évoluer l'état du patient.
   - Il n'existe aucune notion de décompensation ni de décès.
4. Le **Python de référence** n'en contient pas non plus. Or il est normatif : toute nouveauté doit y être écrite d'abord, puis portée en Rust (règle du projet).

## Réponse à « peut-on aller au-delà de 1 ? »

Oui techniquement, mais ce n'est pas la bonne réponse. E > 1 ne fait qu'amplifier l'effet thérapeutique lui-même (par exemple un blocage vagal à 120 %, ce qui n'a pas de sens). Les toxicités réelles passent par **d'autres mécanismes, à d'autres concentrations** :

| Médicament | Effet toxique |
|---|---|
| Opioïde | Apnée |
| Furosémide | Hypovolémie et hypokaliémie (déjà émergentes via D et U) ; ototoxicité |
| Pimobendane | Tachyarythmies, hypotension |
| Bêtabloquant | Bloc AV, choc cardiogénique |
| Atropine | Tachycardie extrême, hyperthermie |

Proposition : garder E (effet thérapeutique, borné) et ajouter **un ou plusieurs termes toxiques T_k**. Chacun a son propre Emax, sa CE50 (plus haute) et sa pente, et agit sur ses propres paramètres.

## Conception proposée (trois couches)

### A. Effets toxiques dans les modules de médicaments (schéma 1.1, rétrocompatible)

```json
"pd": {
  "type": "emax", "emax": 1.0, "ec50_mg_L": 0.015, "hill": 1.3, "effects": [...],
  "toxicity": [
    { "id": "respiratory_arrest", "emax": 1.0, "ec50_mg_L": 0.2, "hill": 3.0,
      "evidence": "D", "source": "...",
      "effects": [{ "param": "vent_drive", "op": "mul", "expr": {"add": [1, {"mul": [-0.9, "T"]}]} }] }
  ]
}
```

- Nouvelle variable DSL : `T` (effet toxique), au même titre que `E`, `D` et `U`. Pas d'`eval`.
- Option `"effect_site_h"` : constante de délai du compartiment d'effet (Ce suit C avec une demi-vie donnée). Elle sert aux toxicités retardées.
- Paramètres nouveaux possibles, à introduire seulement si un effet en a besoin :
  - `av_block` (conduction) ;
  - `arrhythmia_risk` ;
  - `temperature`.

### B. Décompensation : états cliniques dérivés de l'état physiologique

Le module `modules/outcomes.json` porte les seuils et la provenance de chaque critère, avec un niveau de preuve. Il est évalué par une fonction pure du noyau, à chaque `state` et à chaque instant de `timecourse`.

| Critère (exemple) | Variable du moteur | Seuil | Source à trouver |
|---|---|---|---|
| Œdème pulmonaire | `la_mean` + PaO₂ | POG > 20–24 mmHg et PaO₂ < 60 | Yao & Aoki 2025 (citant Guyton 1959) ; à compléter |
| Choc | PAM, lactate | PAM < 60 ; lactate > 4 | ACVIM 2019 : PAS > 85 ou PAM > 60 sous vasodilatateur |
| Hypoxémie sévère | PaO₂, SaO₂ | PaO₂ < 60 | à sourcer |
| Hyperkaliémie menaçante | K⁺ | à sourcer (ECG : bradycardie, onde P absente) | à sourcer |
| Hypokaliémie sévère | K⁺ | à sourcer | à sourcer |
| Acidémie sévère | pH | à sourcer | à sourcer |
| Apnée ou hypoventilation | `vent_drive`, PaCO₂ | à sourcer | à sourcer |

Par ailleurs, quand la boucle hémodynamique ne converge plus (baroréflexe épuisé, débit nul), l'API renverrait un état « décompensation non résolue » au lieu d'une erreur.

### C. Risque de décès : modèle à risque instantané (hazard), reproductible

- Risque instantané h(t) = Σ_k h_k(état), avec h_k = 0 sous le seuil k et une croissance au-delà, par exemple log-linéaire. Survie : S(t) = exp(−∫h).
- **Mode déterministe** (CLI `eval`, golden) : seuls S(t) et les critères actifs sont rapportés. Pas de tirage.
- **Mode stochastique** : un nouveau flux ChaCha `OUTCOME = 5` tire l'instant du décès éventuel. La graine est explicite (`docs/DETERMINISM.md`), et les flux existants ne bougent pas.
- **Sources disponibles aujourd'hui :** une seule est chiffrée, Yin 2026 (lu) : PAS < 90 mmHg à l'admission donne un HR de 6,3 (IC 1,3–30) pour la mort cardiaque à 2 mois. Ce n'est pas un risque instantané. Presque tous les coefficients seraient donc des **placeholders D** au départ, signalés comme tels dans l'interface.

### D. API, interface, tests

- **API.** `state` renverrait `outcomes: {criteria: [...], hazard_per_h, survival}` ; `timecourse` renverrait `outcomes_at` et une courbe S(t). Le contrat `eval` du CLI et le golden ne changent pas (nouvelles clés hors contrat, `api_version` incrémentée).
- **Interface (web et KMP).**
  - Bandeau « décompensation » avec les critères actifs.
  - Courbe de survie dans l'onglet Traitement.
  - Mention « placeholder D » sur chaque critère non sourcé.
  - Le patient « décédé » fige les courbes.
- **Tests.**
  - Référence Python d'abord, puis fixtures Rust à 1e-9.
  - Contrôles d'échec :
    - une dose 10× d'opioïde doit déclencher l'apnée ;
    - un patient sain ne doit déclencher aucun critère ;
    - la mutation d'un seuil doit faire échouer le test.

## Ordre de travail proposé

1. **Littérature.** Doses ou concentrations toxiques chez le chien (atropine, opioïde, bêtabloquant, pimobendane, furosémide) ; seuils cliniques de décompensation (K⁺, pH, PAM, lactate, PaO₂) ; données de survie exploitables. Avec rapport d'accès et sources bloquées, comme pour `VERIFICATION_LITTERATURE.md`.
2. **Schéma 1.1** + validation (`validate_modules.py`, `check_modules.py`).
3. **Référence Python** : couches A puis B puis C, avec fixtures.
4. **Portage Rust**, API, interfaces, journal.

## Décisions demandées

1. Valider l'approche « termes toxiques séparés » plutôt que E > 1.
2. Décès : risque instantané stochastique (avec un mode déterministe sans décès) ou seuils déterministes seulement ?
3. Accepter des coefficients placeholders D pour le risque de décès tant que la littérature manque, ou n'activer que les critères sourcés ?
4. La référence Python, normative, doit être étendue : confirmer.
