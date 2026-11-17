# Contrat du CLI (`vetsim-cli`)

Sortie : une ligne de JSON sur stdout, code de sortie 0. Erreurs : message sur stderr, code ≠ 0. Mode **déterministe** : aucun bruit, aucune variabilité interindividuelle. Les modules sont lus dans `./modules` (option `--modules DIR`).

## `eval`
```
vetsim-cli eval --bw 20 [--disease mmvd:0.5 ...] [--conc drug=mg_per_L ...] [--D mL] [--U mL_per_kg]
```
`--disease id:sévérité` applique le module (tirages en mode déterministe). `--conc` applique les effets PD des médicaments à la concentration plasmatique donnée. `--D` et `--U` sont le déficit de volume (mL) et la diurèse effective (mL/kg) de la PD du furosémide.

Sortie : objet plat de flottants avec les clés
`hr, map, sap, dap, co, sv, edv, esv, ef, reg_fraction, la_mean, pap_mean, cvp, lv_edp, ph, pco2, hco3, be, pao2, sao2, p50, rr, lactate, ph_v, pco2_v, po2_v, hco3_v, na, k, cl, glucose, creat, urea, phos, alb, hct, gfr, anion_gap, do2_kg, er, s_tone, v_tone`
(unités : mmHg, mL, L/min, µmol/L pour creat, mmol/L pour urea, Na, K, Cl, glucose, phos, lactate; g/L pour alb; voir `reference/python/cli.py`).

## `pk`
```
vetsim-cli pk --bw 20 --drug furosemide --dose 2 --route iv --times 0,0.5,1,2 [--disease ckd:0.6]
```
Dose en mg/kg à t = 0, `route` = `iv` (bolus central) ou `po` (dépôt, biodisponibilité `f`). Sortie : `{"t":[...],"conc_mg_L":[...]}` aux temps demandés (heures), concentration plasmatique centrale.

## Conformité
`python tests/conformance.py --cmd "<chemin du binaire>"` compare toutes les valeurs à `tests/golden/cases.json` selon `tolerances`. Critère : **100 % des valeurs conformes**.
