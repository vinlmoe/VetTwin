# Conformité du module auscultation

Généré par `tests/pcg_acceptance.py` ; mesures par `reference/python/pcg_metrics.py`. Source : moteur Rust (target/release/vetsim-cli).

## Méthode
- Synthèse : `vetsim-cli pcg` (Rust, `pcg::patient_pcg` dans `crates/vetsim-core/src/pcg.rs`). Elle se fait battement par battement : décharges sinusales du modèle autonome, conduction, hémodynamique simulée battement par battement après 5 s de chauffe, puis B1, B2, B3 et souffle de chaque battement. C'est le port de `reference/python/pcg_beats.py`, qui reproduit la référence à 1e-9 près avec les mêmes tirages (`cargo test --test pcg_beats`).
- Référence Python (`--python`) : même chaîne (`pcg_beats.signal_beats`, `make_pcg_beats`). Jusqu'au 17/11/2026, la référence était l'ancienne synthèse périodique (`signals_pcg_v5.make_pcg`, un battement simulé posé à chaque instant). Elle comparait donc deux synthèses différentes : à FC 72, l'arythmie sinusale respiratoire ample fait varier la systole d'un battement à l'autre, et l'équivalence du rapport diastole/systole échouait (Rust 1,716 contre 1,825 ; z = 3,2). L'ancienne synthèse reste disponible avec `--v5`.
- Mesures : `reference/python/pcg_metrics.py` sur le WAV 16 bits produit. Chaque critère est jugé sur la médiane des graines. La diastole est le RR médian moins l'intervalle B1→B2 médian.
- Fréquence cardiaque : le paramètre `hr` est imposé avant l'évaluation (72 et 90/min). C'est un réglage d'usage, pas la valeur calibrée du chien sain (87/min).
- **300 graines** (référence `tests/pcg_reference_python.json` et CI ; 100 auparavant). À 100 graines, le rapport B2/B1 à FC 72 n'était pas équivalent (z = 3,1 sur environ 3 paires par graine). À 300, toutes les métriques le sont.
- **Pourquoi plus de 20 graines.** `pcg_metrics` n'apparie B1 et B2 que si leur intervalle est inférieur à 0,3 s. À FC 72, la systole du modèle (0,33 s) dépasse ce seuil. On obtient alors environ 1 paire par graine, et 35 à 40 % des graines n'en ont aucune. À 20 graines, les métriques B2/B1 dépendent du hasard. Constaté : le Rust et la référence divergent à 20 graines (rapport B2/B1 à FC 72 : 1,01 contre 1,63) et convergent à 100 (1,59 contre 1,62).
- Statut « écart connu (référence) » : le critère n'est pas atteint par la référence Python non plus. Décision du projet : port « tel quel ».
- Statut « à la limite du seuil » : la référence passe le seuil et le Rust non, alors que les deux valeurs sont statistiquement équivalentes (règle ci-dessous). Le modèle est au seuil ; ce n'est ni un succès net ni un écart de port, et cela ne fait pas échouer. Cas actuel : diastole/systole à FC 72 (Rust 1,774, référence 1,803, seuil 1,8).
- Statut « non mesurable » : plus de 30 % des graines n'ont aucune paire B1/B2 ; les critères B2/B1 ne sont alors pas évalués (même règle pour la référence et le Rust).
- Équivalence Rust/référence : écart relatif < 5 % ou |écart| < 3 erreurs-types combinées de médianes (1,2533 σ/√n). Les graines 0 à 99 servent à tous les cas, donc les cas ne sont pas indépendants. Vérification sur les graines 1000 à 1099 du Rust : SD RR 175 / 163 / 167 ms contre 177 / 163 / 166 pour la référence, énergie > 200 Hz à FC 90 de 1,04 % contre 1,06 % (z = 1,5).

## Écarts connus et leur cause
1. **B1→B2 trop long à FC 90 (0,26 s pour une cible de 0,15 à 0,25 s ; 0,18 s mesuré sur 20 enregistrements réels, `docs/VERIFICATION_PCG.md`), d'où un rapport diastole/systole trop faible (cible > 1,8).** Cause : l'hémodynamique, pas le module audio. La durée de contraction `ts = 0,37 √T` donne une fermeture aortique 0,29 s après la fermeture mitrale à 90/min. Le prompt indiquait déjà 0,21 s dans le modèle contre 0,18 s mesuré.
2. B2 > B1 à FC 90 : 86 % des paires depuis la version 1.1.0 (cible ≥ 80 % atteinte ; 75 % puis 83 % auparavant). Les gains du site « base » restent calés sur un seul enregistrement.
3. **Métriques B2/B1 à FC 72 peu fiables**, pour la raison d'appariement décrite plus haut.
4. Souffle (MMVD) : le centroïde et l'énergie au-dessus de 100 Hz sont plus élevés que chez le sain. Les critères spectraux ne s'appliquent qu'au chien sain ; les lignes MMVD sont indicatives. Depuis la version 1.1.0, le niveau du souffle est calé par grade sur des enregistrements réels (`docs/VERIFICATION_PCG.md`) ; l'ancienne mention d'un souffle réel « plus grave et moins plateau » n'est pas confirmée par les mesures par bande.

## Limites de validité
- Calage sur huit enregistrements de la bibliothèque TiHo (trois sans souffle, cinq de régurgitation mitrale, un chien par grade), sous licence CC BY-NC-ND 4.0 ; mesures dans `docs/VERIFICATION_PCG.md`. Aucun enregistrement n'est copié ni embarqué ; seuls les chiffres mesurés sont utilisés.
- Le site « apex » est calé sur le rapport B2/B1 de cinq chiens à régurgitation mitrale (evidence C). L'attribution de l'enregistrement normal au site « base » est à revoir.
- Les seuils de grade du souffle ne sont pas calibrés.
- Le garde-fou `t_ac ≥ t_mc + 0,7 ts` compense une éjection trop courte en régurgitation mitrale sévère. C'est une rustine du module audio, pas une correction de l'hémodynamique.

| Cas | Métrique | Cible | Obtenu (médiane de 300 graines) | Référence Python | Statut |
|---|---|---|---|---|---|
| sain FC 72 | Centroïde spectral (Hz) | 90 ± 15 | 88.957 | 88.903 | OK |
| sain FC 72 | Énergie 20–50 Hz | ≤ 8 % | 4.9 % | 4.9 % | OK |
| sain FC 72 | Énergie 50–100 Hz | 60–75 % | 66.1 % | 65.8 % | OK |
| sain FC 72 | Énergie 100–200 Hz | 20–35 % | 28.6 % | 28.8 % | OK |
| sain FC 72 | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 0.4 % | 0.4 % | OK |
| sain FC 72 | B2 > B1 (part des paires) | ≥ 80 % | 1.000 | 1.000 | OK |
| sain FC 72 | Rapport B2/B1 médian | 1,5–2,3 | 1.878 | 1.902 | OK |
| sain FC 72 | Diastole / systole | > 1,8 | 1.774 | 1.803 | à la limite du seuil (Rust équivalent à la référence, de part et d'autre) |
| sain FC 72 | Informations |  | paires B1/B2 par graine : médiane 5, graines sans paire 0 ; FC 71.2/min, SD RR 187 ms, grade [0], souffle ['aucun'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |
| sain FC 90 | Centroïde spectral (Hz) | 90 ± 15 | 88.016 | 87.960 | OK |
| sain FC 90 | Énergie 20–50 Hz | ≤ 8 % | 5.1 % | 5.1 % | OK |
| sain FC 90 | Énergie 50–100 Hz | 60–75 % | 66.8 % | 66.6 % | OK |
| sain FC 90 | Énergie 100–200 Hz | 20–35 % | 27.6 % | 27.9 % | OK |
| sain FC 90 | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 0.4 % | 0.4 % | OK |
| sain FC 90 | B2 > B1 (part des paires) | ≥ 80 % | 85.7 % | 85.7 % | OK |
| sain FC 90 | Rapport B2/B1 médian | 1,5–2,3 | 1.608 | 1.604 | OK |
| sain FC 90 | B1→B2 médian (s), FC 90 | 0,15–0,25 | 0.258 | 0.259 | écart connu (référence) |
| sain FC 90 | Diastole / systole | > 1,8 | 1.328 | 1.306 | écart connu (référence) |
| sain FC 90 | Informations |  | paires B1/B2 par graine : médiane 7, graines sans paire 0 ; FC 90.8/min, SD RR 151 ms, grade [0], souffle ['aucun'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |
| MMVD 0,3 (léger) | Centroïde spectral (Hz) | 90 ± 15 | 90.138 | 90.118 | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 20–50 Hz | ≤ 8 % | 5.2 % | 5.1 % | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 50–100 Hz | 60–75 % | 65.2 % | 65.0 % | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 100–200 Hz | 20–35 % | 28.7 % | 28.5 % | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 1.0 % | 1.0 % | (dans la cible) |
| MMVD 0,3 (léger) | B2 > B1 (part des paires) | ≥ 80 % | 85.7 % | 85.7 % | (dans la cible) |
| MMVD 0,3 (léger) | Rapport B2/B1 médian | 1,5–2,3 | 1.603 | 1.560 | (dans la cible) |
| MMVD 0,3 (léger) | Diastole / systole | > 1,8 | 1.410 | 1.393 | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Informations |  | paires B1/B2 par graine : médiane 8, graines sans paire 0 ; FC 88.7/min, SD RR 149 ms, grade [3], souffle ['holosystolique apexien gauche'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |
| MMVD 0,5 (modéré) | Centroïde spectral (Hz) | 90 ± 15 | 112.266 | 112.730 | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 20–50 Hz | ≤ 8 % | 3.7 % | 3.7 % | (dans la cible) |
| MMVD 0,5 (modéré) | Énergie 50–100 Hz | 60–75 % | 49.8 % | 49.6 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 100–200 Hz | 20–35 % | 38.9 % | 38.9 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 7.2 % | 7.3 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | B2 > B1 (part des paires) | ≥ 80 % | 77.8 % | 77.8 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Rapport B2/B1 médian | 1,5–2,3 | 1.508 | 1.488 | (dans la cible) |
| MMVD 0,5 (modéré) | Diastole / systole | > 1,8 | 1.934 | 1.934 | (dans la cible) |
| MMVD 0,5 (modéré) | Informations |  | paires B1/B2 par graine : médiane 10, graines sans paire 0 ; FC 97.9/min, SD RR 114 ms, grade [6], souffle ['holosystolique apexien gauche'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |

## Équivalence statistique avec la référence Python

```
OK  sain FC 72         Centroïde spectral (Hz)      Rust   88.9573  référence   88.9027  écart   0.1 %  z  0.3
OK  sain FC 72         Énergie 20–50 Hz             Rust    0.0493  référence    0.0490  écart   0.6 %  z  0.3
OK  sain FC 72         Énergie 50–100 Hz            Rust    0.6605  référence    0.6579  écart   0.4 %  z  0.8
OK  sain FC 72         Énergie 100–200 Hz           Rust    0.2864  référence    0.2881  écart   0.6 %  z  0.5
OK  sain FC 72         Énergie 200–400 Hz           Rust    0.0043  référence    0.0041  écart   4.4 %  z  1.7
OK  sain FC 72         B2 > B1 (part des paires)    Rust    1.0000  référence    1.0000  écart   0.0 %  z  0.0
OK  sain FC 72         Rapport B2/B1 médian         Rust    1.8781  référence    1.9016  écart   1.2 %  z  0.5
OK  sain FC 72         Diastole / systole           Rust    1.7737  référence    1.8030  écart   1.6 %  z  0.8
OK  sain FC 90         Centroïde spectral (Hz)      Rust   88.0158  référence   87.9599  écart   0.1 %  z  0.3
OK  sain FC 90         Énergie 20–50 Hz             Rust    0.0514  référence    0.0513  écart   0.4 %  z  0.2
OK  sain FC 90         Énergie 50–100 Hz            Rust    0.6683  référence    0.6662  écart   0.3 %  z  0.7
OK  sain FC 90         Énergie 100–200 Hz           Rust    0.2765  référence    0.2790  écart   0.9 %  z  0.9
OK  sain FC 90         Énergie 200–400 Hz           Rust    0.0041  référence    0.0040  écart   0.9 %  z  0.4
OK  sain FC 90         B2 > B1 (part des paires)    Rust    0.8571  référence    0.8571  écart   0.0 %  z  0.0
OK  sain FC 90         Rapport B2/B1 médian         Rust    1.6082  référence    1.6040  écart   0.3 %  z  0.1
OK  sain FC 90         B1→B2 médian (s), FC 90      Rust    0.2578  référence    0.2588  écart   0.4 %  z  1.4
OK  sain FC 90         Diastole / systole           Rust    1.3275  référence    1.3057  écart   1.7 %  z  0.8
OK  MMVD 0,3 (léger)   Centroïde spectral (Hz)      Rust   90.1384  référence   90.1176  écart   0.0 %  z  0.1
OK  MMVD 0,3 (léger)   Énergie 20–50 Hz             Rust    0.0520  référence    0.0514  écart   1.1 %  z  0.7
OK  MMVD 0,3 (léger)   Énergie 50–100 Hz            Rust    0.6521  référence    0.6498  écart   0.3 %  z  0.8
OK  MMVD 0,3 (léger)   Énergie 100–200 Hz           Rust    0.2865  référence    0.2855  écart   0.4 %  z  0.4
OK  MMVD 0,3 (léger)   Énergie 200–400 Hz           Rust    0.0103  référence    0.0104  écart   0.6 %  z  0.4
OK  MMVD 0,3 (léger)   B2 > B1 (part des paires)    Rust    0.8571  référence    0.8571  écart   0.0 %  z  0.0
OK  MMVD 0,3 (léger)   Rapport B2/B1 médian         Rust    1.6026  référence    1.5603  écart   2.7 %  z  1.3
OK  MMVD 0,3 (léger)   Diastole / systole           Rust    1.4102  référence    1.3934  écart   1.2 %  z  0.6
OK  MMVD 0,5 (modéré)  Centroïde spectral (Hz)      Rust  112.2665  référence  112.7297  écart   0.4 %  z  1.5
OK  MMVD 0,5 (modéré)  Énergie 20–50 Hz             Rust    0.0371  référence    0.0374  écart   0.7 %  z  0.4
OK  MMVD 0,5 (modéré)  Énergie 50–100 Hz            Rust    0.4979  référence    0.4960  écart   0.4 %  z  0.6
OK  MMVD 0,5 (modéré)  Énergie 100–200 Hz           Rust    0.3888  référence    0.3891  écart   0.1 %  z  0.1
OK  MMVD 0,5 (modéré)  Énergie 200–400 Hz           Rust    0.0719  référence    0.0725  écart   0.8 %  z  0.7
OK  MMVD 0,5 (modéré)  B2 > B1 (part des paires)    Rust    0.7778  référence    0.7778  écart   0.0 %  z  0.0
OK  MMVD 0,5 (modéré)  Rapport B2/B1 médian         Rust    1.5077  référence    1.4877  écart   1.3 %  z  0.7
OK  MMVD 0,5 (modéré)  Diastole / systole           Rust    1.9342  référence    1.9341  écart   0.0 %  z  0.0
```
