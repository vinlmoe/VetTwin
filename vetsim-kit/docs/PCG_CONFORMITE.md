# Conformité du module auscultation

Généré par `tests/pcg_acceptance.py` ; mesures par `reference/python/pcg_metrics.py`. Source : moteur Rust (target/release/vetsim-cli).

## Méthode
- Synthèse : `vetsim-cli pcg` (Rust, `crates/vetsim-core/src/pcg.rs`), port fidèle de `reference/python/signals_pcg_v5.py`. Avec les mêmes tirages, le Rust reproduit la référence à 4e-15 près (`cargo test --test auscultation`).
- Mesures : `reference/python/pcg_metrics.py` sur le WAV 16 bits produit. Chaque critère est jugé sur la médiane des graines. La diastole est le RR médian moins l'intervalle B1→B2 médian.
- Fréquence cardiaque : le paramètre `hr` est imposé avant l'évaluation (72 et 90/min). C'est un réglage d'usage, pas la valeur calibrée du chien sain (87/min).
- **100 graines au lieu des 20 du prompt.** `pcg_metrics` n'apparie B1 et B2 que si leur intervalle est inférieur à 0,3 s. À FC 72, la systole du modèle (0,33 s) dépasse ce seuil. On obtient alors environ 1 paire par graine, et 35 à 40 % des graines n'en ont aucune. À 20 graines, les métriques B2/B1 dépendent du hasard. Constaté : le Rust et la référence divergent à 20 graines (rapport B2/B1 à FC 72 : 1,01 contre 1,63) et convergent à 100 (1,59 contre 1,62).
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

| Cas | Métrique | Cible | Obtenu (médiane de 100 graines) | Référence Python | Statut |
|---|---|---|---|---|---|
| sain FC 72 | Centroïde spectral (Hz) | 90 ± 15 | 93.087 | 92.894 | OK |
| sain FC 72 | Énergie 20–50 Hz | ≤ 8 % | 4.0 % | 4.1 % | OK |
| sain FC 72 | Énergie 50–100 Hz | 60–75 % | 63.3 % | 64.3 % | OK |
| sain FC 72 | Énergie 100–200 Hz | 20–35 % | 31.3 % | 30.3 % | OK |
| sain FC 72 | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 1.2 % | 1.2 % | OK |
| sain FC 72 | B2 > B1 (part des paires) | ≥ 80 % | 1.000 | 1.000 | non mesurable (40 % de graines sans paire) |
| sain FC 72 | Rapport B2/B1 médian | 1,5–2,3 | 1.589 | 1.617 | non mesurable (40 % de graines sans paire) |
| sain FC 72 | Diastole / systole | > 1,8 | 1.602 | 1.581 | écart connu (référence) |
| sain FC 72 | Informations |  | paires B1/B2 par graine : médiane 1, graines sans paire 40 ; FC 74.1/min, SD RR 178 ms, grade [0], souffle ['aucun'], B3 [False], écrêtage 100, finis True, échantillons [32000] |  |  |
| sain FC 90 | Centroïde spectral (Hz) | 90 ± 15 | 92.119 | 92.224 | OK |
| sain FC 90 | Énergie 20–50 Hz | ≤ 8 % | 4.0 % | 4.1 % | OK |
| sain FC 90 | Énergie 50–100 Hz | 60–75 % | 64.5 % | 64.7 % | OK |
| sain FC 90 | Énergie 100–200 Hz | 20–35 % | 30.3 % | 29.9 % | OK |
| sain FC 90 | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 1.0 % | 1.1 % | OK |
| sain FC 90 | B2 > B1 (part des paires) | ≥ 80 % | 75.0 % | 75.0 % | écart connu (référence) |
| sain FC 90 | Rapport B2/B1 médian | 1,5–2,3 | 1.620 | 1.645 | OK |
| sain FC 90 | B1→B2 médian (s), FC 90 | 0,15–0,25 | 0.275 | 0.273 | écart connu (référence) |
| sain FC 90 | Diastole / systole | > 1,8 | 1.235 | 1.252 | écart connu (référence) |
| sain FC 90 | Informations |  | paires B1/B2 par graine : médiane 4, graines sans paire 1 ; FC 90.0/min, SD RR 173 ms, grade [0], souffle ['aucun'], B3 [False], écrêtage 100, finis True, échantillons [32000] |  |  |
| MMVD 0,3 (léger) | Centroïde spectral (Hz) | 90 ± 15 | 117.136 | 117.774 | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Énergie 20–50 Hz | ≤ 8 % | 2.8 % | 3.0 % | (dans la cible) |
| MMVD 0,3 (léger) | Énergie 50–100 Hz | 60–75 % | 49.6 % | 49.0 % | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Énergie 100–200 Hz | 20–35 % | 36.6 % | 36.8 % | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 10.6 % | 11.1 % | (hors cible, indicatif) |
| MMVD 0,3 (léger) | B2 > B1 (part des paires) | ≥ 80 % | 60.0 % | 57.1 % | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Rapport B2/B1 médian | 1,5–2,3 | 1.228 | 1.158 | (hors cible, indicatif) |
| MMVD 0,3 (léger) | Diastole / systole | > 1,8 | 2.125 | 2.185 | (dans la cible) |
| MMVD 0,3 (léger) | Informations |  | paires B1/B2 par graine : médiane 5, graines sans paire 0 ; FC 87.0/min, SD RR 178 ms, grade [3], souffle ['holosystolique apexien gauche'], B3 [False], écrêtage 100, finis True, échantillons [32000] |  |  |
| MMVD 0,5 (modéré) | Centroïde spectral (Hz) | 90 ± 15 | 136.378 | 138.028 | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 20–50 Hz | ≤ 8 % | 1.9 % | 1.9 % | (dans la cible) |
| MMVD 0,5 (modéré) | Énergie 50–100 Hz | 60–75 % | 37.1 % | 36.6 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 100–200 Hz | 20–35 % | 42.2 % | 42.0 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Énergie 200–400 Hz | ≤ 5 % (> 200 Hz) | 18.4 % | 19.1 % | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | B2 > B1 (part des paires) | ≥ 80 % | 80.0 % | 80.0 % | (dans la cible) |
| MMVD 0,5 (modéré) | Rapport B2/B1 médian | 1,5–2,3 | 1.496 | 1.501 | (hors cible, indicatif) |
| MMVD 0,5 (modéré) | Diastole / systole | > 1,8 | 3.363 | 3.257 | (dans la cible) |
| MMVD 0,5 (modéré) | Informations |  | paires B1/B2 par graine : médiane 10, graines sans paire 0 ; FC 97.8/min, SD RR 133 ms, grade [5], souffle ['holosystolique apexien gauche'], B3 [False], écrêtage 101, finis True, échantillons [32000] |  |  |

## Équivalence statistique avec la référence Python

```
OK  sain FC 72         Centroïde spectral (Hz)      Rust   93.0866  référence   92.8945  écart   0.2 %  z  0.6
OK  sain FC 72         Énergie 20–50 Hz             Rust    0.0398  référence    0.0406  écart   2.1 %  z  0.6
OK  sain FC 72         Énergie 50–100 Hz            Rust    0.6334  référence    0.6425  écart   1.4 %  z  1.5
OK  sain FC 72         Énergie 100–200 Hz           Rust    0.3134  référence    0.3033  écart   3.3 %  z  1.7
OK  sain FC 72         Énergie 200–400 Hz           Rust    0.0116  référence    0.0120  écart   3.3 %  z  1.1
OK  sain FC 72         B2 > B1 (part des paires)    Rust    1.0000  référence    1.0000  écart   0.0 %  z  0.0
OK  sain FC 72         Rapport B2/B1 médian         Rust    1.5893  référence    1.6172  écart   1.7 %  z  0.1
OK  sain FC 72         Diastole / systole           Rust    1.6018  référence    1.5805  écart   1.3 %  z  0.3
OK  sain FC 90         Centroïde spectral (Hz)      Rust   92.1193  référence   92.2236  écart   0.1 %  z  0.4
OK  sain FC 90         Énergie 20–50 Hz             Rust    0.0397  référence    0.0409  écart   3.0 %  z  1.0
OK  sain FC 90         Énergie 50–100 Hz            Rust    0.6447  référence    0.6472  écart   0.4 %  z  0.4
OK  sain FC 90         Énergie 100–200 Hz           Rust    0.3033  référence    0.2994  écart   1.3 %  z  0.7
OK  sain FC 90         Énergie 200–400 Hz           Rust    0.0097  référence    0.0106  écart   8.3 %  z  2.9
OK  sain FC 90         B2 > B1 (part des paires)    Rust    0.7500  référence    0.7500  écart   0.0 %  z  0.0
OK  sain FC 90         Rapport B2/B1 médian         Rust    1.6200  référence    1.6449  écart   1.5 %  z  0.2
OK  sain FC 90         B1→B2 médian (s), FC 90      Rust    0.2750  référence    0.2733  écart   0.6 %  z  0.7
OK  sain FC 90         Diastole / systole           Rust    1.2351  référence    1.2518  écart   1.3 %  z  0.4
OK  MMVD 0,3 (léger)   Centroïde spectral (Hz)      Rust  117.1357  référence  117.7736  écart   0.5 %  z  0.9
OK  MMVD 0,3 (léger)   Énergie 20–50 Hz             Rust    0.0280  référence    0.0295  écart   5.0 %  z  1.7
OK  MMVD 0,3 (léger)   Énergie 50–100 Hz            Rust    0.4959  référence    0.4896  écart   1.3 %  z  1.0
OK  MMVD 0,3 (léger)   Énergie 100–200 Hz           Rust    0.3664  référence    0.3676  écart   0.3 %  z  0.3
OK  MMVD 0,3 (léger)   Énergie 200–400 Hz           Rust    0.1058  référence    0.1106  écart   4.3 %  z  1.8
OK  MMVD 0,3 (léger)   B2 > B1 (part des paires)    Rust    0.6000  référence    0.5714  écart   5.0 %  z  0.7
OK  MMVD 0,3 (léger)   Rapport B2/B1 médian         Rust    1.2277  référence    1.1584  écart   6.0 %  z  0.8
OK  MMVD 0,3 (léger)   Diastole / systole           Rust    2.1246  référence    2.1846  écart   2.7 %  z  0.3
OK  MMVD 0,5 (modéré)  Centroïde spectral (Hz)      Rust  136.3783  référence  138.0284  écart   1.2 %  z  2.3
OK  MMVD 0,5 (modéré)  Énergie 20–50 Hz             Rust    0.0189  référence    0.0185  écart   1.9 %  z  0.6
OK  MMVD 0,5 (modéré)  Énergie 50–100 Hz            Rust    0.3711  référence    0.3659  écart   1.4 %  z  0.9
OK  MMVD 0,5 (modéré)  Énergie 100–200 Hz           Rust    0.4218  référence    0.4205  écart   0.3 %  z  0.3
OK  MMVD 0,5 (modéré)  Énergie 200–400 Hz           Rust    0.1845  référence    0.1909  écart   3.4 %  z  2.1
OK  MMVD 0,5 (modéré)  B2 > B1 (part des paires)    Rust    0.8000  référence    0.8000  écart   0.0 %  z  0.0
OK  MMVD 0,5 (modéré)  Rapport B2/B1 médian         Rust    1.4956  référence    1.5006  écart   0.3 %  z  0.1
OK  MMVD 0,5 (modéré)  Diastole / systole           Rust    3.3634  référence    3.2568  écart   3.3 %  z  1.0
```
