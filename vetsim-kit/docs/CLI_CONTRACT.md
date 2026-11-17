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

## Diagnostic (hors contrat, non versionné)
```
vetsim-cli params --bw 20 [--disease mmvd:0.5 ...]
```
Paramètres du patient (objet plat, 73 clés de `Params`) après application des maladies en mode déterministe. Sert à `tests/check_params_cli.py` (comparaison avec la référence Python, rel 1e-9). Peut changer sans incrément d'`api_version`; ne pas l'utiliser depuis une application.

```
vetsim-cli calibrate [--targets calibration/targets.json] [--n-healthy 50] [--n-disease 24] [--seed0 100] [--n-hrv 20]
```
Banc de calibration (jalon M5), même structure que `calibration/calibration_run_python.json` : cohorte saine (graines `0..n`), indicateurs PK, cas par stade de MMVD, MRC et Addison (graines `seed0 + i`), lignes de VFC (`n-hrv` chiens, 300 s ; jalon M6). Stochastique mais reproductible : même graine, même sortie. Comparaison avec la référence : `python tests/compare_calibration.py --cmd target/release/vetsim-cli`.

```
vetsim-cli signals --bw 20 [--disease mmvd:0.8 ...] [--conc ...] [--seed 0] [--duration 8]
```
Signaux d'un patient (jalon M6) : ECG à 500 Hz sur `duration` secondes (`ecg.v`, `ecg.r_times`, `ecg.features`), PCG de 4 battements à 4000 Hz (`pcg.v`, `pcg.features` : B1, B2, B3, souffle, grade), indices de VFC sur 120 s. Stochastique mais reproductible (graine, flux ChaCha distincts pour l'autonome, l'ECG et le PCG ; voir `docs/DETERMINISM.md`).
