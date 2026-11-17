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
