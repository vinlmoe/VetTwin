# Vérification de l'ECG du modèle contre des ECG canins de référence (17 novembre 2026)

Ce rapport mesure l'ECG produit par le modèle (dérivation II, `signals.make_ecg_conducted`, chien de 20 kg sans variabilité interindividuelle, graine 0). Il compare ces mesures à des valeurs publiées. La première version de ce rapport ne modifiait rien. Les corrections de forme ont ensuite été appliquées (section « Corrections appliquées »). Aucun module ni aucun paramètre physiologique n'a été modifié. La recherche bibliographique a été faite par une session IA séparée de ce projet, ce n'est pas une relecture externe. Les 10 DOI cités ont été vérifiés dans OpenAlex (titre, auteurs, année).

Accès aux sources : **lu** = texte intégral (Europe PMC) ; **résumé** = résumé seul.

## Méthode

* **Mesure sur le tracé.** Le bruit du générateur est retiré par soustraction : même graine, bruit blanc 0,01 mV et ligne de base 0,02 mV à 0,25 Hz. Les ondes sont ensuite délimitées au seuil |v| > 0,02 mV. Ce seuil correspond à 0,2 mm à 10 mm/mV, soit à peu près la limite de lecture à l'œil.
* **Durée visible de chaque onde.** Elle est calculée à partir de chaque gaussienne du générateur, avec les mêmes centres, largeurs et amplitudes et le même seuil. Ce calcul est nécessaire parce que, sur le tracé composite, l'onde T fusionne avec le QRS, ce qui empêche de délimiter le QRS directement.
* **Sensibilité au seuil.** Les mesures sont aussi faites à 0,05 mV.
* **Scripts** (hors dépôt, dans le répertoire de travail de la session) : `ecg_measure.py` (tracé) et `ecg_analytic.py` (ondes). Les mesures sur le tracé et le calcul par onde concordent pour P : 92 ms et 94 ms.

## A. Chien sain : intervalles et amplitudes

FC du modèle : 87,6/min ; K 4,1 mmol/L. La FC de référence est de 60 à 170/min [1], donc conforme.

| Grandeur | Modèle, seuil 0,02 mV (0,05) | Référence | Verdict |
|---|---|---|---|
| Durée de P | **94 ms** (73) | < 40 ms [1, secondaire] ; 40 et 50 ms (Labrador, Berger allemand) [2] ; 44 (42–47) ms [3] | **Trop longue, environ ×2** |
| Amplitude de P | 0,20 mV | < 0,4 mV [1] ; 0,21 et 0,19 mV [2] ; 0,21 mV [3] | Conforme |
| PR | 112–136 ms (médiane 120) | 60–130 ms [1] ; 120 ms [2] ; 95 (83–103) ms [3] | Conforme, en haut de plage |
| Durée du QRS | **84 ms** (77) visible ; paramètre `qrs` 45 ms | < 70 ms [1] ; 40 et 50 ms [2] ; 62 (59–69) ms [3] | **Trop long** : les ondes Q et S du générateur débordent de 20 ms de chaque côté de la valeur nominale |
| Amplitude de R | 1,76 mV | < 3 mV [1] ; 1,78 et 1,64 mV [2] ; 1,1 (0,84–1,40) mV [3] | Conforme |
| QT | 229 ms (206) | 150–240 ms [1] ; 210 et 190 ms [2] ; 207 (193–227) ms [3] | Conforme |
| Amplitude de T | 0,30 mV (T/R 0,18) | 0,05 à 1 mV, positive ou négative [1] ; 0,38 et 0,23 mV [2] ; 0,29 (0,15–0,39) mV [3] | Conforme |
| Segment ST | **absent** : T commence 65 ms avant la fin du QRS ; durée visible de T 209 ms | Le segment ST est mesuré chez 180 chiens sains (sus- ou sous-décalé chez 43) [4], il existe donc | **Non conforme (qualitatif)** : T est trop large (σ 45 ms) |
| Axe électrique | non calculable : dérivation II seule | +60 à +120° ; 78 et 87° [2] | Non modélisé |

**Cause.** Les écarts viennent des constantes de forme du générateur et non de la physiologie :

* la largeur de P est fixée à σ = 22 ms ;
* les ondes Q et S sont centrées à ±0,55·`qrs`, avec σ de 0,8 et 0,9 fois `qrs`/5 ;
* l'onde T a une largeur `t_w` de 45 ms.

Les intervalles qui pilotent le modèle (`pr` 0,12 s, `qrs` 0,045 s, `qt` 0,20 s) sont, eux, conformes [2].

## B. Hyperkaliémie (Addison)

| Cas | K (mmol/L) | FC | P (mV, ms) | PR (ms) | QRS visible (ms) | T (mV), T/R | QT visible (ms) |
|---|---|---|---|---|---|---|---|
| Sain | 4,10 | 88 | 0,20 ; 94 | 120 | 84 | 0,30 ; 0,18 | 229 |
| Addison 0,5 | 5,72 | 109 | 0,20 ; 94 | 120 | 84 | 0,33 ; 0,19 | 214 |
| Addison 0,8 | 6,70 | 141 | 0,165 ; 90 | 136 | 86 | 0,46 ; 0,27 | 197 |
| Addison 1,0 | 7,35 | 163 | 0,133 ; 86 | 151 | 93 | 0,55 ; 0,32 | 187 |

La séquence décrite au-dessus de 6–6,5 mmol/L [5, citation secondaire d'un traité] est : T ample et pointue, QT court, P plus longue et plus petite, puis bradycardie, rythme sino-ventriculaire et QRS large. Le modèle reproduit le sens de chaque changement de forme :

* T plus ample et plus étroite ;
* QT raccourci ;
* P plus petite ;
* PR allongé ;
* QRS élargi au-delà de 6,5.

Deux écarts restent :

1. **P plus courte au lieu de plus longue.** L'amplitude baisse alors que la largeur reste fixe, donc la durée visible diminue.
2. **Pas de bradycardie ni de perte de P à K 7,35.** Le cas publié d'une crise addisonienne (1 chien, K 7,9 ; lu) montre un bloc AV du 3e degré : fréquence atriale 140/min, ventriculaire 60/min, P à 0,04 mV [5]. Dans un essai de perfusion de KCl (résumé), les ondes P disparaissent, et 37 des 50 mesures ont K ≥ 7,5 mmol/L [6]. Le modèle ne supprime P qu'à K 8,5 (arrêt atrial, Hariman 1983, résumé) et ne bloque la conduction His-Purkinje qu'au-delà de 10. C'est un écart déjà connu (« Addison sans frein », NOUVELLE_REFERENCE.md). Ces deux sources indiquent un seuil plus bas, mais l'une est un cas unique et l'autre n'a été lue qu'en résumé. Elles ne suffisent pas à caler un seuil.

## C. Troubles du rythme : critères diagnostiques

Fenêtre de 8 s, sévérités indiquées.

| Trouble (modèle) | Ce que produit le modèle | Critère publié | Verdict |
|---|---|---|---|
| FA 0,5 | Aucune onde P ; ondes f (63 activations atriales en 8 s, 0,06 mV) ; RR irréguliers de 0,33 à 0,84 s ; FC 132 | « absence d'ondes P, réponse ventriculaire irrégulière, ondes f d'amplitude variable » [7, lu] | Conforme ; l'amplitude des ondes f est constante (0,06 mV) alors que la littérature la dit variable |
| BAV 0,3 | PR 222–229 ms, conduction 1:1 | PR de référence ≤ 130 ms [1] ; aucun seuil publié du 1er degré trouvé en accès libre | Cohérent (BAV du 1er degré) |
| BAV 0,6 | 14 P pour 7 QRS, PR constant 240 ms, FC 52 | Définition du bloc 2:1 [8, lu] | Conforme |
| BAV 1,0 | Aucune P conduite ; échappement à QRS large, 42/min, RR fixe 1,43 s | Rythme idioventriculaire : ≥ 4 battements à ≤ 50/min [9, lu] | Conforme |
| ESV 0,6 | 1 QRS large (492 ms visible) avec T discordante, RR de couplage 0,22 s pour un RR dominant d'environ 0,64 s | ESV : RR < 100 % du RR dominant [9] | Conforme ; aucune valeur publiée de largeur du QRS ni de polarité de T trouvée en accès libre |
| SSS 0,8 | Pause maximale 4,33 s en 8 s, FC 53 | Pause ≥ 2,0 s [9] ; chiens sains de grande race : pause la plus longue sur 24 h 3,8 ± 0,8 s [9] ; un cas de SSS : pause maximale 9,2 s sur 24 h [10, lu] | Conforme au critère. Une pause de 4,3 s reste dans l'étendue des chiens sains de grande race sur 24 h, mais elle survient ici dans une fenêtre de 8 s |

Le QRS d'un battement ventriculaire mesure environ 490 ms à 0,02 mV, alors que le paramètre vaut 2,2·`qrs`, soit 99 ms. L'onde T discordante, large, fusionne avec le QRS : même artefact de forme qu'en A.

## Corrections appliquées (17 novembre 2026, validées)

Les corrections ne touchent que la forme des ondes dans `signals.py` (référence) et `signals.rs` (port). Elles ne changent ni la physiologie, ni la conduction, ni les intervalles du modèle.

| Constante | Avant | Après | Fondement |
|---|---|---|---|
| `P_SIGMA` | 22 ms | 10,5 ms | P visible 45 ms à 0,2 mV [2, 3] |
| R, Q, S | R σ `qrs`/5 ; Q et S à ±0,55·`qrs` | R σ `qrs`/10 ; Q à −0,34·`qrs` (σ `qrs`/14) ; S à +0,34·`qrs` (σ `qrs`/13) | QRS visible = paramètre `qrs`, avec Q et S encore visibles en négatif |
| QRS ventriculaire | R σ (2,2·`qrs`)/5 ; S à +0,55 | R σ /8 ; S à +0,40 (σ /11) | QRS visible = `WIDE_QRS`·`qrs` (101 ms pour 99 ms) |
| `T_W0` | 45 ms | 25 ms, avec la même loi relative d'étroitesse en hyperkaliémie | Segment ST isoélectrique [4]. Placeholder D : aucune durée de T lue |
| `P_WIDEN_K` | — | +25 % de largeur de P par mmol/L au-delà de 6 | Sens « P plus longue » [5, secondaire]. Placeholder D |
| Ondes f | amplitude constante | amplitude × min(`F_WAVE_MAX` = 2, intervalle / `af_cycle`) | « amplitude variable » [7]. Variabilité issue des cycles de la conduction, sans tirage propre (placeholder D) |

**Mesures sur le tracé après correction** (même méthode, seuil 0,02 mV ; médianes) :

| Cas | P (ms) | PR (ms) | QRS (ms) | QT (ms) | T (mV) |
|---|---|---|---|---|---|
| Sain, FC 88 | 42 | 112 | 44 | 202 | 0,30 |
| Addison 0,8 (K 6,7) | 48 | 132 | 44 | 174 | 0,46 |
| Addison 1,0 (K 7,35) | 52 | 153 | 48 | 160 | 0,55 |
| BAV 0,6 (2:1) | 42 | 232 | 44 | 250 | 0,30 |

Chez le chien sain, toutes les valeurs sont dans les plages de [1–3]. En hyperkaliémie, P s'allonge désormais.

Ondes f en FA 0,9 : de 0,046 à 0,086 mV, contre 0,06 mV constants auparavant.

Battement ventriculaire : son T discordant suit le QRS presque sans segment ST, ce qui est attendu pour un battement large. Le tracé ne permet donc pas de délimiter le QRS seul, et la durée visible de 101 ms est calculée par onde.

**Ce qui reste inchangé :**

* pas de perte de P ni de bloc à K 7,35, alors que le cas publié montre un bloc complet à K 7,9 (seuils de la conduction) ;
* l'axe électrique n'est pas modélisé.

**Vérification :**

* nouvelle fixture `python_reference_ecg_conducted.json`, produite par `tests/make_ecg_conducted_fixture.py` : 8 séquences (sain, FA, BAV 3, ESV, K 6,7, K 7,35, K 9, K 2,5) ;
* test Rust `tests/ecg_conducted.rs` : tracé identique à 1e-9 ;
* contrôle d'échec dans le test : QRS +5 %, cycle de FA +10 % ;
* 3 mutations du source Rust détectées sur 3 (`R_SIGMA`, `F_WAVE_MAX`, signe de `P_WIDEN_K`) ;
* fixture M6 régénérée, `tests/signals.rs` conforme.

## Corrections proposées dans la première version de ce rapport

Toutes portent sur les constantes de forme de `signals.py`, qui est une référence normative. Elles changeraient les fixtures ECG et demandent une décision du projet. Aucune ne touche la physiologie ni la conduction.

1. **Largeur de P.** σ = 22 ms → environ 10 ms, ce qui donne une durée visible de 45 ms à 0,2 mV, au centre de [2, 3].
2. **QRS.** Placer Q et S pour que la durée visible égale le paramètre `qrs`. Exemple : centres à ±0,3·`qrs` et σ de `qrs`/8. Calage à faire.
3. **Onde T.** Réduire `t_w` (environ 25–30 ms) ou rendre T asymétrique, pour qu'un segment ST isoélectrique sépare le QRS de T. La valeur demande une source : la durée de T de Romito 2022 (J Vet Cardiol 42:52–64) n'est pas en accès libre et n'a pas été lue.
4. **Hyperkaliémie.** P plus large (et non seulement plus petite) quand K monte. Seuil de perte de P et du bloc à revoir si une source plus solide que [5, 6] est trouvée.
5. **FA.** Amplitude des ondes f variable : tirage par onde, sur le flux aléatoire de la conduction.

## Non trouvé ou non lu

* Aucun seuil publié pour la profondeur de Q et de S, ni pour le rapport T/R (comme « T < 25 % de R »).
* Aucune mesure de l'amplitude ou de la fréquence des ondes f, ni des blocs Mobitz I et II.
* Aucune donnée ECG sur l'hypokaliémie.
* Non lus : Tilley 1992, Santilli 2018, Romito 2022 (onde T), Ward 2016 (SSS), PMC13142072 (refusé : « Too Many Requests »).
* Ces absences ne prouvent pas que ces données n'existent pas.
* Jeux de données ECG canins en accès libre repérés mais non ouverts : Zenodo 10.5281/zenodo.8020390 (WAV, CC-BY-4.0), figshare 10.6084/m9.figshare.23820915.v1.

## Références

1. Santos BA et al. 2024, *Vet Sci* 11:467, doi:10.3390/vetsci11100467 (lu ; Table 2, intervalles cités de Romão 2022, donc secondaire).
2. Lerdweeraphon W et al. 2020, *Vet World* 13:1001–1004, doi:10.14202/vetworld.2020.1001-1004 (lu ; Table 1, 16 Labradors et 14 Bergers allemands, vigiles, décubitus latéral droit).
3. Bispo GA et al. 2025, *Vet Med Int*, doi:10.1155/vmi/9947159 (lu ; Table 1, colonne de base sous butorphanol, n = 22).
4. Romito G et al. 2022, *J Small Anim Pract*, doi:10.1111/jsap.13532 (lu ; 180 chiens sains).
5. Pașca PM et al. 2025, *Vet Sci* 12:63, doi:10.3390/vetsci12010063 (lu ; cas clinique, et introduction citant Santilli 2018).
6. Porter RS et al. 2001, *Acad Emerg Med*, doi:10.1111/j.1553-2712.2001.tb00170.x (résumé).
7. Kurogochi K et al. 2026, *J Vet Intern Med*, doi:10.1093/jvimsj/aalaf028 (lu ; Méthodes).
8. Santilli RA et al. 2015, *J Vet Intern Med*, doi:10.1111/jvim.13661 (lu ; Méthodes).
9. Gunasekaran T et al. 2025, *PLoS ONE*, doi:10.1371/journal.pone.0319886 (lu ; Tables 1 et 2).
10. Kanno N, Suzuki T 2017, *J Vet Med Sci*, doi:10.1292/jvms.17-0018 (lu ; Table 1).
