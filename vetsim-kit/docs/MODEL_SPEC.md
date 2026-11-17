# Spécification du modèle (résumé normatif ; le code Python fait foi pour tout détail)

Unités : pression mmHg, volume mL, temps s, débit mL/s (L/min en sortie), concentrations mmol/L (g/L pour l'albumine, µmol/L pour la créatinine). `bw` en kg, `s = bw/20`.

## 1. Patient sain
`modules/healthy_dog.json` donne chaque paramètre comme expression de `s`. Allométrie : débits et résistances en `s^0.75`, FC et VO2 par kg en `s^-0.25`, élastances en `1/s`, volumes en `s`. Variabilité interindividuelle : multiplicateur lognormal de moyenne 1.

## 2. Hémodynamique (`physio.simulate_hemo`)
6 compartiments : VG, VD, aorte, veines systémiques, artère pulmonaire, veine pulmonaire/OG. États = volumes `V = [lv, rv, ao, sv, pa, pv]`.
- Cycle `T = 60/hr`, `ts = 0.37·√T`, `tr = 0.4·ts`. Activation `e(t)` : demi-cosinus montant sur `[0, ts)`, descendant sur `[ts, ts+tr)`, 0 ensuite.
- Pression ventriculaire : `P = e·ees·(V−v0) + (1−e)·a·(exp(clip(b(V−v0), −20, 8)) − 1)`.
- Compartiments vasculaires : `P = (V−v0)/C`.
- Valve (diode lisse) : `σ = 0.5(1+tanh(Δp/0.3))`, `Q = σ·Δp/r_valve + (1−σ)·g·Δp` (fuite `g` : `g_mr, g_ar, g_tr, g_pr`).
- Débits : mitrale `(Ppv−Plv)`, aortique `(Plv−Pao)`, tricuspide `(Psv−Prv)`, pulmonaire `(Prv−Ppa)`, systémique `(Pao−Psv)/r_sys`, pulmonaire `(Ppa−Ppv)/r_pul`.
- Volumes initiaux : voir `_initial_volumes` (inclut `bvf` et `vol_shift`).
- Intégration : 11 battements, `rtol=1e-6`, `atol=1e-6`, `max_step=0.004`; les indicateurs sont calculés sur le dernier battement (échantillonnage `dt=0.002`). `reg_fraction` = volume régurgité (parties négatives des débits mitral et aortique) / volume d'éjection total.
- Sorties : `map, sap, dap, co (L/min), edv, esv, ef, sv, reg_fraction, la_mean, la_max, pap_mean, cvp, lv_edp, converged`.

### Oreillettes contractiles (structure, 17/11/2026)
- `pv` = OG + veines pulmonaires, `sv` = OD + veines systémiques. Activation atriale `e_a(t) = act(t + pr, T, ta_atr, tra_atr)` : PR de l'ECG = délai mécanique oreillette → ventricule.
- `P_pv = (1 + k_la·e_a)·(pv − v0_pv)/c_pv`, `P_sv = (1 + k_ra·e_a)·(sv − v0_sv)/c_sv`. Avec `k = 0`, les oreillettes sont passives : c'est le modèle d'origine, identique au bit près.
- Sorties : `mitral_e` et `mitral_a` (pics du débit mitral hors et pendant l'activation atriale), pour caler `k_la` sur le rapport E/A du chien sain.
- `k_la` = 1,45 (calé sur E/A), `k_ra` = 0,29 ; nouvelle référence du 17/11/2026 (`docs/NOUVELLE_REFERENCE.md`).

### Conduction et rythme (`rhythm.py`, `physio._hemo_with_rhythm`)
- Couche de conduction par événements (voir l'en-tête de `rhythm.py`).
- Rythme sinusal 1:1 à PR constant : simulation périodique, avec le PR de la conduction.
- Autre rythme : conduction sur 30 s (aléa SplitMix64, graine 0), puis `simulate_hemo_beats` (moyennes après 10 s). Une asystolie donne un décès `cardiac_arrest`.
- La FC sinusale vaut `hr_factor(...)·hr` (sans `kfac`) ; la FC rapportée est la FC ventriculaire.
- Signaux : décharges sinusales du modèle autonome, puis conduction, puis ECG (`make_ecg_conducted`) et battements du PCG.

## 3. Boucle de régulation (`physio.evaluate`)
Baroréflexe en régime permanent, jusqu'à 5 itérations (arrêt si `|d_t − d| < 0.04`, relaxation 0.5) :
```
s_tot = clip(s0 + 0.8 d, 0, 1);  v_tot = clip(v0 + vagal_add − 0.6 d, 0, 1)
s_eff = s_tot (1 − beta_block);  v_eff = v_tot (1 − vagal_block)
kfac  = max(0.4, 1 − 0.30·max(0, K − 6))
hr'   = hr · hr_factor(s_eff, v_eff) / hr_factor(S_REF, V_REF) · kfac      (S_REF=0.2, V_REF=0.6)
hr_factor(s, v) = (1 + a_s s)(1 − a_v v)
ees'  = ees·(1 + 0.4 (s_eff − s0))   (VG et VD)
r_sys' = r_sys·(1 + 0.5 (s_tot − s0))
vol_shift' = vol_shift + 0.20·85·bw·(s_tot − s0)
d_t = clip(baro_gain·(map_set − MAP)/45, −1, 1)
```

## 4. Perfusion, lactate, rénal
- `do2 = co·1000·CaO2/100`; `vo2 = vo2_kg·bw`; `er = vo2/do2`.
- `lactate = lact0 + 9/(1 + exp(−(er − 0.55)/0.06))`.
- Perfusion rénale : `perf = max(0.25, min(clip((MAP−40)/35, 0, 1), clip((co/co_ref − 0.5)/0.5, 0, 1)))`, `co_ref = 3.29·(bw/20)^0.75` L/min.
- `gfr_ratio = max(0.03, nephron_mass·perf)`; `creat = creat0/gfr_ratio`; `urea = urea0/gfr_ratio^0.8`; `gfr = gfr0·bw·gfr_ratio`; `hct = 3·hb`.

## 5. Gaz du sang et acide-base (`gas_exchange`, `stewart_ph`)
- Stewart : `SID = Na + K − Cl − lactate − xa`; équilibre de charges résolu en pH (brentq sur [5.5, 10.5]) :
  `SID + [H+] − HCO3 − A_alb − A_phos = 0`, avec `[H+] = 1000·10^(−pH)` (nmol/L→mmol/L selon le code), `HCO3 = 0.0307·PCO2·10^(pH−6.1)`,
  `A_alb = alb_gL·(0.28 + 0.469/(1 + 10^(7.77−pH)))` (canin, Constable & Stämpfli 2005), `A_phos = phos·(0.309·pH − 0.469)` (coefficient humain).
- Ventilation : résoudre en PCO2 (brentq sur [5, 250]) `PCO2 = 0.863·VCO2/VA` avec
  `VA = va0·bw·vent_drive·max(0.12, 1 + 0.08 (PCO2−37))·exp(−2.5 (pH−7.4))·(1 + 0.03·max(0, 70 − PaO2))`; 3 itérations externes sur PaO2.
- Oxygénation : `PAO2 = fio2·(pb−47) − PCO2/rq`; gradient `aa = aagrad + edema_k·max(0, la_mean − 18)`; `PaO2 = max(15, PAO2 − aa)`.
- Hémoglobine : Hill `S = P^n/(P^n + P50^n)`, `P50 = p50·10^(0.37 (7.4 − pH))` (Bohr canin), `CaO2 = 1.34·hb·S + 0.003·PO2`.
- Excès de base (van Slyke simplifié) : voir `base_excess`.
- Sang veineux mêlé : `CvO2 = CaO2 − vo2/(co·10)` ; `PvO2` par inversion de la courbe ; `PvCO2 = PaCO2 + 5.5·(125·bw/1000)/max(co, 0.2)` ; pH veineux par Stewart avec `PvCO2`.

## 6. PK/PD (`pkpd.py`)
- 2 compartiments + dépôt oral. `CL = cl·1e-3·60·20·s^0.75·nephron_mass^renal_exp·hepatic_fn^hep_exp` (L/h), `Vc = vc·bw`, `Vp = vp·bw`, `Q = q·1e-3·60·bw`.
- Oral : la dose × `f` va au dépôt (`ka`); IV : dose dans le compartiment central. Concentration = quantité centrale / `Vc` (mg/L).
- Effet Emax : `E = emax·C^hill/(ec50^hill + C^hill)`.
- Furosémide (PD `diuretic`) : `u = 0.25·E(C)` mL/kg/min ; `dD/dt = 0.3·u·bw·60 − D/4` (mL/h) ; `dU/dt = u·60 − U/24`. Effets : `vol_shift −= D`, `K −= 0.04 U`, `Cl −= 0.5 U`, `Na −= 0.3 U`.
- Les effets PD des autres médicaments sont des effets JSON (variable `E`) appliqués aux paramètres.
- **Termes toxiques** (`pd.toxicity`, variable `T`) : `T = emax·C^hill/(ec50^hill + C^hill)` avec des constantes propres, appliqués après l'effet thérapeutique du même médicament. L'effet thérapeutique reste plafonné (occupation des récepteurs) ; la toxicité prolonge la relation dose-réponse au-delà. Opioïde → `vent_drive ×(1 − 0,99 T)` ; bêtabloquant → `ees_lv`, `ees_rv`, `hr ×(1 − 0,8 T)` ; pimobendane → `r_sys ×(1 − 0,8 T)`. EC50 toxique = 10 × EC50, Hill 3 : placeholders D.

## 6 bis. Décompensation et viabilité (`physio.evaluate`, `physio._evaluate_once`)
- `_evaluate_once` = l'ancienne évaluation (baroréflexe, hémodynamique, aval). `evaluate` l'enveloppe d'un point fixe amorti sur deux facteurs, `fc` (élastances VG et VD) et `fr` (`r_sys`) :
  - acidémie : `f_acid = max(0,1 ; 1 − acid_ees·max(0, acid_ph0 − pH))` sur `fc`, `1 − acid_rsys·(…)` sur `fr` ;
  - hypoxie tissulaire : `f_hyp = max(0,1 ; 1 − hypox_ees·max(0, hypox_pvo2 − PvO2))` sur `fc` ;
  - ischémie coronaire : `f_cor = max(0,1 ; 1 − cpp_ees·max(0, cpp_min − (PAD − PTDVG)))` sur `fc` ;
  - `f ← ½(f + f_cible)` jusqu'à `|f_cible − f| < 1e-3`, au plus 30 passes.
- Hypokaliémie (direct) : `vent_drive × max(0,1 ; 1 − k_weak_vent·max(0, k_weak − K))`.
- **Décès** (exception `NonViable`, erreur `Error::NonViable` en Rust) : FC < `hr_arrest` (`cardiac_arrest`) ; DC < `co_arrest_frac`·3,29·(bw/20)^0,75 (`circulatory_collapse`) ; `er ≥ 1`, VO2 > DO2 (`oxygen_debt`) ; racine introuvable (`no_steady_state`) ; pas de convergence (`vicious_circle`).
- **Décompensé** si au moins un mécanisme est actif : les trois couplages, la faiblesse hypokaliémique, la bradycardie hyperkaliémique (K > 6, déjà présente), `er > 0,55` (anaérobiose), POG > 18 (œdème), PAM < 75 (hypoperfusion rénale), tonus sympathique saturé.
- Tous les seuils et pentes sont des paramètres du patient (`healthy_dog.json`), placeholders D. Ils sont inactifs dans la plage du golden et de la calibration.

## 7. Système autonome et VFC (`autonomic.py`)
Simulation à 20 Hz : porte respiratoire (profil `rho`), tonus vagal filtré (`τ_v=0.25 s`), sympathique lent (`τ_s=3 s`, retard 2.5 s, bruit), dérive vagale lente (`vag_noise`, `τ=12 s`), baroréflexe sigmoïde, nœud sinusal à intégration de phase (IPFM) → instants des ondes R → VFC (RR moyen, SDNN, RMSSD, LF/HF, RSA). **Stochastique** : valider statistiquement (`calibration/targets.json`, section `hrv`, tolérance ±1 ET, moyenne sur ≥ 20 graines), pas au point.

## 8. Signaux (`signals.py`)
ECG (dérivation II) : somme de gaussiennes P, Q, R, S, T pilotées par FC et intervalles; effets de K+ (T pointue, P aplatie, PR et QRS allongés, QT corrigé par Fridericia); battements aux instants R du modèle autonome. PCG : sons S1, S2, S3 et souffle déclenchés par les événements valvulaires simulés, souffle proportionnel au débit régurgitant; grade heuristique (seuils `[10, 25, 50, 80, 120]` sur le pic de débit, souffle présent si pic > 3 mL/s). Validation : propriétés qualitatives (présence/absence de souffle, grade monotone avec la sévérité MMVD, T ample si K > 5.5).

## 9. Points sensibles connus
- Solutions non monotones : la crise addisonienne est très sensible au déficit de volume (effondrement de PAM si la volémie stressée devient trop faible).
- RSA nulle si fréquence respiratoire ≥ ~70/min (échantillonnage par battements) : limite connue.
- Les valeurs PD, la PK de l'atropine, du bêtabloquant et de l'opioïde, et le module pneumonie sont des **placeholders**.
