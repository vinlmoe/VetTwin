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
- Statut « non mesurable » : plus de 30 % des graines n'ont aucune paire B1/B2 ; les critères B2/B1 ne sont alors pas évalués (même règle pour la référence et le Rust).
- Équivalence Rust/référence : écart relatif < 5 % ou |écart| < 3 erreurs-types combinées de médianes (1,2533 σ/√n). Les graines 0 à 99 servent à tous les cas, donc les cas ne sont pas indépendants. Vérification sur les graines 1000 à 1099 du Rust : SD RR 175 / 163 / 167 ms contre 177 / 163 / 166 pour la référence, énergie > 200 Hz à FC 90 de 1,04 % contre 1,06 % (z = 1,5).

## Écarts connus et leur cause
1. **B1→B2 trop long à FC 90 (0,27 s pour une cible de 0,15 à 0,25 s), d'où un rapport diastole/systole trop faible (cible > 1,8).** Cause : l'hémodynamique, pas le module audio. La durée de contraction `ts = 0,37 √T` donne une fermeture aortique 0,29 s après la fermeture mitrale à 90/min. Le prompt indiquait déjà 0,21 s dans le modèle contre 0,18 s mesuré.
2. **B2 > B1 dans 75 % des paires seulement à FC 90 (cible ≥ 80 %)**, dans la référence comme dans le Rust. Les gains de site sont calés sur un seul enregistrement.
3. **Métriques B2/B1 à FC 72 peu fiables**, pour la raison d'appariement décrite plus haut.
4. Souffle (MMVD) : le centroïde et l'énergie au-dessus de 100 Hz sont plus élevés que chez le sain. Les critères spectraux ne s'appliquent qu'au chien sain ; les lignes MMVD sont indicatives. Le prompt note que le souffle réel est plus grave et moins « plateau » que le modèle.

## Limites de validité
- Calage sur **deux** enregistrements de la bibliothèque TiHo (un rythme normal, un souffle mitral II/VI), sous licence CC BY-NC-ND 4.0. Aucun enregistrement n'est copié ni embarqué ; seuls les chiffres mesurés sont utilisés.
- Le site « apex » est un placeholder (evidence D) : hypothèse de symétrie, à calibrer.
- Les seuils de grade du souffle ne sont pas calibrés.
- Le garde-fou `t_ac ≥ t_mc + 0,7 ts` compense une éjection trop courte en régurgitation mitrale sévère. C'est une rustine du module audio, pas une correction de l'hémodynamique.

| Cas | Métrique | Cible | Obtenu (médiane de 300 graines) | Référence Python | Statut |
|---|---|---|---|---|---|
| sain FC 72 | Centroïde spectral (Hz) | 90 ± 15 | 92.554 | 92.592 | OK |
| sain FC 72 | Énergie 20–50 Hz | ≤ 8 % | 4.1 % | 4.2 % | OK |
| sain FC 72 | Énergie 50–100 Hz | 60–75 % | 64.1 % | 64.2 % | OK |
| sain FC 72 | Énergie 100–200 Hz | 20–35 % | 30.4 % | 30.3 % | OK |
| sain FC 72 | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 1.2 % | 1.2 % | OK |
| sain FC 72 | B2 > B1 (part des paires) | ≥ 80 % | 1.000 | 1.000 | OK |
| sain FC 72 | Rapport B2/B1 médian | 1,5–2,3 | 2.090 | 2.055 | OK |
| sain FC 72 | Diastole / systole | > 1,8 | 1.716 | 1.745 | écart connu (référence) |
| sain FC 72 | Informations |  | paires B1/B2 par graine : médiane 3, graines sans paire 10 ; FC 71.2/min, SD RR 187 ms, grade [0], souffle ['aucun'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |
| sain FC 90 | Centroïde spectral (Hz) | 90 ± 15 | 91.258 | 91.380 | OK |
| sain FC 90 | Énergie 20–50 Hz | ≤ 8 % | 4.3 % | 4.3 % | OK |
| sain FC 90 | Énergie 50–100 Hz | 60–75 % | 64.9 % | 64.9 % | OK |
| sain FC 90 | Énergie 100–200 Hz | 20–35 % | 29.7 % | 29.6 % | OK |
| sain FC 90 | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 0.9 % | 0.9 % | OK |
| sain FC 90 | B2 > B1 (part des paires) | ≥ 80 % | 83.3 % | 83.3 % | OK |
| sain FC 90 | Rapport B2/B1 médian | 1,5–2,3 | 1.699 | 1.711 | OK |
| sain FC 90 | B1→B2 médian (s), FC 90 | 0,15–0,25 | 0.267 | 0.267 | écart connu (référence) |
| sain FC 90 | Diastole / systole | > 1,8 | 1.249 | 1.234 | écart connu (référence) |
| sain FC 90 | Informations |  | paires B1/B2 par graine : médiane 6, graines sans paire 0 ; FC 90.8/min, SD RR 151 ms, grade [0], souffle ['aucun'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |
| MMVD 0,3 (léger) | Centroïde spectral (Hz) | 90 ± 15 | 112.302 | 113.001 | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Énergie 20–50 Hz | ≤ 8 % | 3.2 % | 3.2 % | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 50–100 Hz | 60–75 % | 51.8 % | 51.5 % | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Énergie 100–200 Hz | 20–35 % | 35.4 % | 35.6 % | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 9.1 % | 9.3 % | (hors cible, indicatif) |
| MMVD 0,3 (léger) | B2 > B1 (part des paires) | ≥ 80 % | 62.5 % | 60.0 % | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Rapport B2/B1 médian | 1,5–2,3 | 1.411 | 1.342 | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Diastole / systole | > 1,8 | 1.430 | 1.402 | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Informations |  | paires B1/B2 par graine : médiane 7, graines sans paire 0 ; FC 88.7/min, SD RR 149 ms, grade [3], souffle ['holosystolique apexien gauche'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |
| MMVD 0,5 (modéré) | Centroïde spectral (Hz) | 90 ± 15 | 133.133 | 133.790 | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 20–50 Hz | ≤ 8 % | 2.2 % | 2.2 % | (dans la cible) |
| MMVD 0,5 (modéré) | Énergie 50–100 Hz | 60–75 % | 39.4 % | 38.5 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 100–200 Hz | 20–35 % | 40.8 % | 41.1 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 17.3 % | 17.6 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | B2 > B1 (part des paires) | ≥ 80 % | 81.8 % | 81.8 % | (dans la cible) |
| MMVD 0,5 (modéré) | Rapport B2/B1 médian | 1,5–2,3 | 1.650 | 1.635 | (dans la cible) |
| MMVD 0,5 (modéré) | Diastole / systole | > 1,8 | 2.929 | 3.079 | (dans la cible) |
| MMVD 0,5 (modéré) | Informations |  | paires B1/B2 par graine : médiane 10, graines sans paire 0 ; FC 97.9/min, SD RR 114 ms, grade [6], souffle ['holosystolique apexien gauche'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |

## Équivalence statistique avec la référence Python

```
OK  sain FC 72         Centroïde spectral (Hz)      Rust   92.5536  référence   92.5917  écart   0.0 %  z  0.2
OK  sain FC 72         Énergie 20–50 Hz             Rust    0.0409  référence    0.0418  écart   2.2 %  z  1.1
OK  sain FC 72         Énergie 50–100 Hz            Rust    0.6405  référence    0.6422  écart   0.3 %  z  0.4
OK  sain FC 72         Énergie 100–200 Hz           Rust    0.3036  référence    0.3030  écart   0.2 %  z  0.2
OK  sain FC 72         Énergie 200–400 Hz           Rust    0.0118  référence    0.0117  écart   0.8 %  z  0.4
OK  sain FC 72         B2 > B1 (part des paires)    Rust    1.0000  référence    1.0000  écart   0.0 %  z  0.0
OK  sain FC 72         Rapport B2/B1 médian         Rust    2.0897  référence    2.0549  écart   1.7 %  z  0.5
OK  sain FC 72         Diastole / systole           Rust    1.7159  référence    1.7447  écart   1.7 %  z  0.8
OK  sain FC 90         Centroïde spectral (Hz)      Rust   91.2575  référence   91.3799  écart   0.1 %  z  0.8
OK  sain FC 90         Énergie 20–50 Hz             Rust    0.0432  référence    0.0428  écart   1.0 %  z  0.5
OK  sain FC 90         Énergie 50–100 Hz            Rust    0.6491  référence    0.6491  écart   0.0 %  z  0.0
OK  sain FC 90         Énergie 100–200 Hz           Rust    0.2966  référence    0.2965  écart   0.0 %  z  0.0
OK  sain FC 90         Énergie 200–400 Hz           Rust    0.0091  référence    0.0093  écart   1.6 %  z  0.9
OK  sain FC 90         B2 > B1 (part des paires)    Rust    0.8333  référence    0.8333  écart   0.0 %  z  0.0
OK  sain FC 90         Rapport B2/B1 médian         Rust    1.6987  référence    1.7112  écart   0.7 %  z  0.3
OK  sain FC 90         B1→B2 médian (s), FC 90      Rust    0.2668  référence    0.2671  écart   0.1 %  z  0.6
OK  sain FC 90         Diastole / systole           Rust    1.2490  référence    1.2335  écart   1.3 %  z  0.6
OK  MMVD 0,3 (léger)   Centroïde spectral (Hz)      Rust  112.3023  référence  113.0013  écart   0.6 %  z  2.0
OK  MMVD 0,3 (léger)   Énergie 20–50 Hz             Rust    0.0322  référence    0.0325  écart   0.8 %  z  0.4
OK  MMVD 0,3 (léger)   Énergie 50–100 Hz            Rust    0.5176  référence    0.5146  écart   0.6 %  z  0.9
OK  MMVD 0,3 (léger)   Énergie 100–200 Hz           Rust    0.3542  référence    0.3565  écart   0.6 %  z  0.8
OK  MMVD 0,3 (léger)   Énergie 200–400 Hz           Rust    0.0913  référence    0.0933  écart   2.2 %  z  1.5
OK  MMVD 0,3 (léger)   B2 > B1 (part des paires)    Rust    0.6250  référence    0.6000  écart   4.2 %  z  1.2
OK  MMVD 0,3 (léger)   Rapport B2/B1 médian         Rust    1.4111  référence    1.3417  écart   5.2 %  z  1.4
OK  MMVD 0,3 (léger)   Diastole / systole           Rust    1.4299  référence    1.4017  écart   2.0 %  z  0.3
OK  MMVD 0,5 (modéré)  Centroïde spectral (Hz)      Rust  133.1331  référence  133.7899  écart   0.5 %  z  1.7
OK  MMVD 0,5 (modéré)  Énergie 20–50 Hz             Rust    0.0218  référence    0.0218  écart   0.4 %  z  0.2
OK  MMVD 0,5 (modéré)  Énergie 50–100 Hz            Rust    0.3936  référence    0.3851  écart   2.2 %  z  2.8
OK  MMVD 0,5 (modéré)  Énergie 100–200 Hz           Rust    0.4080  référence    0.4110  écart   0.7 %  z  1.3
OK  MMVD 0,5 (modéré)  Énergie 200–400 Hz           Rust    0.1730  référence    0.1758  écart   1.6 %  z  1.7
OK  MMVD 0,5 (modéré)  B2 > B1 (part des paires)    Rust    0.8182  référence    0.8182  écart   0.0 %  z  0.0
OK  MMVD 0,5 (modéré)  Rapport B2/B1 médian         Rust    1.6497  référence    1.6353  écart   0.9 %  z  0.5
OK  MMVD 0,5 (modéré)  Diastole / systole           Rust    2.9288  référence    3.0794  écart   4.9 %  z  1.9
```
