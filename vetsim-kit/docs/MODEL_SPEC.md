# Spécification du modèle (résumé normatif ; le code Rust de `crates/vetsim-core` fait foi pour tout détail, le Python de `reference/python/` est secondaire)

Unités : pression mmHg, volume mL, temps s, débit mL/s (L/min en sortie), concentrations mmol/L (g/L pour l'albumine, µmol/L pour la créatinine). `bw` en kg, `s = bw/20`.

## 1. Patient sain
`modules/healthy_dog.json` donne chaque paramètre comme expression de `s`. Allométrie : débits en `s^0.75`, résistances en `s^-0.75`, FC et VO2 par kg en `s^-0.25`, élastances en `1/s`, volumes en `s`. Variabilité interindividuelle : multiplicateur lognormal de moyenne 1.

## 2. Hémodynamique (`physio.simulate_hemo`)
6 compartiments : VG, VD, aorte, veines systémiques, artère pulmonaire, veine pulmonaire/OG. États = volumes `V = [lv, rv, ao, sv, pa, pv]`.
- Cycle `T = 60/hr`, `ts = ts_coef·√T` (`ts_coef` = 0,325 depuis le 17/11/2026), `tr = 0.4·ts`. Activation `e(t)` : demi-cosinus montant sur `[0, ts)`, descendant sur `[ts, ts+tr)`, 0 ensuite.
- Pression ventriculaire : `P = e·ees·(V−v0) + (1−e)·a·(exp(clip(b(V−v0), −20, 8)) − 1)`.
- Compartiments vasculaires : `P = (V−v0)/C`.
- Valve (diode lisse) : `σ = 0.5(1+tanh(Δp/0.3))`, `Q = σ·Δp/r_valve + (1−σ)·g·Δp` (fuite `g` : `g_mr, g_ar, g_tr, g_pr`, normalisées au chien de 20 kg et multipliées par `(bw/20)^0.75` dans l'hémodynamique, comme les débits et `1/r_valve` ; `hemo::leak_scale`, 18/11/2026).
- **Orifice mitral dynamique** (18/11/2026, `engine::hemo_with_rhythm`) : l'orifice effectif grandit avec le VG (Yoran 1979, chiens, résumé). Il est fixe pendant le battement : `g = g_mr·(VTD/mr_vref)^mr_k`, point fixe amorti sur ln g (relaxation ½, tolérance 1e-3, au plus 12 simulations), ignoré si `g_mr < 0,05`. `mr_k` = 2,5 et `mr_vref` = 68,5·s mL (VTD du chien sain depuis le recalage de la systole du 18/11/2026) ; calé sur Suzuki 2013 (furosémide 0,17 mg/kg/h × 6 h, IM expérimentale : POG −27 %, modèle −27 %). La conductance retenue est reportée dans les paramètres de l'évaluation (`g_mr`, `mr_k = 0`) pour les recalculs en aval.
- **Capacitance veineuse** `ven_cap` (1 au repos) : multiplie `c_sv` dans la pression veineuse systémique, pas dans le volume initial (une veinodilatation redistribue le volume sans en ajouter).
- Débits : mitrale `(Ppv−Plv)`, aortique `(Plv−Pao)`, tricuspide `(Psv−Prv)`, pulmonaire `(Prv−Ppa)`, systémique `(Pao−Psv)/r_sys`, pulmonaire `(Ppa−Ppv)/r_pul`.
- Volumes initiaux : voir `_initial_volumes` (inclut `bvf` et `vol_shift`).
- Intégration : 22 battements (11 avant le 20/11/2026 : dernier cycle non périodique en hypovolémie), doublés tant que la PAM dérive de plus de 0,5 % entre les deux derniers (`hemo::MAP_DRIFT_TOL`, au plus 176), `rtol=1e-6`, `atol=1e-6`, `max_step=0.004`; les indicateurs sont calculés sur le dernier battement (échantillonnage `dt=0.002`). `reg_fraction` = volume régurgité (parties négatives des débits mitral et aortique) / volume d'éjection total.
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
hr'   = hr · hr_factor(s_eff, v_eff) / hr_factor(S_REF, V_REF)      (S_REF=0.2, V_REF=0.6 ; K⁺ agit par la conduction)
hr_factor(s, v) = (1 + a_s s)(1 − a_v v)
ees'  = ees·(1 + 0.4 (s_eff − s0))   (VG et VD)
r_sys' = r_sys·(1 + 0.5 (s_tot − s0))
vol_shift' = vol_shift + 0.20·85·bw·(s_tot − s0)
d_t = clip(baro_gain·(map_set − MAP)/45, −1, 1)
```

## 4. Perfusion, lactate, rénal
- `do2 = co·1000·CaO2/100`; `vo2 = vo2_kg·bw`; `er = vo2/do2`.
- `lactate = lact0 + 9/(1 + exp(−(er − 0.55)/0.06))`.
- Perfusion rénale : `perf = max(0.25, min(clip((MAP−40)/35, 0, 1), clip((co/co_ref − (renal_co_auto − 0.5))/0.5, 0, 1)))`, `co_ref = 3.29·(bw/20)^0.75` L/min. Autorégulation vis-à-vis du débit (21/11/2026) : `renal_co_auto` 0,8, le débit ne limite la filtration qu'au-dessous de 0,8·co_ref (1,0 redonne la rampe historique 0,5–1,0).
- Axe cardio-rénal (20/11/2026) : `f_cong = max(0,3 ; 1 − renal_k_cong·max(0, PVC − renal_cvp_ref))`, `f_symp = max(0,3 ; 1 − renal_k_symp·max(0, s − renal_s_ref))` ; anémie : `r_sys ×= (hb/hb_ref)^anemia_visc` dans la boucle de décompensation.
- `gfr_ratio = max(0.03, nephron_mass·perf·f_cong·f_symp)`; `creat = creat0/gfr_ratio`; `urea = urea0/gfr_ratio^0.8`; `gfr = gfr0·bw·gfr_ratio`; `hct = 3·hb`.

## 5. Gaz du sang et acide-base (`gas_exchange`, `stewart_ph`)
- Stewart : `SID = Na + K − Cl − lactate − xa`; équilibre de charges résolu en pH (brentq sur [5.5, 10.5]) :
  `SID + [H+] − HCO3 − A_alb − A_phos = 0`, avec `[H+] = 1000·10^(−pH)` (nmol/L→mmol/L selon le code), `HCO3 = 0.0307·PCO2·10^(pH−6.1)`,
  `A_alb = alb_gL·(0.28 + 0.469/(1 + 10^(7.77−pH)))` (canin, Constable & Stämpfli 2005), `A_phos = phos·(0.309·pH − 0.469)` (coefficient humain).
- Ventilation : résoudre en PCO2 (brentq sur [5, 250]) `PCO2 = 0.863·VCO2/VA` avec
  `VA = va0·bw·vent_drive·max(0.12, 1 + 0.08 (PCO2−37))·exp(0.05 − 6.0 (pH−7.38))·(1 + 0.03·max(0, 70 − PaO2))` (gain au pH recalé le 20/11/2026 : compensation 0,74 mmHg/(mmol/L), Madias 1984) ; 3 itérations externes sur PaO2.
- Oxygénation : `PAO2 = fio2·(pb−47) − PCO2/rq`; gradient `aa = aagrad + edema_k·max(0, la_mean − LA_EDEMA)` avec `LA_EDEMA` = 24 mmHg (18 avant le 21/11/2026, Guyton & Lindsey 1959); `PaO2 = max(15, PAO2 − aa)`.
- Hémoglobine : Hill `S = P^n/(P^n + P50^n)`, `P50 = p50·10^(0.37 (7.4 − pH))` (Bohr canin), `CaO2 = 1.34·hb·S + 0.003·PO2`.
- Excès de base (van Slyke simplifié) : voir `base_excess`.
- Sang veineux mêlé : `CvO2 = CaO2 − vo2/(co·10)` ; `PvO2` par inversion de la courbe ; `PvCO2 = PaCO2 + 5.5·(125·bw/1000)/max(co, 0.2)` ; pH veineux par Stewart avec `PvCO2`.

## 6. PK/PD (`pkpd.rs` ; port secondaire `pkpd.py`)
- 2 compartiments + dépôt oral. `CL = cl·1e-3·60·20·s^0.75·nephron_mass^renal_exp·hepatic_fn^hep_exp` (L/h), `Vc = vc·bw`, `Vp = vp·bw`, `Q = q·1e-3·60·bw`.
- Oral : la dose × `f` va au dépôt (`ka`); IV : dose dans le compartiment central ; IM/SC : la dose × `f_im` va à un dépôt propre (`ka_im_per_h`). Concentration = quantité centrale / `Vc` (mg/L). 5 états par médicament : dépôt digestif, central, périphérique, dépôt IM, Ce.
- **Compartiment d'effet** (`pd.ke0_per_h`) : `dCe/dt = ke0·(C − Ce)` ; `E` est alors calculé sur `Ce`. Un terme toxique choisit son site par `site: "plasma"` (défaut) ou `"effect"`. La dissociation des deux sites produit les effets biphasiques (atropine : bradycardie paradoxale au site plasmatique, blocage vagal au site d'effet).
- Effet Emax : `E = emax·C^hill/(ec50^hill + C^hill)`.
- Furosémide (PD `diuretic`, module 1.2.0, recalé le 18/11/2026 sur Koh 2021, Adin 2020 et l'essai TEST) : `u = urine_max·E(X)` mL/kg/min avec `urine_max = 0,2044`, `X = C·nephron_mass` (`drive: urinary_excretion` : l'effet suit l'excrétion urinaire, Miyazaki 1990) ; `dD/dt = f_plasma·u·bw·60 − D/τ_refill` (mL/h ; `f_plasma` 0,3 et `τ_refill` 4 h : placeholders D) ; `dU/dt = u·60 − U/τ_elect` (`τ_elect` 200 h) ; adaptation `dW/dt = (U − W)/τ_adapt` (`adapt_tau_h` 12 h, placeholder D) et `U_eff = U − φ·W` (`adapt_frac` φ 0,9808) : c'est `U_eff` que reçoivent les effets. Effets : `vol_shift −= D` ; `ven_cap ×= 1 + 0,36·E` (veinodilatation dépendante du rein, Bayne 1979) ; pertes **proportionnelles à la concentration**, `K ×= exp(−0,0030·U_eff)` (0,00585 avant le 20/11/2026), `Cl ×= exp(−0,00225·U_eff)`, `Na ×= exp(−0,00032·U_eff)` : jamais négatives. Azotémie prérénale (20/11/2026, Giorgi 2022) : `nephron_mass ×= max(0,5 ; 1 − 3,4·D/Vb)`. Pour un diurétique à `drive: urinary_excretion`, `E` des effets est calculé sur `C·nephron_mass`.
- Les effets PD des autres médicaments sont des effets JSON (variable `E`) appliqués aux paramètres.
- **Termes toxiques** (`pd.toxicity`, variable `T`) : `T = emax·C^hill/(ec50^hill + C^hill)` avec des constantes propres, appliqués après l'effet thérapeutique du même médicament. L'effet thérapeutique reste plafonné (occupation des récepteurs) ; la toxicité prolonge la relation dose-réponse au-delà. Opioïde → `vent_drive ×(1 − 0,8 T)` (EC50 toxique 1 mg/L : plateau de PaCO2 ≈ 65 mmHg sans apnée chez le chien vigile, Bailey 1987) ; pimobendane → `r_sys ×(1 − 0,8 T)` (EC50 toxique = 10 × EC50, Hill 3, placeholder D). Bêtabloquant (profil aténolol) : pas de terme toxique depuis le 18/11/2026. Atropine 1.1.0 (19/11/2026) : trois termes, effet vagal paradoxal (plasma, `vagal_add += 0,2 T`), blocage muscarinique périphérique (site d'effet, `musc_block += T`) et tachycardie excessive (plasma, `hr ×(1 + 0,2 T)`).
- Opioïde (chien vigile, 18/11/2026) : `vent_drive ×(1 − 0,25 E)`, `vagal_add += 0,27 E`, `r_sys ×(1 + 0,36 E)` : FC −32 %, PAM +6 %, PaCO2 +3 mmHg à effet maximal (Mills 2025, Maiante 2009, Wunsch 2010).
- Furosémide 1.3.0 (19/11/2026) : hémoconcentration `hb ×= Vb/(Vb − D)` et `alb ×= Vp/(Vp − D)`, avec `Vb = 85·bvf·bw` mL et `Vp = Vb·(1 − Ht)` : le déficit est prélevé sur le plasma. Les effets d'un module diurétique disposent donc des variables `E, D, U, Vb, Vp`.
- Pimobendane 1.1.0 : `pr ×(1 − 0,25 E)` et `av_erp ×(1 − 0,25 E)` (PQ −11,8 % à 0,15 mg/kg IV, Pichayapaiboon 2021).
- Opioïde 1.2.0 : en plus de ce qui précède, `t_set −= 2,0 E`, `pupil_ew += E`, `gi_inhib += 0,9 E`, `sedation += 0,2 E`.
- **Créatinine et urée en évolution temporelle** (`kinetics.rs`, API `timecourse` et `state` avec `doses`/`at_h`) : compartiment unique `V·dC/dt = P − DFG(t)·C`, soit `dC/dt = (DFG/V)(C_ss − C)`, départ à l'équilibre du patient sans médicament ; `V = creat_vd·bw` (0,539 L/kg, Collignon 2012) et `urea_vd·bw` (0,66 L/kg, Bowsher 1985). L'état stationnaire (CLI `eval`, golden) reste `C_ss = C0/ratio_DFG`. Non porté en Python.

## 6 ter. Rein dynamique (`renal.rs`, 21/11/2026 ; pas de port Python)
- Évolution temporelle seulement (API `renal_course`) ; point fixe = état stationnaire. Apports Na et K = ceux d'un chien sain de même poids (`gfr0·bw·Na·fe_na0`), indépendants de la maladie ; FE de base du patient = apport/charge filtrée (×1/ratio_DFG en MRC) ; leviers `na_intake_factor`, `k_intake_factor`. `FE_K ≤ fe_k_max`.
- `FE_Na = (FE_Na,b + fe_loop·E·(1 − φ·B))·A^(−a_na)·(PAM/PAM_b)^n_p` ; freinage `dB/dt = (E − B)/τ` (τ_on si E > B, sinon τ_off) ; DFG effectif `DFG_ss·(DFG_b/DFG_ss)^E·exp(−g_gfr·max(0, −V)/VEC·(1 − E))` (macula densa aveugle pendant l'exposition, rétrocontrôle tubulo-glomérulaire ensuite ; `C_ss` rapporté au DFG effectif) ; diurèse `u0 + 1000·(UNa − Na_in)/Na_u + w_max·W·bw`, `dW/dt = (E − W)/τ_w` (eau libre) ; `dV/dt = 1000·(Na_in − DFG·Na·FE_Na)/Na` (VEC, iso-osmotique) ; plasma `Vp/VEC·V` → `vol_shift`, `hb`, `alb`.
- `UK = DFG·(K_b·FE_K,b + σ·(K − K_b))·(FE_Na/FE_Na,b)^k_flow·(A·A_K)^a_k` (≤ DFG·K·fe_k_max) : réserve d'excrétion proportionnelle au DFG (σ_up au-dessus, σ_down en dessous de K_b) ; K extra/intracellulaire : `dKe/dt = K_in + K_abs − UK − J`, `J = (B·bw·ΔK − Ki)/τ_ic` ; charges orales de K (`k_loads`, absorption en deux étapes de constante τ_abs).
- Aldostérone en deux composantes (« paradoxe de l'aldostérone ») : `A` (rénine) : `dA/dt = (exp(g·max(0, −V)/VEC)·(1 + g_md·E)·(1 − IEC) − A)/τ_A` (déplétion et macula densa), agit sur Na avec latence (`A_n`, `dA_n/dt = (A − A_n)/τ_n`, `A_n^−a_na`) et sur K ; `A_K` (kaliémie) : `dA_K/dt = ((K − k0)/(K_b − k0) − A_K)/τ_A` (k0 = 3,29, Young 1984), agit sur K seulement. Créatinine et urée : `dC/dt = (DFG/V_d)(C_ss − C)`.
- Médicaments : effets directs avec `D = U = 0` (termes calés du diurétique stationnaire neutres) ; `E` du diurétique de l'anse sur la natriurèse.
- Physiologie recalculée à chaque pas `dt_h` (0,25 h) si un paramètre a bougé de plus de 0,2 % (K : 0,03 mmol/L).
- Domaine validé (1.4.0) : furosémide, prise unique ou répétée, ≤ 14 j, sans levier alimentaire, charge de K ni IEC (calage Koh, Bourgoignie, Pelligand 2020, Adin 2021 ; validation Giorgi 2022, profil de créatinine d'Adin, rein restant). Écarts : docs/VALIDATION_HORS_CALIBRAGE.tex.

## 6 bis. Décompensation et viabilité (`physio.evaluate`, `physio._evaluate_once`)
- `_evaluate_once` = l'ancienne évaluation (baroréflexe, hémodynamique, aval). `evaluate` l'enveloppe d'un point fixe amorti sur deux facteurs, `fc` (élastances VG et VD) et `fr` (`r_sys`) :
  - acidémie : `f_acid = max(0,1 ; 1 − acid_ees·max(0, acid_ph0 − pH))` sur `fc`, `1 − acid_rsys·(…)` sur `fr` ;
  - hypoxie tissulaire : `f_hyp = max(0,1 ; 1 − hypox_ees·max(0, hypox_pvo2 − PvO2))` sur `fc` ;
  - réponse à l'hypoxémie (20/11/2026) : `h = clip((hypox_pao2 − PaO2)/(hypox_pao2 − hypox_pao2_sat), 0, 1)^hypox_n`, itérée avec `fc`, `fr` : `s0 += hypox_symp·h`, `ees ×= 1 + hypox_inotropy·h`, `r_sys ×= max(0,2 ; 1 − hypox_vaso·h)` (Krebs 1999, Krasney & Koehler 1977) ;
  - ischémie coronaire : `f_cor = max(0,1 ; 1 − cpp_ees·max(0, cpp_min − (PAD − PTDVG)))` sur `fc` ;
  - `f ← ½(f + f_cible)` jusqu'à `|f_cible − f| < 1e-3`, au plus 30 passes.
- Hypokaliémie (direct) : `vent_drive × max(0,1 ; 1 − k_weak_vent·max(0, k_weak − K))`.
- **Solveur du baroréflexe** (18/11/2026, `engine::baro_next`) : le point fixe `g(d) = d_t(d) − d = 0` est résolu sur [−1, 1].
  - Premier pas amorti `d + g/2`, puis sécante, avec bisection de l'encadrement si la sécante en sort.
  - Arrêt si |g| < 0,01, au plus 16 évaluations.
  - Il remplace l'itération amortie (5 passages, |g| < 0,04), qui pouvait s'arrêter sur un état non convergé.
- **Effet dromotrope du sympathique** (18/11/2026) : à chaque passage du baroréflexe, `av_erp` et le délai AH (`pr − hv`) sont multipliés par `max(0,1 ; 1 − av_symp·(s_eff − 0,2))`, `av_symp` = 0,5 (dénervation sympathique en FA : FC ventriculaire −16 %, PMC7872775). Le bêtabloquant (qui réduit `s_eff`) ralentit donc la réponse ventriculaire de la FA.
- **Décès** (exception `NonViable`, erreur `Error::NonViable` en Rust) : FC < `hr_arrest` (`cardiac_arrest`) ; DC < `co_arrest_frac`·3,29·(bw/20)^0,75 (`circulatory_collapse`) ; PAM < `map_arrest` = 40 mmHg (`irreversible_shock` : PAM maintenue vers 40 mmHg, choc irréversible en 1 à 3 h, Morales 1999, Granot 1985) ; `er ≥ 1`, VO2 > DO2 (`oxygen_debt`) ; racine introuvable (`no_steady_state`) ; pas de convergence (`vicious_circle`).
- **Décompensé** si au moins un mécanisme est actif : les trois couplages, la faiblesse hypokaliémique, la bradycardie hyperkaliémique (K > 6, déjà présente), `er > 0,55` (anaérobiose), POG > 18 (œdème), PAM < 75 (hypoperfusion rénale), tonus sympathique saturé.
- Tous les seuils et pentes sont des paramètres du patient (`healthy_dog.json`), placeholders D sauf `map_arrest` (evidence C). Ils sont inactifs dans la plage du golden et de la calibration.

## 7. Système autonome et VFC (`autonomic.py`)
Simulation à 20 Hz : porte respiratoire (profil `rho`), tonus vagal filtré (`τ_v=0.25 s`), sympathique lent (`τ_s=3 s`, retard 2.5 s, bruit), dérive vagale lente (`vag_noise`, `τ=12 s`), baroréflexe sigmoïde, nœud sinusal à intégration de phase (IPFM) → instants des ondes R → VFC (RR moyen, SDNN, RMSSD, LF/HF, RSA). **Stochastique** : valider statistiquement (`calibration/targets.json`, section `hrv`, tolérance ±1 ET, moyenne sur ≥ 20 graines), pas au point.

## 8. Signaux (`signals.py`)
ECG (dérivation II) : somme de gaussiennes P, Q, R, S, T pilotées par FC et intervalles, construite depuis la conduction. Largeurs calées pour que les durées visibles (seuil 0,02 mV) soient celles des intervalles publiés : P 45 ms (`P_SIGMA`), QRS visible = paramètre `qrs` (Q et S compris), QRS ventriculaire visible = `WIDE_QRS`·`qrs`, segment ST isoélectrique (`T_W0` 25 ms, placeholder D) ; voir `docs/VERIFICATION_ECG.md`. Effets de K+ : T pointue et étroite, P aplatie et élargie (`P_WIDEN_K`, placeholder D), PR et QRS allongés, QT raccourci ; QT corrigé par Fridericia. Ondes f de la FA d'amplitude proportionnelle à l'intervalle depuis l'activation atriale précédente (rapporté à `af_cycle`, plafonné à `F_WAVE_MAX`). PCG : sons S1, S2, S3 et souffle déclenchés par les événements valvulaires simulés, souffle proportionnel au débit régurgitant; grade heuristique (seuils `[10, 25, 50, 80, 120]` sur le pic de débit, souffle présent si pic > 3 mL/s). Validation : propriétés qualitatives (présence/absence de souffle, grade monotone avec la sévérité MMVD, T ample si K > 5.5).

## 8 bis. Périphérie (`periph.rs` ; port secondaire `periph.py`)
- **Thermorégulation** : deux compartiments (noyau `fc = 0,69`, enveloppe), `c = 3470` J/kg/°C ; `M` suit un Q10 de 1,7 et le frisson (× 4 max), `E` les pertes respiratoires (0,20 du basal) et le halètement, `K` la vasomotricité (clip de 0,2 à 3), `G` la perte vers l'ambiance, `warming` l'apport actif. Réponses proportionnelles à l'écart à `t_set`, supprimées par `anesth_mac`. Intégration RK4 au pas de 60 s ; `thermal_course` applique les effets des médicaments à chaque pas.
- **Retour sur l'évaluation** (`apply_temperature`, en tête de `evaluate`) : `hr ×= 1,51^(ΔT/10)`, `vo2 ×= (1 − 0,10·I)·1,7^(ΔT/10) + frisson`, `p50 ×= 10^(0,021·ΔT)` ; identité exacte à 38,6 °C. CAM effective `anesth_mac·exp(−0,086·ΔT)`.
- **Pupille** : équilibre dilatateur (α1, tonus sympathique total) / sphincter (M3, `pupil_ew`, `light`, `pupil_block`), `D = 1,0 + 10,0·d/(d + σ)` ; gains fixés par trois diamètres mesurés (8,53 ambiant, 10,06 obscurité, 6,57 après flash).
- **Digestif** : `motilité = (1 − musc_block)(1 − gi_inhib)(1 − 0,727·min(CAM_eff, 1))` ; `les = 47,9·(1 − 0,724·musc_block)` mmHg ; `salivation = 1 − musc_block`.
- **SNC** : `drive = sedation + (CAM_eff/0,4)^4`, `score = 1 + 20·drive/(1 + drive)` sur l'échelle 0–21 de Grint ; BIS interpolé dans la table de Greene 2003, absent hors de 0,8–2,0 CAM.
- `anesth_mac` n'agit **que** sur la périphérie : les effets hémodynamiques et respiratoires des halogénés ne sont pas modélisés.

## 9. Points sensibles connus
- Solutions non monotones : la crise addisonienne est très sensible au déficit de volume (effondrement de PAM si la volémie stressée devient trop faible).
- RSA nulle si fréquence respiratoire ≥ ~70/min (échantillonnage par battements) : limite connue.
- Placeholders : PD de l'atropine, du bêtabloquant et du pimobendane ; PK de l'atropine et du bêtabloquant ; terme toxique de l'opioïde (borné) ; `f_plasma`, `tau_refill` et `adapt_tau_h` du furosémide ; module pneumonie.
