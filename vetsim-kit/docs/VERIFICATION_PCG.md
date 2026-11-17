# Vérification des bruits d'auscultation contre des enregistrements réels (17 novembre 2026)

Ce rapport compare les bruits synthétisés par le module auscultation à de vrais enregistrements de cœur de chien, puis décrit le recalage qui en découle (`modules/auscultation.json` version 1.1.0). **Usage pédagogique uniquement.**

## Source

Heartsound Library de la Tierärztliche Hochschule Hannover (K. Vörös, J. Ehlers, C. Kleinsorgen, I. Nolte ; pages *Grading*, *Examples* et *Cardiac arrhythmias*), consultée le 17 novembre 2026 :
<https://www.tiho-hannover.de/studium-lehre/zel/e-learning-beratung/service-materialien-und-lernprogramme/heartsound-library>

- 32 fichiers mp3 (30 enregistrements distincts : *Examples* 001 et 002 sont les mêmes que *Grading* 2/6 et 3/6).
- Matériel indiqué par la bibliothèque : stéthoscope électronique Welch Allyn Meditron (Analyser Meditron Master Elite).
- Licence CC BY-NC-ND 4.0. **Aucun enregistrement n'est copié, modifié ni embarqué dans le dépôt** ; les fichiers ont été mesurés dans un répertoire de travail hors dépôt, et seuls les chiffres figurent ici.
- La version 1.0.0 du module était calée sur deux de ces enregistrements, sans référence précise. Ce rapport les identifie : *Arrhythmias* 1.1 (rythme normal) et *Grading* 2/6.

## Méthode

Script : `tests/pcg_calibration_real.py` (réel et synthèse passent par la même chaîne).

- Signal ramené en mono, à 4000 Hz, crête à 1. Enveloppe de `pcg_metrics.py` : bande 25–300 Hz, transformée de Hilbert, passe-bas 25 Hz.
- **Repérage de B1 et B2.** Réel : pics d'enveloppe appariés comme dans `pcg_metrics.py`. Synthèse : instants connus du moteur (l'appariement de `pcg_metrics.py` échoue sur la synthèse, dont la systole de 0,26 s est trop proche du seuil de 0,3 s).
- **Durée d'un bruit.** Largeur à mi-hauteur de l'enveloppe, temps de montée (mi-hauteur → pic) et de descente (pic → mi-hauteur). Le passe-bas de l'enveloppe impose un plancher d'environ 12 ms à ces temps.
- **Niveaux par bande** (50–100, 100–200, 200–400, 400–800 Hz) : valeur efficace dans la fenêtre, rapportée à celle de B1 (bande 50–200 Hz, ±25 ms autour du pic). Systole : de B1 + 60 ms à B2 − 60 ms. Diastole : de B2 + 100 ms à B1 suivant − 60 ms.
- **Souffle seul** : puissance systolique moins puissance diastolique, par bande. Sans cette soustraction, le grondement basse fréquence de plusieurs enregistrements masque le souffle.
- Synthèse : `vetsim-cli pcg`, 20 s, 8 graines, médiane.

## Résultats (après recalage)

### 1. Bruits normaux et bruit de fond (site base, chien sain)

| Source | FC | B1→B2 (s) | B2/B1 | B1 : largeur / montée / descente (ms) | B2 : largeur / montée / descente (ms) | Fond 50–100 | 100–200 | 200–400 | 400–800 Hz (dB / B1) |
|---|---|---|---|---|---|---|---|---|---|
| Arrhythmias 1.1, rythme normal (beagle sain) | 91 | 0,184 | 1,93 | 38 / 16,2 / 22,5 | 25 / 11,8 / 14,2 | −16,3 | −20,4 | −38,2 | −74,4 |
| Arrhythmias 4.1, arythmie sinusale physiologique | 92 | 0,184 | 1,17 | 46 / 25,8 / 21,5 | 26 / 12,8 / 13,8 | −20,3 | −21,8 | −39,3 | −74,4 |
| Arrhythmias 3.1.2, bradycardie avec ESV isolées | 73 | 0,210 | 0,82 | 44 / 19,2 / 20,2 | 33 / 14,5 / 20,8 | −21,0 | −23,5 | −42,3 | −74,9 |
| **Réel, médiane** | 91 | 0,184 | 1,17 | 44 / 19,2 / 21,5 | 26 / 12,8 / 14,2 | −20,3 | −21,8 | −39,3 | −74,4 |
| **Synthèse recalée** (8 graines) | 90 | 0,261 | 1,73 | 43 / 16,5 / 21,6 | 29 / 12,6 / 15,6 | −22,1 | −21,1 | −39,9 | −65,7 |
| Synthèse avant recalage (4 graines) | 90 | 0,268 | 2,03 | 29 / 13,2 / 16,2 | 29 / 12,5 / 16,2 | −25,0 | −22,3 | −25,8 | −35,5 |

### 2. Durée de B1 et de B2 sur les 8 enregistrements de calage

| | B1 largeur (ms) | B1 montée | B1 descente | B2 largeur (ms) | B2 montée | B2 descente |
|---|---|---|---|---|---|---|
| Réel, médiane (3 sans souffle + 5 mitraux) | 45 | 18,6 | 22,0 | 31 | 14,2 | 17,8 |
| Synthèse recalée, chien sain (8 graines) | 43 | 16,5 | 21,6 | 29 | 12,6 | 15,6 |

### 3. Régurgitation mitrale à l'apex gauche : rapport B2/B1 et niveau du souffle

Niveau du souffle seul, en dB par rapport à B1 ; « — » : pas de souffle distinct du fond dans la bande. Les sévérités MMVD de la synthèse sont celles dont le débit régurgitant tombe dans le même grade du modèle.

| Source | Grade | B2/B1 | Souffle 100–200 Hz | 200–400 Hz | 400–800 Hz |
|---|---|---|---|---|---|
| Grading 1/6, insuffisance mitrale | 1 | 0,86 | −28,2 | −44,9 | −60,4 |
| Grading 2/6 (= Examples 001) | 2 | 0,53 | — | −37,9 | −45,7 |
| Synthèse, MMVD 0,2 à l'apex (débit 13 mL/s) | 2 | 0,83 | −32,6 | −38,8 | −50,2 |
| Grading 3/6 (= Examples 002) | 3 | 0,71 | −25,7 | −31,2 | −43,6 |
| Synthèse, MMVD 0,3 à l'apex (débit 35 mL/s) | 3 | 0,81 | −22,6 | −28,3 | −40,0 |
| Examples 003, insuffisance mitrale 4/6 | 4 | 0,82 | — | −20,5 | −26,0 |
| Synthèse, MMVD 0,4 à l'apex (débit 74 mL/s) | 4 | 0,79 | −15,3 | −20,9 | −32,4 |
| Grading 5/6, insuffisance mitrale | 5 | 0,92 | −8,6 | −18,5 | −32,5 |
| Synthèse, MMVD 0,45 à l'apex (débit 101 mL/s) | 5 | 0,77 | −12,8 | −18,2 | −29,8 |

Rapport B2/B1 réel à l'apex : médiane 0,82. Écart quadratique moyen synthèse − réel sur les niveaux du souffle : 3,4 dB (10 valeurs).

Avant recalage, MMVD 0,3 à l'apex : B2/B1 0,22 ; niveau systolique −17,3 dB (100–200 Hz) et −19,1 dB (200–400 Hz), soit un souffle de grade 3 plus fort de 8 à 12 dB que l'enregistrement réel de grade 3.

### 4. Ensemble de la bibliothèque

20 enregistrements avec au moins 8 paires B1/B2 repérées. B1→B2 : médiane 0,180 s (de 0,131 à 0,255). B2/B1 : médiane 0,82 (de 0,28 à 1,93). Synthèse, chien sain : B1→B2 0,261 s.

## Ce que les mesures montrent

1. **B1 était trop bref.** Largeur réelle 45 ms contre 29 ms ; B2 était correct (31 contre 29 ms). Allonger la décroissance ou l'attaque ne change presque rien, parce que la durée d'un bruit fait de trois sinusoïdes voisines est fixée par l'écart de leurs fréquences. Une seconde composante retardée (tricuspidienne) reproduit la durée mesurée.
2. **Le bruit de fond contenait trop d'aigus** : 12 à 14 dB de trop entre 200 et 400 Hz, et un souffle audible au-dessus de 400 Hz que les enregistrements sans souffle n'ont pas. Il manquait 5 dB entre 50 et 100 Hz.
3. **Le souffle mitral était trop fort pour son grade et sa progression trop plate.** Du grade 2 au grade 5, le niveau réel monte d'environ 20 dB ; avec une amplitude en racine du débit, la synthèse ne montait que de 9 dB.
4. **Le souffle réel n'est pas « plus grave » que le modèle.** L'ancienne documentation le disait, d'après le centroïde spectral de l'enregistrement 2/6 entier. Ce centroïde est dominé par un grondement sous 50 Hz présent aussi en diastole. Par bande, le souffle des grades 2 et 3 ressort du fond surtout entre 200 et 800 Hz (+8 à +15 dB), presque pas sous 200 Hz. Son spectre décroît d'environ 8 dB par octave de 100 à 800 Hz.
5. **La forme en plateau est confirmée** aux grades 2 et 3 (niveau à 200–400 Hz constant sur les quatre quarts de la systole, à 10 % près), comme la bibliothèque le décrit (« holosystolic plateau murmur »). L'ancienne mention « moins plateau » n'est pas retenue.
6. **À l'apex, B1 domine B2** chez les cinq chiens à régurgitation mitrale (B2/B1 0,53 à 0,92). Le préréglage « apex » donnait 0,2.

## Corrections appliquées

| Paramètre (`modules/auscultation.json`) | Avant | Après | Calé sur |
|---|---|---|---|
| `s1.split_s`, `s1.second_amp` (nouveaux) | — | 0,022 s ; 0,9 | Durée de B1, tableau 2 |
| `background.band_hz`, `filter_order`, `amp` | 35–220 Hz, ordre 2, 0,010 | 40–155 Hz, ordre 4, 0,014 | Niveau diastolique, tableau 1 |
| `murmur.band_hz`, `filter_order` | 60–250 Hz, ordre 3 | 60–200 Hz, ordre 2 | Pente spectrale du souffle, tableau 3 |
| `murmur.amp`, `murmur.flow_exp` (nouveau) | 0,016 ; racine du débit (0,5) | 0,00054 ; 1,15 | Niveau par grade, tableau 3 |
| `sites.apex.s1_gain`, `s2_gain` | 2,48 ; 0,875 (placeholder D) | 1,5 ; 3,1 (evidence C) | B2/B1 à l'apex, tableau 3 |

- Formule du souffle : niveau = `amp` × (débit régurgitant maximal du battement)^`flow_exp` ; forme dans le battement = √(débit / débit maximal), inchangée.
- Code : `reference/python/signals_pcg_v5.py` et `pcg_beats.py` (référence), puis `crates/vetsim-core/src/pcg.rs`. Trois phases aléatoires de plus par battement (seconde composante de B1). Fixtures de rejeu régénérées ; le Rust reproduit la référence aux mêmes tolérances qu'avant.
- Deux tests modifiés parce que leur attente contredisait les mesures, pas pour les faire passer :
  - `auscultation_properties.rs`, souffle « visible » du MMVD léger : le critère (énergie systolique > 3 × celle du sain) est maintenu mais mesuré dans la bande 200–400 Hz, là où le souffle réel ressort du fond. En large bande, un vrai souffle de grade 3 ne dépasse le fond que de 4 à 5 dB.
  - `auscultation_properties.rs`, site apex : `apex.s1 > apex.s2` devient « le rapport B1/B2 augmente de la base à l'apex », parce que `s1` est l'amplitude d'une seule des deux composantes de B1.

## Écarts non corrigés

- **B1→B2 trop long : 0,26 s contre 0,18 s** (médiane de 20 enregistrements ; 0,184 s chez le beagle sain à FC 91). La cause est la durée de contraction du modèle hémodynamique. Elle n'est pas corrigée ici : le module audio ne compense pas l'hémodynamique. C'est l'écart le plus audible qui reste.
- **Site « base ».** Son rapport B2/B1 (1,93) vient de l'enregistrement *Arrhythmias* 1.1. La bibliothèque décrit cet enregistrement comme pris à l'apex gauche, et c'est le rapport le plus élevé des 20 enregistrements mesurés. L'attribution au site « base » est donc à revoir ; aucun enregistrement explicitement pris à la base chez un chien sans souffle n'est disponible.
- **Souffle au site « base »** : aucun enregistrement de régurgitation mitrale à la base. Le gain du souffle y est le même qu'à l'apex, par défaut.
- **Correspondance grade ↔ débit** (`grade.thresholds_ml_s`) : toujours non calibrée. Le calage garantit seulement que le souffle d'un patient affiché « grade g » a le niveau de l'enregistrement réel de grade g.
- B3 et souffle diastolique : aucun enregistrement exploité.

## Limites

- **Un chien par grade** de souffle, trois enregistrements sans souffle. Les valeurs calées restent de niveau de preuve C.
- Fichiers mp3 (compression avec pertes) et réglage de filtre du stéthoscope inconnu : les enregistrements se répartissent en deux familles spectrales (énergie sous 50 Hz presque nulle, ou dominante). Les niveaux sont donc rapportés à B1 dans la même bande et le même enregistrement, jamais comparés en valeur absolue.
- Le grade 4 est mesuré sur *Examples* 003 ; le fichier *Grading* 4/6 n'a pas pu être segmenté (moins de 3 paires B1/B2 repérées).
- Le repérage de B1 et B2 diffère entre le réel (pics appariés) et la synthèse (instants connus).
- Les grades 1 (souffle à peine distinct, décroissant en début de systole) et 6 (sténose pulmonaire, non segmenté) ne servent pas au calage du souffle.

## Reproduire

```bash
python tests/pcg_calibration_real.py --real DOSSIER_HORS_DEPOT --cmd target/release/vetsim-cli --seeds 8
```

`DOSSIER_HORS_DEPOT` contient les mp3 de la bibliothèque sous leur nom d'origine ; `ffmpeg` est requis.
