# Provenance des données et limites

**Niveaux de preuve (`evidence`)** : A calé sur mesures publiées; B bornes ou critères publiés; C plausibilité à partir d'intervalles publiés; D placeholder pédagogique.

## Sources utilisées
| Bloc | Source | Utilisation | Limite |
|---|---|---|---|
| Hémodynamique, gaz du sang | Haskins et al. 2005 (97 chiens normaux, éveillés) | FC 87, PAM 103, PAP 14, PAOP 5,5, IC 4,42 L/min/m², gaz art. et veineux mêlés | Seul le résumé a été lu : moyennes sans écart-type. DC dérivé de l'IC par la formule de Meeh (0,745 m² à 20 kg) |
| Acide-base | Constable & Stämpfli 2005, J Vet Intern Med 19:507 (10 chiens sains) | Na, K, Cl, albumine, phosphate; Atot de l'albumine 0,469 mmol/g, pKa 7,77, charge nette 0,42 mEq/g | Coefficient du phosphate encore humain (Figge-Fencl) |
| Affinité O₂ | Cambier et al. 2004, Res Vet Sci 77:83 | P50 30,0 ± 1,3 mmHg; Δlog P50/ΔpH = −0,370 | — |
| VFC | Bogucki & Noszczyk-Nowak 2015, Pol J Vet Sci (50 chiens, 60 min) | RR moyen, SDNN, RMSSD, LF/HF | Source unique, valeurs très élevées : à recouper |
| ECG | Labrador et berger allemand (n = 30, dérivation II) ; Bispo 2025 (n = 22) ; Romito 2022 (n = 180) | PR, QRS, QT, amplitudes P, R, T ; durée visible de P (45 ms), QRS visible = `qrs`, segment ST présent | Durées et amplitudes mesurées sur le tracé (`docs/VERIFICATION_ECG.md`) ; largeur de T et élargissement de P en hyperkaliémie : placeholders D |
| PK/PD furosémide | Koh 2021 (BMC Vet Res 17:295, texte intégral) ; Adin 2021 (JVIM 35:1245) ; Roudebush 1994 (résumé) | Koh : CL 735,7 mL/h/kg, Vss 0,668, C0 9,65, F 0,55, t½ 2,07 h, Cmax PO 0,61 ; diurèse horaire (tableau 5) ; électrolytes sériques (tableau 4) et urinaires (tableau 6). Adin : K⁺ inchangé à 24 h | cl rapportée à 20 kg (BW^0,75) ; compartiment d'effet ke0 calé sur la diurèse par intervalle ; τ_K = 8 h compromis Koh/Adin ; Tmax PO 0,5 h (publié 1) ; tau_refill, tau_elect, f_plasma placeholders |
| PK pimobendane | Her et al. 2020, Front Vet Sci 7:423 (0,5 mg/kg per os) | Cmax, AUC, t½ | Tmax mal reproduit; F = 0,5 supposée |
| IRIS | Staging IRIS | Bornes de créatininémie par stade | — |
| MMVD | MSD Vet Manual; JVIM 2004 (MR expérimentale) | RF 17–81 %, œdème si POG > ~20 mmHg | FC 130 et 160/min aux stades C et D : choix clinique non sourcé |
| Addison | MSD Vet Manual | Na:K < 27 | Cas rapporté unique pour K 8,3 et créatinine 2,6 mg/dL |

## À ne pas oublier
- Calage et non validation prospective : aucune donnée n'est tenue à part.
- Population hétérogène (races, poids, méthodes) dans les sources.
- Placeholders : PD de tous les médicaments, PK de l'atropine, du bêtabloquant et de l'opioïde, module pneumonie.
- Placeholders (evidence D), ajoutés le 17/11/2026 : termes toxiques des médicaments (`pd.toxicity` : EC50 toxique = 10 × EC50 thérapeutique, Hill 3, amplitudes) et paramètres de décompensation de `healthy_dog.json` (`acid_ph0` 7,20, `acid_ees` 1,5, `acid_rsys` 1,0, `hypox_pvo2` 20 mmHg, `hypox_ees` 0,03, `k_weak` 2,5 mmol/L, `k_weak_vent` 0,4, `cpp_min` 40 mmHg, `cpp_ees` 0,02, `hr_arrest` 20 /min, `co_arrest_frac` 0,15). Aucune source : ordres de grandeur pédagogiques. Voir `docs/PROPOSITION_TOXICITE.md`.
- Usage pédagogique uniquement.

## Conduction (`reference/python/rhythm.py`, 17/11/2026)
- Calés : `av_erp` 0,15 s et `av_conceal` 0,75 sur la FA canine (FC Holter médiane 129,7 [116,8–156,0], CVRR 0,309 [0,247–0,442], n = 50 ; 10.3389/fcvm.2026.1890025) sous la contrainte de conduction 1:1 jusqu'à ~225/min (FC max Holter des chiens sains 204 ± 25 ; 10.1371/journal.pone.0319886). Modèle : 129,6 /min, CVRR 0,291.
- Sourcés : `vent_escape_rate` 42 /min (42 ± 11, n = 97 ; 10.1111/jvim.12512) ; `k_atrial_standstill` 8,5 mmol/L (arrêt atrial 8,5–10 avec conduction sino-ventriculaire, Hariman 1983, résumé) ; `k_hp_block` 10 (élargissement du QRS au-delà de 10,1, Tag & Day 2008, vu par moteur seulement).
- Placeholders D : `af_cycle`, `af_cv`, `hv`, `av_beta`, `av_tau`, `av_vagal`, `av_k_gain`, `hp_k_slope`, `junct_rate` 50, `esc_k_gain`, `vpc_k_gain`, `k_vpc`.
- Oreillette : gain `k_la` à caler sur E/A ≈ 1,39 (E 0,82, A 0,59 m/s, n = 53 ; 10.3390/ani15111524) ; k_la ≈ 1,45 donne E/A 1,39 à la FC de repos du modèle (contribution ≈ 10 % du VES). Les ondes fusionnent au-delà d'environ 120/min.

## Troubles du rythme (modules `atrial_fibrillation`, `av_block`, `ventricular_premature`, `sick_sinus`, 17/11/2026)
Ces modules n'agissent que sur les paramètres de la conduction. FC, ECG, hémodynamique et sons émergent.

- **Fibrillation atriale** (evidence C).
  - Effets : `af` = 1 ; `av_erp` × (1,25 − 0,5·s).
  - Réponse ventriculaire : 105, 132 et 155 /min aux sévérités 0,1, 0,5 et 0,9.
  - Comparaison : FC Holter médiane 129,7 (10.3389/fcvm.2026.1890025) ; FC > 150 chez 80 % des chiens non traités (10.1111/jvim.16666).
- **Bloc AV** (evidence C).
  - Effets :
    - PR + 0,12·min(1, s/0,33) ;
    - `av_erp` × (1 + 4·min(1, max(0, (s − 0,33)/0,33))) ;
    - `hp_conduction` × max(0, 1 − max(0, s − 0,66)/0,1).
  - Rythmes produits : 1er degré PR jusqu'à 0,24 s (critère > 0,13 s) ; Wenckebach à s ≈ 0,5 ; 2:1 à s ≈ 0,6 ; 3e degré avec échappement ventriculaire 42 /min (42 ± 11, n = 97 ; 10.1111/jvim.12512).
- **Extrasystoles ventriculaires** (evidence D).
  - Effet : `vpc_rate` + 30·s² /min.
  - Comparaison : le chien sain a moins de 20 ESV/24 h dans 86 % des cas (10.1371/journal.pone.0319886).
  - Pause compensatrice rarement complète (ERP nodal court, calé sur la FA) : à recaler.
  - Caractère productif : il émerge du remplissage écourté et de l'amplitude d'élastance propre au battement (voir « Relation force-intervalle » plus bas). Avec ESV 0,8 : non productive si le couplage est ≤ 0,43 s, productive si le couplage est tardif.
- **Relation force-intervalle et dyssynchronie** (`ca_tau` 0,2 s, `ca_recirc` 0,6, `ees_wide` 0,8 ; placeholders D, 17/11/2026).
  - Modèle de recirculation du Ca²⁺ à deux compartiments (structure : 10.1016/s0006-3495(87)83307-6 ; principe : 10.1113/jphysiol.1986.sp016167). Il produit la restitution mécanique et la potentialisation post-extrasystolique (10.1161/01.cir.88.6.2962). En rythme régulier, l'amplitude vaut 1 à toute fréquence.
  - QRS large (activation hors Purkinje, dyssynchronie ; 10.1016/s0735-1097(99)00068-6) : amplitude × `ees_wide`.
  - Valeurs non sourcées : à caler sur des courbes canines de dP/dt en fonction de l'intervalle de couplage.
- **Maladie du sinus** (evidence D).
  - Effets : `sa_block` + 0,45·s ; FC sinusale × (1 − 0,3·s) ; échappements jonctionnel et ventriculaire × (1 − 0,8·s).
  - Pause la plus longue : 4,9 s sur 5 h à s = 0,85, contre 6,9 s (5,5–10,3) sur Holter de 24 h (10.1111/jvim.17071).
- **`esc_overdrive`** = 1,3 (placeholder D) : suppression des échappements par entraînement. Sans elle, l'échappement jonctionnel s'insérait dans chaque pause de Wenckebach.
