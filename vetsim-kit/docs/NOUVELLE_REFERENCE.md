# Nouvelle référence : conduction, oreillettes contractiles, ECG et sons depuis la conduction

Date : 17/11/2026. Décision de l'utilisateur : « nouvelle référence ». L'oreillette contractile ne
pouvait pas tenir dans les tolérances de l'ancien golden : dès 0,8 % de contribution atriale,
5 valeurs sortaient.

## Ce qui change dans le modèle (référence Python, port Rust)

1. **Oreillettes contractiles.** Élastance active sur `pv` (OG) et `sv` (OD), déclenchée à chaque
   activation atriale organisée. `k_la` = 1,45, calé sur E/A ≈ 1,39 (E 0,82, A 0,59 m/s, n = 53 ;
   10.3390/ani15111524). `k_ra` = 0,2·`k_la` (placeholder D).
2. **Couche de conduction (`rhythm`) branchée au moteur.**
   - Rythme sinusal 1:1 : simulation périodique, avec le PR de la conduction.
   - Autre rythme : séquence de conduction sur 30 s, puis hémodynamique battement par battement
     (moyennes après 10 s).
   - Aléa : SplitMix64 à graine fixe, identique au bit près en Python et en Rust.
3. **`kfac` supprimé.** K⁺ agit sur la conduction : allongement nodal, arrêt atrial à 8,5 mmol/L,
   bloc infranodal au-delà de 10. Il ne freine plus directement la FC.
4. **FC rapportée = FC ventriculaire.** La FC sinusale reste disponible (`hr_sinus`, `sinus_hr`).
5. **ECG construit depuis la conduction.**
   - Onde P à chaque activation atriale organisée, aucune en arrêt atrial ; ondes f en FA.
   - PR propre à chaque battement.
   - Battement ventriculaire : QRS large et onde T discordante.
   - QT corrigé par le RR précédent et raccourci en hyperkaliémie (placeholder D).
   - Échelle unique de l'ECG dans les interfaces (−1,5 à +2,5 mV).
6. **Sons.** Les battements du PCG sont les activations ventriculaires de la même conduction
   (mêmes battements que l'ECG). Le PR varie avec l'arythmie respiratoire (récupération nodale).

## Vérifications

- Golden régénéré depuis la référence Python : Rust 962/962 ; paramètres 6213/6213.
- Calibration régénérée : 164/164 lignes Rust = Python. La cohorte saine reste dans toutes les
  cibles (FC 86,9 ; PAM 106,1 ; DC 3,43 ; PAP 13,8 ; POG 4,97 ; gaz du sang ; VFC).
- Décès mécanistiques comptés à part : MMVD stade D, Python 2/24 et Rust 3/96 (dette d'O₂,
  spirale).
- Fixtures M1 à M6, décompensation, conduction et battements régénérées : Rust = Python.
- Tests modifiés, et pourquoi :
  - **Fenêtre « souffle visible ».** Elle contenait la fin de B1 et, sur les RR courts, A2. Elle
    est remplacée par une fenêtre propre à chaque battement (loi de la synthèse). Rapport
    souffle/sain : 7,7.
  - **API.**
    - Signe d'œdème : testé « si et seulement si POG > 20 ».
    - Préréglage de FC : accepté à ± 2/min, car le baroréflexe ajuste la FC intrinsèque.
    - Ischémie coronaire : testée sur le pimobendane à 3 mg/L.
  - **Battements du son et ondes R.** Ce sont désormais la décharge sinusale + PR, et non la
    décharge elle-même. Le PR varie en sens inverse du RR précédent.
  - **Disparition de P.** Décidée par la conduction (K ≥ 8,5), et non plus par le dessin
    (K > 7,8).

## Avant → après (golden, cas « eval » ; en gras : écart ≥ 1 %)

| Cas | hr | map | co | sv | edv | la_mean | lv_edp | reg_fraction | pao2 | k | ph | creat |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| healthy_5kg | **132→126** | **98.9→103** | **1.09→1.16** | **8.17→9.17** | **15.2→16.6** | **6.73→5.7** | **1.35→1.66** | 0→0 | 94.9 | 4.1 | 7.35 | **85.7→75.2** |
| healthy_20kg | **87.6→86.5** | **101→106** | **3.25→3.41** | **36.8→39.2** | **66.7→70.4** | **5.48→4.93** | **1.63→1.79** | 0→0 | 98.9 | 4.1 | 7.38 | **77→75** |
| healthy_40kg | 73.2 | **102→107** | **5.54→5.83** | **75.3→79.2** | **136→143** | **5.12→4.72** | **1.7→1.84** | 0→0 | 101 | 4.1 | 7.39 | 75 |
| mmvd_s0.2 | 86.4 | **103→108** | **3.31→3.5** | **40.6→43.3** | **71.3→75.3** | **5.88→5.35** | **1.66→1.87** | 7→7 | 98.9 | 4.1 | 7.38 | 75 |
| mmvd_s0.5 | **97.8→95.8** | **103→108** | **3.33→3.53** | **60.9→66.4** | **86.5→93** | **9.63→8.79** | **2.01→2.37** | 45→45 | 98.9 | 4.1 | 7.38 | 75 |
| mmvd_s0.8 | **136→120** | **106→110** | **3.44→3.81** | **85→104** | **105→124** | **20.6→17.4** | **2.3→3.48** | 70→70 | **88.6→98.9** | 4.1 | 7.38 | 75 |
| ckd_s0.2 | **87.6→86.5** | **101→106** | **3.25→3.41** | **36.8→39.2** | **66.7→70.4** | **5.48→4.93** | **1.63→1.79** | 0→0 | 98.9 | 4.1 | 7.38 | **124→121** |
| ckd_s0.5 | **87.6→86.5** | **101→106** | **3.25→3.41** | **36.8→39.2** | **66.7→70.4** | **5.48→4.93** | **1.63→1.79** | 0→0 | 99.5 | 4.1 | 7.36 | **256→249** |
| ckd_s0.8 | **87.6→86.5** | **101→106** | **3.25→3.41** | **36.8→39.2** | **66.7→70.4** | **5.48→4.93** | **1.63→1.79** | 0→0 | 101 | 4.1 | 7.32 | **525→512** |
| addison_s0.2 | **98→92.6** | 96.9 | **3.14→3.22** | **31.7→34.6** | **59.1→62.8** | **5.34→4.57** | **1.35→1.48** | 0→0 | 99 | 4.78 | 7.38 | **82.7→78.1** |
| addison_s0.5 | **120→113** | **87.9→89** | **2.88→3** | **23→26.1** | **46.5→50.5** | **5.19→4.22** | **0.927→1.06** | 0→0 | 99.2 | 5.8 | 7.37 | **100→91.3** |
| addison_s0.8 | **110→141** | **77.8→81** | **2.59→2.73** | **22.9→18** | **43.3→38.8** | **4.44→4.12** | **0.826→0.728** | 0→0 | 99.4 | 6.82 | 7.36 | **131→113** |
| pneumonia_s0.2 | **87.6→86.5** | **101→106** | **3.25→3.41** | **36.8→39.2** | **66.7→70.4** | **5.48→4.93** | **1.63→1.79** | 0→0 | 90.1 | 4.1 | 7.38 | **77→75** |
| pneumonia_s0.5 | **87.6→86.5** | **101→106** | **3.25→3.41** | **36.8→39.2** | **66.7→70.4** | **5.48→4.93** | **1.63→1.79** | 0→0 | 70.1 | 4.1 | 7.37 | **77→75** |
| pneumonia_s0.8 | **87.6→86.5** | **101→106** | **3.25→3.41** | **36.8→39.2** | **66.7→70.4** | **5.48→4.93** | **1.63→1.79** | 0→0 | 50.3 | 4.1 | 7.4 | **77→75** |
| mmvd_s0.8_pimobendan | **142→125** | **104→109** | **3.65→4.09** | **81→102** | **97.4→118** | **21.2→17.6** | **2.04→3.16** | 68→68 | **85.9→98.9** | 4.1 | 7.38 | 75 |
| healthy_pimobendan | **91.7→88.1** | **99.2→101** | **3.46→3.58** | **37.6→40.5** | **62.8→66.6** | **5.74→5.06** | **1.48→1.64** | 0→0 | 98.9 | 4.1 | 7.38 | 75 |
| healthy_atropine | **121→117** | **103→108** | **3.36→3.59** | **27.1→30** | **56.7→61.3** | **6.24→5.14** | **1.22→1.42** | 0→0 | 98.9 | 4.1 | 7.38 | 75 |
| healthy_betabloquant | **87.5→84.2** | **103→104** | **3.26→3.35** | **37→39.6** | **68→71.5** | **5.53→4.88** | **1.67→1.85** | 0→0 | 98.9 | 4.1 | 7.38 | **76.5→75** |
| healthy_opioide | **73.1→71.1** | **93.9→95.6** | **2.99→3.07** | **40.8→43.1** | **69.5→72.6** | **5.03→4.6** | **1.76→1.89** | 0→0 | 95.2 | 4.1 | 7.35 | **91.5→86.3** |
| healthy_furosemide_volume | **105→98.7** | 93.3 | **2.82→2.9** | **26.2→29** | **52→55.5** | **4.87→4.08** | **1.11→1.21** | 0→0 | 98.7 | 4.02 | 7.38 | **105→98.5** |


## Écarts à signaler

- **MMVD stade C moins congestif.**
  - La pompe atriale abaisse la POG : à s = 0,8, elle passe de 20,6 à 17,4 mmHg, et la PaO₂ de
    88,6 à 98,9.
  - Le stade C est défini par l'insuffisance cardiaque congestive, or l'œdème n'apparaît plus
    qu'au-delà de s ≈ 0,85.
  - À recaler (module MMVD) sur la littérature (Yin 2026 : FC 150, FR 64 en ICC).
- **Addison sans frein `kfac`.**
  - FC : 110 → 141 à s = 0,8 (K 6,8) ; 92 → 162 à s = 1 (K 7,5). Aucune bradycardie sous le
    seuil d'arrêt atrial.
  - Cohérent avec 9 à 21 % de chiens bradycardes en crise (Chalifoux 2023), mais la forte
    tachycardie compensatrice reste à vérifier.
- **Chien sain un peu haut** : PAM 106 (Haskins : 103) ; index cardiaque ≈ 4,58 (cible 4,42).
- **ECG de l'Addison.**
  - T ample, QT court et P aplatie : conformes à la revue (figure adaptée de l'humain).
  - P disparaît à 8,5 mmol/L (Hariman 1983, chien).
