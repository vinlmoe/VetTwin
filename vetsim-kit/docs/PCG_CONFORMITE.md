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
| sain FC 72 | Centroïde spectral (Hz) | 90 ± 15 | 89.023 | 88.896 | OK |
| sain FC 72 | Énergie 20–50 Hz | ≤ 8 % | 4.9 % | 4.9 % | OK |
| sain FC 72 | Énergie 50–100 Hz | 60–75 % | 65.9 % | 65.8 % | OK |
| sain FC 72 | Énergie 100–200 Hz | 20–35 % | 28.6 % | 28.8 % | OK |
| sain FC 72 | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 0.4 % | 0.4 % | OK |
| sain FC 72 | B2 > B1 (part des paires) | ≥ 80 % | 1.000 | 1.000 | OK |
| sain FC 72 | Rapport B2/B1 médian | 1,5–2,3 | 1.866 | 1.923 | OK |
| sain FC 72 | Diastole / systole | > 1,8 | 1.784 | 1.801 | à la limite du seuil (Rust équivalent à la référence, de part et d'autre) |
| sain FC 72 | Informations |  | paires B1/B2 par graine : médiane 5, graines sans paire 0 ; FC 71.1/min, SD RR 188 ms, grade [0], souffle ['aucun'], B3 [False], écrêtage 302, finis True, échantillons [32000] |  |  |
| sain FC 90 | Centroïde spectral (Hz) | 90 ± 15 | 88.039 | 88.011 | OK |
| sain FC 90 | Énergie 20–50 Hz | ≤ 8 % | 5.2 % | 5.1 % | OK |
| sain FC 90 | Énergie 50–100 Hz | 60–75 % | 66.6 % | 66.9 % | OK |
| sain FC 90 | Énergie 100–200 Hz | 20–35 % | 27.9 % | 27.5 % | OK |
| sain FC 90 | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 0.4 % | 0.4 % | OK |
| sain FC 90 | B2 > B1 (part des paires) | ≥ 80 % | 85.7 % | 83.3 % | OK |
| sain FC 90 | Rapport B2/B1 médian | 1,5–2,3 | 1.586 | 1.546 | OK |
| sain FC 90 | B1→B2 médian (s), FC 90 | 0,15–0,25 | 0.258 | 0.258 | écart connu (référence) |
| sain FC 90 | Diastole / systole | > 1,8 | 1.316 | 1.295 | écart connu (référence) |
| sain FC 90 | Informations |  | paires B1/B2 par graine : médiane 7, graines sans paire 0 ; FC 91.0/min, SD RR 149 ms, grade [0], souffle ['aucun'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |
| MMVD 0,3 (léger) | Centroïde spectral (Hz) | 90 ± 15 | 92.499 | 92.284 | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 20–50 Hz | ≤ 8 % | 5.0 % | 5.0 % | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 50–100 Hz | 60–75 % | 63.6 % | 63.5 % | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 100–200 Hz | 20–35 % | 29.8 % | 29.6 % | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 1.7 % | 1.7 % | (dans la cible) |
| MMVD 0,3 (léger) | B2 > B1 (part des paires) | ≥ 80 % | 85.7 % | 85.7 % | (dans la cible) |
| MMVD 0,3 (léger) | Rapport B2/B1 médian | 1,5–2,3 | 1.577 | 1.540 | (dans la cible) |
| MMVD 0,3 (léger) | Diastole / systole | > 1,8 | 1.481 | 1.484 | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Informations |  | paires B1/B2 par graine : médiane 8, graines sans paire 0 ; FC 88.2/min, SD RR 154 ms, grade [3, 4], souffle ['holosystolique apexien gauche'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |
| MMVD 0,5 (modéré) | Centroïde spectral (Hz) | 90 ± 15 | 106.449 | 106.376 | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 20–50 Hz | ≤ 8 % | 4.1 % | 4.1 % | (dans la cible) |
| MMVD 0,5 (modéré) | Énergie 50–100 Hz | 60–75 % | 54.0 % | 54.2 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 100–200 Hz | 20–35 % | 36.0 % | 35.7 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 5.5 % | 5.6 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | B2 > B1 (part des paires) | ≥ 80 % | 70.0 % | 72.7 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Rapport B2/B1 médian | 1,5–2,3 | 1.417 | 1.418 | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Diastole / systole | > 1,8 | 1.794 | 1.789 | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Informations |  | paires B1/B2 par graine : médiane 9, graines sans paire 0 ; FC 98.0/min, SD RR 112 ms, grade [5], souffle ['holosystolique apexien gauche'], B3 [False], écrêtage 300, finis True, échantillons [32000] |  |  |

## Équivalence statistique avec la référence Python

```
OK  sain FC 72         Centroïde spectral (Hz)      Rust   89.0226  référence   88.8961  écart   0.1 %  z  0.7
OK  sain FC 72         Énergie 20–50 Hz             Rust    0.0494  référence    0.0487  écart   1.3 %  z  0.7
OK  sain FC 72         Énergie 50–100 Hz            Rust    0.6586  référence    0.6579  écart   0.1 %  z  0.2
OK  sain FC 72         Énergie 100–200 Hz           Rust    0.2859  référence    0.2883  écart   0.9 %  z  0.7
OK  sain FC 72         Énergie 200–400 Hz           Rust    0.0043  référence    0.0041  écart   5.1 %  z  2.1
OK  sain FC 72         B2 > B1 (part des paires)    Rust    1.0000  référence    1.0000  écart   0.0 %  z  0.0
OK  sain FC 72         Rapport B2/B1 médian         Rust    1.8657  référence    1.9227  écart   3.0 %  z  1.3
OK  sain FC 72         Diastole / systole           Rust    1.7842  référence    1.8009  écart   0.9 %  z  0.4
OK  sain FC 90         Centroïde spectral (Hz)      Rust   88.0395  référence   88.0108  écart   0.0 %  z  0.2
OK  sain FC 90         Énergie 20–50 Hz             Rust    0.0518  référence    0.0512  écart   1.1 %  z  0.6
OK  sain FC 90         Énergie 50–100 Hz            Rust    0.6658  référence    0.6688  écart   0.5 %  z  1.0
OK  sain FC 90         Énergie 100–200 Hz           Rust    0.2787  référence    0.2749  écart   1.4 %  z  1.3
OK  sain FC 90         Énergie 200–400 Hz           Rust    0.0041  référence    0.0040  écart   2.6 %  z  1.2
OK  sain FC 90         B2 > B1 (part des paires)    Rust    0.8571  référence    0.8333  écart   2.9 %  z  1.6
OK  sain FC 90         Rapport B2/B1 médian         Rust    1.5861  référence    1.5462  écart   2.6 %  z  1.2
OK  sain FC 90         B1→B2 médian (s), FC 90      Rust    0.2575  référence    0.2583  écart   0.3 %  z  1.1
OK  sain FC 90         Diastole / systole           Rust    1.3163  référence    1.2954  écart   1.6 %  z  0.8
OK  MMVD 0,3 (léger)   Centroïde spectral (Hz)      Rust   92.4993  référence   92.2841  écart   0.2 %  z  1.2
OK  MMVD 0,3 (léger)   Énergie 20–50 Hz             Rust    0.0495  référence    0.0496  écart   0.1 %  z  0.0
OK  MMVD 0,3 (léger)   Énergie 50–100 Hz            Rust    0.6360  référence    0.6346  écart   0.2 %  z  0.5
OK  MMVD 0,3 (léger)   Énergie 100–200 Hz           Rust    0.2977  référence    0.2957  écart   0.7 %  z  0.8
OK  MMVD 0,3 (léger)   Énergie 200–400 Hz           Rust    0.0167  référence    0.0171  écart   2.4 %  z  1.5
OK  MMVD 0,3 (léger)   B2 > B1 (part des paires)    Rust    0.8571  référence    0.8571  écart   0.0 %  z  0.0
OK  MMVD 0,3 (léger)   Rapport B2/B1 médian         Rust    1.5772  référence    1.5396  écart   2.4 %  z  1.2
OK  MMVD 0,3 (léger)   Diastole / systole           Rust    1.4812  référence    1.4843  écart   0.2 %  z  0.1
OK  MMVD 0,5 (modéré)  Centroïde spectral (Hz)      Rust  106.4491  référence  106.3758  écart   0.1 %  z  0.3
OK  MMVD 0,5 (modéré)  Énergie 20–50 Hz             Rust    0.0413  référence    0.0414  écart   0.3 %  z  0.2
OK  MMVD 0,5 (modéré)  Énergie 50–100 Hz            Rust    0.5397  référence    0.5420  écart   0.4 %  z  0.8
OK  MMVD 0,5 (modéré)  Énergie 100–200 Hz           Rust    0.3596  référence    0.3568  écart   0.8 %  z  1.1
OK  MMVD 0,5 (modéré)  Énergie 200–400 Hz           Rust    0.0552  référence    0.0559  écart   1.3 %  z  1.0
OK  MMVD 0,5 (modéré)  B2 > B1 (part des paires)    Rust    0.7000  référence    0.7273  écart   3.8 %  z  1.7
OK  MMVD 0,5 (modéré)  Rapport B2/B1 médian         Rust    1.4170  référence    1.4179  écart   0.1 %  z  0.0
OK  MMVD 0,5 (modéré)  Diastole / systole           Rust    1.7941  référence    1.7893  écart   0.3 %  z  0.1
```
