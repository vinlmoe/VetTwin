# Prompt : module « auscultation » du patient virtuel vétérinaire (vetsim)

## Rôle
Tu es un ingénieur Rust/Kotlin Multiplatform et traitement du signal. Tu ajoutes au projet vetsim (déjà décrit dans `PROMPT.md`, `CLAUDE.md`, `docs/`) un module qui **synthétise les bruits du cœur du chien** à partir de l'état hémodynamique simulé. Lis d'abord `CLAUDE.md`, puis `reference/python/signals_pcg_v5.py` (implémentation normative en Python), puis `reference/python/pcg_metrics.py` (métriques d'acceptation).

## Objectif
Fonction pure `synthesize_pcg(hemo, autonomic, options) -> PcgResult` dans `vetsim-core/src/pcg.rs`, exposée via `vetsim-ffi` (UniFFI) et la messagerie JSON, avec lecture audio dans `kmp-demo`. Le son doit **s'entendre comme un vrai chien** : deux bruits distincts (« boum-ta »), arythmie sinusale respiratoire audible, souffle gradé, bruit de fond.

## Entrées
- `hemo` (un cycle simulé, pas `dt = 2 ms`) : `T`, `ts` (durée de contraction), séries `t`, `P_lv`, `P_ao`, `P_pv`, `Q_mi`, `Q_ao`, `la_mean`.
- `autonomic.beats` : instants des battements (module vagal/sympathique, arythmie sinusale respiratoire).
- `options` : `fs` (4000 par défaut), `dur` (8 s), `seed`, `resp_hz` (fréquence respiratoire), `site` (voir plus bas).

## Événements dérivés (comme dans la référence)
- `t_mc` : fermeture mitrale (P_lv dépasse P_pv).
- `t_ac` : fermeture aortique (fin du débit aortique). **Garde-fou** : `t_ac = max(t_ac, t_mc + 0.7*ts)` car le modèle raccourcit trop l'éjection en régurgitation mitrale sévère.
- `dpdt_max`, `dp_close = 2*P_ao(t_ac) - P_lv(t_ac)`, `q_early` (débit mitral protodiastolique).

## Synthèse (par battement, à l'instant `b` de `autonomic.beats`)
1. **B1** à `b + t_mc` : somme de 3 sinusoïdes amorties (fréquences ×0,80 / ×1,0 / ×1,45 de f0 = 70 Hz, poids 0,6 / 1 / 0,6 à 0,8, tau 20 ms), attaque 4 ms, phases tirées au hasard.
2. **B2** à `b + t_mc + ts_b`, avec `ts_b = (t_ac - t_mc) * (RR/T)^0.35` (la systole raccourcit peu quand le RR diminue) : deux composantes, aortique (f0 96 Hz, tau 15 ms) et pulmonaire (f0 90 Hz) retardée de `0.008 + 0.030*insp` s, où `insp = max(0, sin(2π·resp_hz·b))`. Dédoublement physiologique à l'inspiration ; amplitude pulmonaire 0,35 + 0,3·insp.
3. **B3** si `la_mean > 15` mmHg : burst 45 Hz à `t_ac + 0.12 s`, amplitude `0.35*max(0,(q_early-160)/160)`.
4. **Souffle** : bruit gaussien filtré 60–250 Hz, enveloppe = `sqrt(q_reg(t)/q_reg_max)`, où `q_reg = max(-Q_mi,0)+max(-Q_ao,0)` ; amplitude `m_amp = 0.7*0.016*sqrt(q_reg_max)` si `q_reg_max > 3 mL/s`, facteur aléatoire par battement uniforme(0,7 ; 1,2).
5. **Variabilité** : gain par battement lognormal(0 ; 0,18) pour B1 et B2 (indépendants), modulation respiratoire `1 + 0.15*sin(2π·resp_hz·b)`, B2 multiplié par `(RR/T)^0.5`.
6. **Bruit de fond** : bruit gaussien filtré 35–220 Hz, amplitude 0,010, modulé par la respiration.
7. **Amplitudes de base (gains de site)** : `s1 = 0.7*0.5*min(2, dpdt_max/1800)`, `s2 = 3.1*0.4*min(2, dp_close/190)`. Ces gains sont calés sur **un** enregistrement normal de la bibliothèque TiHo (B2 médian = 1,9 × B1). Expose-les dans `options.site` (`base` = B2 > B1, `apex` = B1 > B2) ; le préréglage `apex` est à calibrer.
8. Normalisation à 1 en crête ; sortie WAV 16 bits mono ; grade de souffle heuristique d'après `q_reg_max` (seuils [10, 25, 50, 80, 120] mL/s, **non calibrés**).

Tous les paramètres numériques vont dans un fichier de configuration JSON versionné (`modules/auscultation.json`) avec `source`, `evidence` (A–D) et `calibre_sur` ; pas de constantes magiques dans le code.

## Critères d'acceptation (tests automatiques, sur le chien sain, FC 72 et FC 90, 20 graines)
Utilise `pcg_metrics.py` comme référence des mesures ; le RNG Rust ne reproduit pas NumPy, donc **tests statistiques**, pas bit à bit.
- Centroïde spectral 90 ± 15 Hz ; part de l'énergie 20–1000 Hz : 20–50 Hz ≤ 8 %, 50–100 Hz 60–75 %, 100–200 Hz 20–35 %, > 200 Hz ≤ 5 %.
- Intervalle B1→B2 (médiane) 0,15–0,25 s à FC 90 ; diastole médiane > 1,8 × systole.
- B2 > B1 dans ≥ 80 % des paires ; rapport médian B2/B1 entre 1,5 et 2,3.
- Écart-type des RR : celui du module autonome (ne pas le modifier ici).
- Aucune valeur NaN, pas de saturation, durée exacte, déterminisme pour une graine donnée.
- Test de non-régression : MMVD léger (sévérité 0,3) a un souffle holosystolique visible et B1 ≥ le sain.

## Contraintes et limites à respecter
- **Licence** : la bibliothèque TiHo (Hanovre) est sous CC BY-NC-ND 4.0. **Ne copie, ne redistribue et n'embarque aucun enregistrement** ; seuls les chiffres mesurés ci-dessus sont utilisés. Aucune version modifiée d'un enregistrement réel dans le dépôt.
- Calage fondé sur **deux** enregistrements (un normal, un souffle mitral II/VI) : écris-le dans le README du module comme limite de validité.
- Écart connu : le souffle réel est plus grave et moins « plateau » que le modèle ; B1→B2 vaut 0,21 s dans le modèle contre 0,18 s mesuré.
- L'hémodynamique en RM sévère raccourcit trop l'éjection aortique : ne pas corriger dans le module audio (le garde-fou est une rustine documentée).
- Le préréglage de fréquence cardiaque « repos » (72/min) est un réglage d'usage, pas la valeur calibrée du chien sain (87/min).

## Livrables
1. `pcg.rs` + tests unitaires et statistiques ; `modules/auscultation.json` + schéma ; mise à jour de `docs/`.
2. Exposition FFI et message JSON `{"op":"pcg","patient":…,"options":…}` renvoyant `{wav_base64, fs, grade, murmur, s3}`.
3. Démo KMP : bouton « écouter », forme d'onde et spectrogramme, choix FC et site d'auscultation.
4. Rapport de conformité : tableau des métriques (cibles, obtenues, statut) pour chien sain et MMVD léger/modéré.

Travaille par étapes : (a) port fidèle de la référence ; (b) tests d'acceptation ; (c) FFI ; (d) démo. Ne passe à l'étape suivante que si les tests de l'étape courante sont verts, et signale tout écart avec la référence Python.
