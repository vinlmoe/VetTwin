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
| PK furosémide | RCP Noah; EMA 1999; BMC Vet Res 2021 | Vss 0,52 L/kg, t½ IV 1–1,5 h, Cmax per os | Biodisponibilité ajustée (0,39) pour reproduire Cmax; sources en désaccord |
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
- Placeholders D : `af_cycle`, `af_cv`, `hv`, `av_beta`, `av_tau`, `av_vagal`, `av_k_gain`, `hp_k_slope`, `esc_k_gain`, `esc_sup_tau`, `vpc_k_gain`, `k_vpc`.
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
- **Échappements et ASR (18/11/2026, evidence C, sources lues en résumé sauf mention).**
  - `esc_overdrive` 1,3 est remplacé par une suppression par entraînement mécanistique : premier cycle × (1 + `esc_sup_gain`·max(0, cycle·f̄ − 1)), où f̄ est la fréquence ventriculaire moyennée sur `esc_sup_tau`.
    - `esc_sup_gain` 1,8 : pause de 4,7 ± 1,1 s après un entraînement à 2,5× (Tanahashi 1979, 10.1016/s0022-0736(79)80048-5).
    - `esc_sup_tau` 60 s, placeholder D : Krellenstein 1978 (10.1016/s0022-0736(78)80022-3) et Neely 1985 (10.1161/01.res.57.1.182).
    - Résultat : le premier échappement jonctionnel survient 3,3 s après un arrêt sinusal chez le chien sain, dans l'ordre des pauses Holter de Flanders 2024 (PMC11099791, lu).
  - Modulation autonome des foyers, Tanahashi 1979 :
    - `esc_a_v` 0,4 : effet de l'atropine sur le foyer jonctionnel ;
    - `esc_a_s` 2,0 : sympathique, sur les foyers jonctionnel et ventriculaire ; une partie seulement de l'effet du practolol est attribuée à la fréquence, et un gain plus fort déstabilise le point fixe du baroréflexe.
  - `junct_rate` 47 /min au tonus de repos : 37 ± 9 sous cholinergique local maximal (Rosenshtraukh 1986, 10.1016/s0735-1097(86)80035-3), ramené au repos par `esc_a_v`. La valeur de 40 envisagée passait sous l'échappement ventriculaire (42).
  - `rsa_depth` 0,95 → 0,64 : ASR 40,1 ± 4,5 % de la FC moyenne (Grosso 2021, 10.1016/j.resp.2021.103776) ; le modèle passait de 56 % à 40 %.
    - L'ASR est déjà proportionnelle au tonus vagal effectif. En % de la FC, elle baisse avec la MMVD (sain 40 %, B2 37 %, C 27 %, D 18 %, dans le sens de Baisan 2024) et disparaît sous atropine.
    - RMSSD 165 ms, dans 259 ± 120 (Bogucki 2015). LF/HF redevient conforme côté Rust.

## Corrections du 18/11/2026 (maladies × médicaments × surdosage)
Détail et sources : `docs/VERIFICATION_MALADIES_MEDICAMENTS.tex`. Accès : « lu » = texte intégral (PMC), « résumé » = résumé seul.
- **Furosémide** (`modules/drugs/furosemide.json` 1.2.0, evidence B).
  - PK : Koh 2021 (lu, PMC8420016) ; CL 735,7 mL/h/kg à ≈ 11,7 kg ramenée à 20 kg (10,724 mL/min/kg), Vc 0,2073, Vp 0,4602 L/kg, Q 3,341 mL/min/kg, ka 0,724 /h, F 0,55.
  - Diurèse : `urine_max` 0,2044 mL/kg/min, EC50 0,2412 mg/L, Hill 3 (borne ; libre ≈ 5), ajustés sur les volumes urinaires de Koh par intervalle (IV et PO).
  - Électrolytes : `X ×= exp(−a·U_eff)`, aK 0,00585, aCl 0,00225, aNa 0,00032 (mL/kg)⁻¹, τ_elect 200 h, adaptation W (τ 12 h placeholder D, φ 0,9808) ; Koh 2021 (lu), Adin 2020 (PMC6979094, lu : stade C 3,5 mg/kg/j, K 4,50 contre 4,70), TEST 2017 (PMC5697186, lu : K 4,50 à J84). Wong 1977 (résumé) n'est plus reproduit.
  - Veinodilatation `ven_cap ×(1 + 0,36 E)` : Bayne & Williamson 1979 (résumé : compliance veineuse 120 % à 10 min, 136 % à 60 min, chien aux uretères ligaturés ; nulle chez le néphrectomisé).
  - `drive: urinary_excretion` : Miyazaki 1990, Hirai 1992 (résumés).
- **Opioïde** (1.1.0, evidence C) : Mills 2025 (lu), Amon 2021 (lu), Wunsch 2010 (lu), Maiante 2009, Garofalo 2012, Bailey 1987, Arndt 1984 (résumés). Terme toxique : EC50 1 mg/L (placeholder D borné par Maiante et Bailey).
- **Bêtabloquant** (1.1.0) : profil aténolol, terme toxique de stabilisation de membrane retiré.
- **Patient** : `creat_vd` 0,539 L/kg (Collignon 2012, résumé, n = 50) ; `urea_vd` 0,66 L/kg (Bowsher 1985, résumé) ; `map_arrest` 40 mmHg (Morales 1999, Granot 1985, résumés ; evidence C) ; `av_symp` 0,5 (PMC7872775, lu ; Rigel 1990, résumé ; evidence C).
- **Orifice mitral dynamique** (`mr_k` 2,5, `mr_vref` 72·s mL ; evidence C) : forme d'après Yoran 1979 (Circulation 60:170, 6 chiens, résumé) ; exposant calé sur Suzuki 2013 (résumé : POG 14,75 → 10,77 sous furosémide 0,17 mg/kg/h × 6 h). Coefficients MMVD de `g_mr` recalés en conséquence (0,9458·s^1,111) pour conserver la fraction régurgitée et la POG par stade.
- **MMVD, congestion par stade** (`mmvd.json` 1.1.0, 18/11/2026) : rétention hydrosodée portée de 0,149 à 0,30 (`bvf`), fuites valvulaires normalisées à 20 kg ((bw/20)^0,75). POG au milieu du stade C 15,2 mmHg (12,1 avant), stade C 9,8–22,8, stade D jusqu'à 37. Repères : POG invasive en ICC (stades C/D) médiane 14 mmHg (8–32), Allen 2021, JAVMA 258:638 (résumé) ; IM expérimentale B2 14,6 ± 3,8 (Suzuki 2012, PMC3489586, lu) ; œdème expérimental aigu au-delà de 20–25 mmHg (Nakata 1986 ; Magno & Szidon 1976 ; résumés). Cibles du module : C 14–24, D 20–32.
