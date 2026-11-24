# Vérification du modèle vis-à-vis de la littérature

Date : 17/11/2026. Branche `m0-m1-workspace-dsl`. Aucun module, paramètre ni golden n'a été modifié : ce document **rapporte** les écarts, il ne les corrige pas (le Python de `reference/python/` reste normatif).

Usage pédagogique uniquement. Rien ici ne sert à décider d'une dose ou d'un traitement réel.

> **Mise à jour du 18/11/2026.** La PK et la PD du furosémide (synthèse, points 1 et 2) ont été recalées sur Koh 2021, lu en texte intégral, et sur Wong 1977. Les autres corrections et l'état après correction sont dans `docs/VERIFICATION_MALADIES_MEDICAMENTS.tex`. Ce document conserve l'état du 17/11/2026 ; la PAM saine y vaut 101,2 mmHg, contre 109,4 depuis la systole raccourcie.

## Méthode

- **Côté modèle.** Les valeurs viennent du binaire `vetsim-cli` (mode déterministe, chien de 20 kg) :
  - `eval` pour l'état stationnaire ;
  - `pk` pour les concentrations (grille 0,01 h) ;
  - `signals` pour la VFC (10 graines, fenêtre de 120 s).
  
  La diurèse et le K⁺ du furosémide sont recalculés à partir de la courbe `pk` avec les constantes de `reference/python/pkpd.py` : u = 0,25·E(C) mL/kg/min, U avec un oubli de 24 h, ΔK = −0,04·U.
- **Côté littérature.** Cinq recherches ont été menées en parallèle par des sessions IA séparées de ce projet. Ce n'est **pas** une relecture externe. Chaque valeur porte un niveau d'accès :
  - **lu** : passage lu dans la source ;
  - **résumé** : seul le résumé a été lu ;
  - **moteur** : résumé de moteur de recherche, page non ouverte.
- **Références.** Les DOI cités ci-dessous (sauf mention contraire) ont été vérifiés dans OpenAlex le 17/11/2026 (titre, auteurs, année).
- **Statuts.** **OK** : dans l'intervalle publié (±1 ET ou IQR). **Écart** : hors intervalle, chiffré. **Non comparable** : définition ou conditions différentes.

## 1. Chien sain (Haskins 2005, Constable & Stämpfli 2005, Cambier 2004)

Accès : **résumé** pour les trois articles (textes intégraux payants, 403). Haskins : n = 97 chiens vigiles. Le résumé donne des moyennes sans ET.

| Grandeur | Modèle | Littérature | Statut |
|---|---|---|---|
| FC (/min) | 87,6 | 87 | OK |
| PAM (mmHg) | 101,2 | 103 | OK |
| PAP moyenne (mmHg) | 13,9 | 14 | OK |
| POG / PAOP (mmHg) | 5,5 | 5,5 | OK |
| Index cardiaque (L/min/m²) | ≈ 4,37 (DC 3,248 L/min ; SC 0,101·20^(2/3) = 0,744 m², calcul personnel) | 4,42 | OK |
| pH artériel / veineux mêlé | 7,380 / 7,342 | 7,38 / 7,36 | OK |
| PaCO₂ / PvCO₂ (mmHg) | 40,2 / 44,4 | 40,2 / 44,1 | OK |
| PaO₂ / PvO₂ (mmHg) | 98,9 / 50,8 | 99,5 / 49,3 | OK |
| Excès de base artériel (mmol/L) | −1,27 | « base-deficit, −2,1 » | Écart 0,8 mmol/L (signe de la convention à confirmer sur le texte intégral) |
| Extraction d'O₂ | 17,3 % | 20,5 % | Écart −3,2 points |
| P50 (mmHg) | 30,0 (paramètre) | 30,0 ± 1,3 | OK |
| Effet Bohr Δlog P50/ΔpH | −0,37 (code) | −0,370 | OK |
| A_tot albumine (mmol/g) | 0,469, pKa 7,77 (code) | 0,469 ; pKa 7,77 | OK |

Non vérifiés faute de texte intégral : PVC, HCO₃⁻, ET de Haskins, moyennes des électrolytes de Constable, coefficient de Hill (2,7 dans le modèle) et n de Cambier.

## 2. Variabilité de la fréquence cardiaque et arythmie sinusale

**Bogucki & Noszczyk-Nowak 2015** (accès **lu**, PDF) : 50 chiens sains de 6 à 20 kg, Holter de 60 min au repos.

| Grandeur | Modèle sain | Bogucki 2015 (moy ± ET) | Statut |
|---|---|---|---|
| FC | 88,0 | 102,43 ± 29,61 (incohérent : 2017 reprend 122,43 ; NN moyen 677,68 ms ⇒ 88,5) | Non comparable (incohérence de la source) |
| SDNN (ms) | 185 | 208,86 ± 77,1 | OK (fenêtres de 120 s contre 60 min) |
| rMSSD (ms) | 250 | 259 ± 120,17 | OK |
| LF/HF | 0,23 | 0,28 ± 0,11 | OK |

**Bogucki & Noszczyk-Nowak 2017** (accès **lu**) : 20 chiens non traités, MVD stade Cc (ACVIM), même méthode. Comparaison avec la MMVD *s* = 0,7 (stade C) du modèle :

| Grandeur | Modèle C | 2017 (moy ± ET) | Statut |
|---|---|---|---|
| FC | 119,3 | 106,9 ± 24,76 | OK |
| SDNN (ms) | 84 | 138,2 ± 56,23 | Écart : −54 ms (≈ −1 ET) |
| rMSSD (ms) | 85 | 172,66 ± 116,68 | OK (dans l'ET) mais −51 % |
| LF/HF | 0,25 | 0,40 ± 0,13 | Écart : ≈ −1,2 ET ; le modèle ne fait pas monter LF/HF |

**Arythmie sinusale respiratoire et fréquence cardiaque** (17/11/2026). Ce que disent les sources :

- **Billman 2013** (**lu**, 74 chiens vigiles) :
  - au repos, FC et puissance HF sont corrélées à r = −0,51 ;
  - l'atropine (FC 113 → 190) et l'exercice (FC 120 → 182) réduisent fortement HF et SDNN (P < 10⁻⁶), y compris après correction pour la FC ;
  - « cardiac parasympathetic activity is responsible for a major portion of the HRV in the dog ».
- **ACVIM 2019** (**lu**, §6.5.1) : « dogs with marked sinus arrhythmia and relatively slow heart rates also are less likely to have clinical signs attributable to MMVD ».
- **Baisan 2024** (**résumé**) : la %RSA diminue jusqu'au stade Ca et remonte relativement en Cc. La perte de RSA est un facteur pronostique négatif.
- **Grosso 2021** (**résumé**) : RSA absente pendant le halètement chez deux chiens.
- **Billman, exercice** (**moteur**, non lu) : l'indice HF diminue mais reste supérieur à 0.

Ce que fait le modèle (10 graines, 120 s) :

| Cas | FC | SDNN (ms) | RSA (bpm) |
|---|---|---|---|
| Sain | 88 | 185 | 47,3 |
| MMVD B2 | 94 | 164 | 47,1 |
| MMVD C | 119 | 84 | 39,8 |
| MMVD D | 156 | 32 | 24,8 |
| Atropine 0,01 mg/L | 142 | 31 | 17,6 |

Conclusion :

- **Le sens est conforme.** La VFC baisse quand la FC monte (maladie, atropine).
- **L'ampleur ne peut pas être jugée.** Les sources expriment la RSA en ms, ms² ou %RSA, le modèle en bpm d'amplitude. Aucune source lue ne donne une amplitude de RSA en fonction de la FC.
- **Ce qui ressort tout de même :**
  - la RSA du modèle en stade C (40 bpm) n'est pas « perdue » ;
  - l'abolition complète au halètement n'est rapportée que chez 2 chiens (**résumé**) ;
  - l'exercice réduit la RSA sans l'abolir (**moteur**).
- **Correction éventuelle :** à décider. Elle exige d'abord une source chiffrée de la RSA en fonction de la FC.

## 3. MMVD par stade

Stades du modèle : B1 *s* = 0,2 ; B2 0,45 ; C 0,7 ; D 0,9 (et 1,0).

| Grandeur | Modèle | Littérature | Accès | Statut |
|---|---|---|---|---|
| Fraction de régurgitation B1 | 7,0 % | 13,5 % (IQR 1,9–28,3), n = 83 (Kawai 2026) | lu | OK |
| FR B2 | 38,3 % | 53,3 % (38,7–65,8), n = 57 | lu | Écart : sous l'IQR (−0,4 point) |
| FR C | 63,9 % | 60,2 % (52,4–72,1), n = 27 | lu | OK |
| FR, étendue | 7–80 % | 17–81 % (IM aiguë expérimentale, Oyama 2004) | résumé | OK |
| POG baseline | 5,5 mmHg | 5,4 ± 2,5 (Oyama 2004, anesthésiés) | résumé | OK |
| POG en IM | C 16,0 ; D 25,9–32,0 mmHg | 17,4 ± 9,4 (IM aiguë) | résumé | OK (pas de valeur par stade clinique) |
| Seuil d'œdème | 18 mmHg (`gas.rs`, `la_mean − 18`) | > 24 mmHg (Yao & Aoki 2025 citant Guyton & Lindsey 1959, non lu) ; ~20 (MSD, déjà dans PROVENANCE) | lu (citation secondaire) | Écart possible : 18 contre 20–24 |
| FC B1 / B2 | 87 / 93 | 124,0 ± 25,1 / 124,0 ± 25,2 (Santos 2024, ECG en clinique) | lu | Non comparable (contexte clinique ; Haskins = 87 au repos) |
| FC en ICC | C 120 ; D 158 | médiane 150 (IQR 120–169), n = 100 (Yin 2026) | lu | OK |
| PAS en ICC | C 121,5 ; D 114,3 mmHg | médiane 118 (104–131) | lu | OK |
| **Fréquence respiratoire en ICC** | **23,0–24,2 /min** (inchangée) | **médiane 64 /min** (Yin 2026) | lu | **Écart : pas de tachypnée** même avec PaO₂ 68 (*s* = 0,9) |
| FC Holter vs sévérité | croît | croît (Rasmussen 2012) | résumé | OK (sens) |
| Débit cardiaque par stade | — | aucune source trouvée | — | Non vérifié |

Critères ACVIM 2019 (**lu**) :

- B2 : LA:Ao ≥ 1,6, LVIDDN ≥ 1,7, VHS > 10,5 ;
- D : plus de 8 mg/kg/j de furosémide.

Le modèle ne calcule pas ces indices échographiques.

## 4. Pharmacocinétique et pharmacodynamie

### Furosémide (Koh 2021, accès lu, tableaux du texte intégral PMC)

Protocole : 5 beagles de 10 à 13 kg, 2 mg/kg IV ou comprimé oral, plan croisé.

| Grandeur (2 mg/kg) | Modèle | Koh 2021 (moy ± ET) | Statut |
|---|---|---|---|
| C₀ IV (mg/L) | 5,88 | 9,65 ± 1,63 | Écart −39 % (V_c 0,34 contre ≈ 0,21 L/kg implicite) |
| t½ IV (h) | 1,25 | 2,07 ± 0,76 (médiane 1,63) | Écart −1,1 ET. **La cible de `fit_pk.py` (1,25 h, attribuée à BMC Vet Res 2021) ne correspond pas à l'article** |
| Clairance (mL/h/kg) | 300 (fixée dans `fit_pk.py`) | 735,7 ± 63,84 | **Écart : ×0,41** |
| ASC IV (mg·h/L) | 6,67 | 2,74 ± 0,25 | **Écart : ×2,4** |
| V_ss (L/kg) | 0,52 | 0,668 ± 0,223 | OK |
| C_max per os | 0,888 à 0,8 h | 0,61 ± 0,31 à 1 h (0,5–1,5) | OK |
| ASC per os | 2,60 | 1,50 ± 0,73 | Écart +1,5 ET |
| Biodisponibilité F | 0,39 | 0,550 ± 0,265 | OK (dans l'ET) |
| Diurèse sur 8 h, IV | 36,8 mL/kg (supplément seul) | 24,65 mL/kg (total, base comprise) | **Écart : +49 %** |
| Diurèse sur 8 h, per os | 18,6 mL/kg | 30,9 mL/kg | **Écart : −40 % ; ordre IV/oral inversé** |
| ΔK⁺ plasmatique | −1,14 mmol/L à 8 h (IV) ; −0,58 (oral) | IV : −0,51 au nadir (2 h), −0,33 à 8 h ; oral : −0,54 à 2 h, −0,46 à 8 h | **Écart IV : ×3,5 à 8 h** |
| ΔNa⁺ | −8,6 (IV) | pas de variation significative | **Écart** |
| ΔCl⁻ | −14,3 (IV) | ≈ −4 à −6 (110,7 → 104,3–107,6) | **Écart : ×2–3** |

Prises répétées :

- **Wong 1977** (**moteur** seulement) : 1 mg/kg IV par jour pendant 7 jours fait passer K⁺ de 4,48 à 4,09.
- **Roche-Catholy 2021** (résumé de page, quasi littéral) : hypokaliémie chez 4 chiens sur 46 (9 %) en insuffisance cardiaque aiguë.
- Le modèle, avec 2 mg/kg per os toutes les 12 h, donne K⁺ entre 2,4 et 3,0 mmol/L (journal, commit e59a51c). La PD du module est un placeholder (evidence D) ; ces sources indiquent qu'elle surestime nettement la perte de K⁺.

### Pimobendane (Her 2020, accès lu)

**L'article comporte bien un bras oral à 0,5 mg/kg** (n = 7, 8 chiens au total). Le signalement du journal est levé.

| Grandeur | Modèle | Her 2020 | Statut |
|---|---|---|---|
| C_max (ng/mL) | 43,8 | 49,1 ± 28,7 | OK |
| T_max (h) | 0,95 | 2,1 ± 0,9 | Écart −1,3 ET (déjà noté dans le module) |
| t½ (h) | 1,66 | 1,8 ± 0,8 | OK |
| ASC (ng·h/mL) | 149,3 | 148,4 ± 71,6 | OK |
| F | 0,5 (supposée) | non mesurée ; l'introduction cite 70 % (source secondaire) | Non vérifié |

## 5. Rein, Addison, urines normales

### MRC : bornes IRIS (« IRIS Staging of CKD, modified 2026 », iris-kidney.com, accès lu)

Créatinine chez le chien : stade 1 < 125, stade 2 125–250, stade 3 251–440, stade 4 > 440 µmol/L. La source précise que ces bornes valent pour un chien de taille moyenne. Le modèle est en mode déterministe (tirages = 1). Le stade est donné par l'intervalle de sévérité du module.

| *s* (stade du module) | Créatinine modèle (µmol/L) | Stade IRIS de cette créatinine | Statut |
|---|---|---|---|
| 0,05 (IRIS1, borne basse) | 87 | 1 | OK |
| 0,25 (borne IRIS1/IRIS2) | 140 | 2 (borne 125) | Décalage : la créatinine franchit 125 vers *s* ≈ 0,2. Le haut de la plage IRIS1 du module tombe donc en stade 2 |
| 0,5 (borne IRIS2/IRIS3) | 256 | 3 (borne 250) | OK (+2 %) |
| 0,75 (borne IRIS3/IRIS4) | 466 | 4 (borne 440) | Décalage +6 % |

Ce que le modèle ne simule pas :

- la SDMA (bornes de la source : 18 / 35 / 54 µg/dL) ;
- le rapport UP/C (seuils : 0,2 et 0,5).

La sous-stadification tensionnelle de la source (seuils de PAS : 140, 160, 180 mmHg) est possible, mais l'hypertension du modèle est tirée au hasard. En mode déterministe, la PAM reste à 101–103 mmHg.

### Ionogramme de la MRC du modèle (avant comparaison bibliographique)

| *s* | Na | K | Cl | HCO₃⁻ | pH | EB | Trou anionique | Phosphate (mmol/L) | Albumine (g/L) | Ht (%) |
|---|---|---|---|---|---|---|---|---|---|---|
| 0,05 | 150,0 | 4,10 | 113,6 | 23,6 | 7,381 | −1,2 | 16,9 | 1,42 | 37,5 | 44,8 |
| 0,25 | 150,0 | 4,10 | 113,6 | 23,4 | 7,378 | −1,4 | 17,1 | 1,52 | 36,8 | 43,0 |
| 0,5 | 150,0 | 4,10 | 113,6 | 22,2 | 7,361 | −2,8 | 18,3 | 1,82 | 35,8 | 39,4 |
| 0,75 | 150,0 | 4,10 | 113,6 | 20,1 | 7,328 | −5,4 | 20,4 | 2,32 | 34,9 | 34,8 |
| 0,85 | 150,0 | 4,10 | 113,6 | 19,0 | 7,308 | −6,8 | 21,5 | 2,58 | 34,5 | 32,7 |
| 1,0 | 150,0 | 6,10 | 113,6 | 18,8 | 7,304 | −7,1 | 23,7 | 3,02 | 33,9 | 29,2 |

Observations sur le modèle (accès **dépôt**, `modules/diseases/ckd.json`) :

- **Na⁺ et Cl⁻ sont constants** à tous les stades. Le module ne les modifie pas, et le moteur n'a pas de bilan hydrosodé rénal.
- **K⁺ est constant (4,1) jusqu'à *s* = 0,85**, puis il monte linéairement jusqu'à 6,1. Aucune hypokaliémie n'est possible.
- **L'acidose vient uniquement des anions indosés** (`xa` + 6·s²), avec un phosphate en hausse. Elle est donc à trou anionique élevé, sans composante hyperchlorémique.
- Le **calcium** n'est pas modélisé.

La comparaison de cet ionogramme à la littérature figure dans la section 6.

### Addison

| Grandeur | Modèle | Littérature | Accès | Statut |
|---|---|---|---|---|
| Na:K, forme modérée (*s* = 0,55) | 23,1 | seuil 27 : Se 89 %, Sp 97 % ; seuil 24 : Se 79 %, Sp 100 % (Adler 2007, 76 chiens) | résumé | OK (sous les deux seuils) |
| Na:K, forme discrète (*s* = 0,25) | 29,2 | au-dessus du seuil 28 (Se 93 %) | résumé | Cohérent avec une forme discrète |
| Na⁺ en crise (*s* = 0,85) | 131,3 | médiane 137 (116–160), n = 70 (Baumstark 2014) | lu | OK |
| K⁺ en crise | 6,99 | médiane 7,3 (3,3–11,3) | lu | OK |
| Créatinine en crise | 140 µmol/L = 1,58 mg/dL | médiane 1,7 (0,5–11,3) mg/dL dans le groupe gluco- et minéralocorticoïde, n = 72 (Hall 2023) | lu | OK |
| **FC en crise (hyperkaliémie)** | *s* = 0,85 : K 6,99, **FC 106** (tonus sympathique 0,66) ; *s* = 1 : K 7,50, FC 92 | FC < 70 associée à l'hyperkaliémie (8× plus probable ; revue Can Vet J, PMC13142072) ; perte des ondes P (arrêt atrial) et rythmes d'échappement dès K ≥ 7 (PMC13040492, figure adaptée de l'humain) ; BAV de 3e degré transitoire à K 7,9 (10.3390/vetsci12010063) | lu | **Écart** : K⁺ n'agit que par un frein multiplicatif `kfac` sur la FC sinusale (0,70 à K 6,99), que le baroréflexe annule, et par l'aspect de l'ECG (T, P, PR, QRS), sans effet sur la conduction. Le libellé « bradycardie hyperkaliémique » était trompeur et a été renommé « frein hyperkaliémique de la FC ». Correction mécanistique validée : `kfac` remplacé par les effets de K⁺ sur l'excitabilité atriale et la conduction AV (`docs/PROPOSITION_RYTHME.md`) |
| Glycémie | −2 mmol/L chez la moitié des patients | 84 (36–401) mg/dL ; forme glucocorticoïde seule, n = 29 (Reagan 2022) | lu | Non comparable (autre population) |

### Urines et DFG du chien sain (base du futur modèle urinaire)

| Grandeur | Modèle | Littérature | Accès |
|---|---|---|---|
| DFG | 68,2 mL/min pour 20 kg = 3,41 mL/min/kg | iohexol, quartile 13,2–25,5 kg : 1,29–3,50 (Bexfield 2008, n = 118) ; médianes 2,37 (Kelley 2022) et 2,71 (Kelly 2016) | résumé / lu / lu |
| Diurèse | non modélisée (seul le supplément du furosémide l'est) | aucune collecte primaire de 24 h trouvée. Estimation sans source de 20–40 mL/kg/j (Osborne, dvm360, magazine) | lu (source non primaire) |
| Densité urinaire | — | 1,040 ± 0,011 (1,011–1,060), n = 103 (Rudinsky 2019) ; 1,044 ± 0,014, n = 81 (Troìa 2018) | lu |
| Osmolalité urinaire | — | matin 1541 ± 527 mOsm/kg (273–2620), n = 89 (van Vonderen 1997) | résumé |
| FE Na / Cl / K | — | 0,20 % (0,02–1,11) / 0,42 % (0,04–1,71) / 9,47 ± 3,68 %, n = 81 (Troìa 2018) | lu |

Statut DFG : **OK**, en haut de l'intervalle.

## 6. Ionogramme et acide-base de la MRC par stade IRIS

Recherche dédiée. Le modèle est évalué au milieu de chaque plage de stade du module (mode déterministe) :

| Stade | *s* | Créatinine (µmol/L) |
|---|---|---|
| 1 | 0,15 | 110 |
| 2 | 0,375 | 189 |
| 3 | 0,625 | 345 |
| 4 | 0,85 | 592 |

Conversion du phosphate : 1 mmol/L = 3,097 mg/dL.

| Grandeur | Modèle (stades 1 / 2 / 3 / 4) | Littérature | Accès | Statut |
|---|---|---|---|---|
| K⁺ (mmol/L) | 4,10 / 4,10 / 4,10 / 4,10 (puis jusqu'à 6,1 au-delà de *s* = 0,85) | MRC 4,8 (3,3–6,1) contre sains 4,3 (3,6–5,2), P < 0,0001, 50 chiens surtout stade 2 (PMC12881969, JVIM 2026) ; hyperkaliémie au moins une fois chez 47 % de 152 chiens, > 6,5 chez 16 % (Segev 2010, sous régime rénal) | lu / résumé | **Écart** : le modèle n'a ni la hausse modérée du K⁺ dès le stade 2 ni sa variabilité ; l'hyperkaliémie n'apparaît qu'en toute fin de stade 4 |
| Hypokaliémie | impossible dans le modèle | aucune prévalence canine par stade trouvée | — | Non vérifié |
| Na⁺ (mmol/L) | 150 constant | 146,8 chez les vivants comme chez les décédés (PMC13218285) ; aucune différence MRC/sains rapportée | lu | OK (constance) |
| Cl⁻ (mmol/L) | 113,6 constant | MRC 111,7 (102,5–118,8) contre sains 113,1 (108,3–119,2), P = 0,02 | lu | Écart faible (−1,4 non reproduit) |
| HCO₃⁻ < 22 mmol/L | 23,5 / 22,9 / 21,3 / 19,0 : déficit seulement à partir du stade 3 | 43 % / 67 % / 78 % / 76 % des chiens selon le stade (Lippi 2023, n = 277) ; médiane 19 (4–38) | lu | **Écart** : acidose trop tardive et trop faible aux stades 1–2 |
| HCO₃⁻ ≤ 18 (acidose) | jamais (minimum 18,8 à *s* = 1) | 21 % au stade 2, 67 % aux stades 3–4 (PMC12881969) | lu | **Écart** |
| Type d'acidose | trou anionique élevé seulement (TA 17 → 21,5) | aucune donnée canine par stade trouvée | — | Non vérifié |
| Phosphate (mg/dL) | 4,5 / 5,1 / 6,3 / 8,0 | médianes 3,7 / 3,7 / 4,9 / 8,2 (n = 10 / 9 / 12 / 6, Parker 2017) ; hyperphosphatémie (> 5,5) de 18 % au stade 1 à 100 % au stade 4 (Cortadellas 2010) | lu / résumé | Stades 1–3 au-dessus de la médiane mais dans l'étendue ; stade 4 OK. Le phosphate du chien sain (1,42 mmol/L = 4,4 mg/dL) est déjà au-dessus des médianes des stades 1–2 |
| Calcium total / ionisé | non modélisé | total stable (10,6–11,1 mg/dL) ; ionisé bas chez 4/37 (Parker 2017) ; hypocalcémie ionisée chez 10–56 % (revue secondaire, PMC13314447) | lu | Absent du modèle |
| Hématocrite (%) | 44,1 / 41,4 / 37,2 / 32,7 | médianes stades 2 / 3 / 4 : 37,6 / 32,1 / 27,3 ; anémie 47 % / 71 % / 82 % (Lippi 2021) | lu | **Écart** : anémie trop faible de 4 à 5 points d'Ht |
| Albumine (g/L) | 37,1 / 36,3 / 35,3 / 34,5 | 30 (26–35), surtout stade 2 (PMC13218285) | lu | Écart (cohorte unique) |

Implications pour l'ionogramme du patient virtuel :

1. Na⁺, Cl⁻ et K⁺ ne réagissent pas à la sévérité avant la toute fin du stade 4. Un étudiant ne voit donc ni la tendance hyperkaliémique ni la légère baisse du Cl⁻.
2. L'acidose métabolique est trop tardive : HCO₃⁻ < 22 seulement à partir du stade 3, jamais ≤ 18. Le trou anionique augmente seul.
3. L'hyperphosphatémie est présente mais commence trop haut.
4. Le calcium est absent.
5. Le modèle n'a pas de bilan rénal de K⁺, Na⁺, Cl⁻ ni H⁺ : les valeurs sont des décalages fixés par sévérité. Le futur modèle urinaire (excrétion fractionnelle, ammoniogenèse) serait l'endroit mécanistique pour les faire émerger.

## Synthèse des écarts à arbitrer

1. **PK du furosémide**
   - La clairance fixée à 0,30 L/h/kg et la cible t½ = 1,25 h de `fit_pk.py` ne correspondent pas à Koh 2021 : 0,74 L/h/kg et 2,07 h.
   - L'ASC IV est ×2,4.
2. **PD du furosémide** (placeholder D)
   - Diurèse : +49 % en IV, −40 % per os.
   - Pertes : K⁺ ×3,5, Cl⁻ ×2–3, Na⁺ alors que l'article n'en observe pas.
   - Conséquence : hypokaliémie marquée en prises répétées.
   - Le futur modèle d'urine pourrait remplacer ces coefficients.
3. **Fréquence respiratoire** constante en ICC : 23 /min contre une médiane de 64.
4. **VFC en stade C** : SDNN trop bas, LF/HF sans hausse.
5. **Seuil d'œdème** : 18 mmHg contre 20–24 dans les sources.
6. **Pimobendane** : T_max trop précoce.
7. **Hyperkaliémie (Addison)** : FC 106 à K 6,99 au lieu d'une bradycardie ; K⁺ n'agit pas sur la conduction (correction prévue avec la couche de rythme).
8. **MRC**
   - Na⁺, Cl⁻ et K⁺ sont constants jusqu'au stade 4, alors que les sources rapportent K⁺ 4,8 contre 4,3 dès le stade 2.
   - L'acidose (HCO₃⁻ < 22) n'apparaît qu'au stade 3, contre 67 % des chiens dès le stade 2.
   - L'hématocrite est trop haute de 4 à 5 points.
   - Le calcium est absent (section 6).
   - La créatinine du haut de la plage IRIS1 tombe déjà en stade 2.

Toute correction touche la référence normative et attend une décision du projet.

## Recherches et sources bloquées

Une source bloquée n'est pas une preuve d'absence.

- **Bloquées (403, mur de cookies ou délai dépassé) :**
  - PubMed ;
  - Wiley, IngentaConnect et ScienceDirect (textes intégraux de Haskins, Constable, Cambier, Rasmussen) ;
  - Elsevier (Baisan 2024) ;
  - Springer (Wong 1977) ;
  - air.unimi.it (Martinelli, résumé ECVIM 2019) ;
  - WebFetch vers PMC, BMC, Frontiers et Illinois Experts (délai dépassé ; contourné par l'API BioC ou Europe PMC quand c'était possible).
- **Bloqués aussi (rein, Addison, urines) :**
  - reCAPTCHA de PMC (contourné par Europe PMC) ;
  - Wiley 403 (Seth 2011) ;
  - texte intégral d'Adler 2007 non obtenu ;
  - Chalifoux 2023 (Can Vet J, crise addisonienne, 84 chiens) : corps du texte inaccessible.
- **Non lus :**
  - Guyton & Lindsey 1959 (original) ;
  - texte intégral d'Oyama 2004 ;
  - textes intégraux d'Oliveira 2012, Grosso 2021 et Moïse 2021.
- **Bloqués aussi (ionogramme de la MRC) :**
  - texte intégral de Cortadellas 2010 (Cloudflare 403) ;
  - réponses vides de BioC (contournées par Europe PMC) ;
  - revues de Polzin et Bartges 2012 non atteintes.
- **Non cherchés :** Crosara 2010, essais LOOK-Mitral, EPIC et QUEST, Yuchi 2024 (POG et débit par stade possibles).

## Références (DOI vérifiés dans OpenAlex le 17/11/2026)

- Haskins S et al. Reference cardiopulmonary values in normal dogs. Comp Med 2005;55(2):156–161. PMID 15884778.
- Constable PD, Stämpfli HR. Experimental determination of net protein charge and A_tot and K_a of nonvolatile buffers in canine plasma. J Vet Intern Med 2005;19(4):507–514. doi:10.1111/j.1939-1676.2005.tb02719.x
- Cambier C et al. Haemoglobin oxygen affinity and regulating factors of the blood oxygen transport in canine and feline blood. Res Vet Sci 2004;77(1):83–88. doi:10.1016/j.rvsc.2004.02.004
- Bogucki Sz, Noszczyk-Nowak A. Short-term heart rate variability (HRV) in healthy dogs. Pol J Vet Sci 2015;18(2):307–312. doi:10.1515/pjvs-2015-0040
- Bogucki Sz, Noszczyk-Nowak A. Short-term heart rate variability in dogs with sick sinus syndrome or chronic mitral valve disease as compared to healthy controls. Pol J Vet Sci 2017;20(1):167–172. doi:10.1515/pjvs-2017-0021
- Billman GE. The effect of heart rate on the heart rate variability response to autonomic interventions. Front Physiol 2013;4:222. doi:10.3389/fphys.2013.00222
- Baisan RA et al. Quantification of respiratory sinus arrhythmia in control dogs and in dogs with various severity classes of myxomatous mitral valve disease. J Vet Cardiol 2024;55:48–56. doi:10.1016/j.jvc.2024.08.001
- Rasmussen CE et al. Heart rate, heart rate variability, and arrhythmias in dogs with myxomatous mitral valve disease. J Vet Intern Med 2012;26:76–84 (en ligne 2011). doi:10.1111/j.1939-1676.2011.00842.x
- Keene BW et al. ACVIM consensus guidelines for the diagnosis and treatment of myxomatous mitral valve disease in dogs. J Vet Intern Med 2019;33:1127–1140. doi:10.1111/jvim.15488
- Oyama MA, Sisson DD, Bulmer BJ, Constable PD. Echocardiographic estimation of mean left atrial pressure in a canine model of acute mitral valve insufficiency. J Vet Intern Med 2004;18:667–672. doi:10.1111/j.1939-1676.2004.tb02604.x
- Kawai S et al. Quantitative mitral regurgitation and conventional echocardiographic variables in canine myxomatous mitral valve disease. Front Vet Sci 2026;13:1885087. doi:10.3389/fvets.2026.1885087
- Yin CY, Lu TL, Rush JE, Lee TJ, Chang CY. Clinical findings and risk factors for clinical outcomes in dogs with myxomatous mitral valve disease hospitalized for cardiogenic pulmonary edema. Front Vet Sci 2026;13:1749038. doi:10.3389/fvets.2026.1749038
- Santos BA et al. Study of the arrhythmogenic profile of dogs with myxomatous mitral valve disease in stages B1 and B2. Vet Sci 2024;11:467. doi:10.3390/vetsci11100467
- Yao J, Aoki T. Impact of mannitol on left atrial pressure in dogs with mitral regurgitation. Vet Med Sci 2025;11(2):e70274. doi:10.1002/vms3.70274
- Koh SK et al. Pharmacokinetics and diuretic effect of furosemide after single intravenous, oral tablet, and newly developed oral disintegrating film administration in healthy beagle dogs. BMC Vet Res 2021;17:295. doi:10.1186/s12917-021-02998-4
- Her J et al. Pharmacokinetics of pimobendan and its metabolite O-desmethyl-pimobendan following rectal administration to healthy dogs. Front Vet Sci 2020;7:423. doi:10.3389/fvets.2020.00423
- Wong et al. 1977, Can Anaesth Soc J, doi:10.1007/BF03006233. **Non vérifié** (moteur seulement).
- IRIS Ltd. IRIS Staging of CKD (modified 2026). https://www.iris-kidney.com/s/IRIS_staging_guidelines-2026.pdf
- Adler JA, Drobatz KJ, Hess RS. Abnormalities of serum electrolyte concentrations in dogs with hypoadrenocorticism. J Vet Intern Med 2007;21(6):1168–1173. doi:10.1111/j.1939-1676.2007.tb01933.x
- Baumstark ME et al. Evaluation of aldosterone concentrations in dogs with hypoadrenocorticism. J Vet Intern Med 2014;28(1):154–159. doi:10.1111/jvim.12243
- Hall HR et al. Prevalence of hypercalcemia in primary hypoadrenocorticism in dogs: multicenter, retrospective study. J Vet Intern Med 2023. doi:10.1111/jvim.16786
- Reagan KL et al. Characterization of clinicopathologic and abdominal ultrasound findings in dogs with glucocorticoid deficient hypoadrenocorticism. J Vet Intern Med 2022. doi:10.1111/jvim.16564
- Bexfield N et al. Glomerular filtration rate estimated by 3-sample plasma clearance of iohexol in 118 healthy dogs. J Vet Intern Med 2008;22(1):66–73. doi:10.1111/j.1939-1676.2007.0035.x
- Kelley MD et al. Effects of an intravenous infusion of diltiazem on glomerular filtration rate, electrolyte excretion, and urine output in healthy dogs. J Vet Intern Med 2022. doi:10.1111/jvim.16575
- Kelly KL, Drobatz KJ, Foster JD. Effect of fenoldopam continuous infusion on glomerular filtration rate and fractional excretion of sodium in healthy dogs. J Vet Intern Med 2016;30:1655–1660. doi:10.1111/jvim.14522
- Troìa R et al. Fractional excretion of electrolytes in volume-responsive and intrinsic acute kidney injury in dogs. J Vet Intern Med 2018. doi:10.1111/jvim.15146
- Rudinsky AJ et al. Variability of first morning urine specific gravity in 103 healthy dogs. J Vet Intern Med 2019;33:2133–2137. doi:10.1111/jvim.15592
- van Vonderen IK, Kooistra HS, Rijnberk A. Intra- and interindividual variation in urine osmolality and urine specific gravity in healthy pet dogs of various ages. J Vet Intern Med 1997;11(1):30–35. doi:10.1111/j.1939-1676.1997.tb00070.x
- Segev G, Fascetti AJ, Weeth LP, Cowgill LD. Correction of hyperkalemia in dogs with chronic kidney disease consuming commercial renal therapeutic diets by a potassium-reduced home-prepared diet. J Vet Intern Med 2010;24:546–550. doi:10.1111/j.1939-1676.2010.0488.x
- Lippi I et al. Serum bicarbonate deficiency in dogs with acute and chronic kidney disease. Vet Sci 2023;10:363. doi:10.3390/vetsci10050363
- Parker VJ et al. Association of vitamin D metabolites with parathyroid hormone, fibroblast growth factor-23, calcium, and phosphorus in dogs with various stages of chronic kidney disease. J Vet Intern Med 2017;31:791–798. doi:10.1111/jvim.14653
- Cortadellas O et al. Calcium and phosphorus homeostasis in dogs with spontaneous chronic kidney disease at different stages of severity. J Vet Intern Med 2010;24:73–79. doi:10.1111/j.1939-1676.2009.0415.x
- Lippi I et al. Erythrogram patterns in dogs with chronic kidney disease. Vet Sci 2021;8:123. doi:10.3390/vetsci8070123
- PMC12881969 et PMC13218285 (J Vet Intern Med 2026, excrétion d'ammoniac dans la MRC canine) et PMC13314447 (Front Vet Sci 2026) : **auteurs et DOI non vérifiés**.
- Roche-Catholy M et al. Clinical relevance of serum electrolytes in dogs and cats with acute heart failure. J Vet Intern Med 2021;35(4):1652–1662. PMC8295692. **Non vérifié dans OpenAlex.**
