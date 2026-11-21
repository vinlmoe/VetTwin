//! Rein dynamique : bilans du sodium (volume extracellulaire), du potassium (deux
//! compartiments), activité du système rénine-angiotensine-aldostérone et cinétique de la
//! créatinine et de l'urée, en heures et en jours (docs/PROPOSITION_REIN_DYNAMIQUE.md).
//!
//! Le rein stationnaire de `engine::metabolic_state` (DFG = masse néphronique × perfusion ×
//! axe cardio-rénal) reste la filtration. Ce module y ajoute ce qui était absent : la
//! réabsorption fractionnelle du sodium et du potassium, et donc les bilans. Il ne vit que dans
//! les évolutions temporelles. L'état stationnaire (`evaluate`, CLI `eval`, golden) est son
//! point fixe. Les apports de Na et de K sont ceux de l'alimentation d'un chien sain de même
//! poids, quelle que soit la maladie : en MRC, le néphron restant excrète la même charge, et
//! les FE de base du patient (`FE_b = apport/charge filtrée`) croissent comme 1/ratio_DFG.
//!
//! Équations (temps en h, débits en mmol/h ou mL/h ; `b` : état de départ) :
//!
//! - `FE_Na = (FE_Na,b + fe_loop·E)·A^(−a_na)·(PAM/PAM_b)^n_p` ; `E` : effet du diurétique de
//!   l'anse (pharmacodynamie du module médicament) ; `UNa = DFG·Na·FE_Na` ;
//! - `dV/dt = 1000·(Na_in − UNa)/Na` : variation du volume extracellulaire, iso-osmotique
//!   (la natrémie reste celle du patient, l'eau libre est supposée compensée par la boisson) ;
//!   diurèse `u0 + 1000·(UNa − Na_in)/Na_u` (`Na_u` : natriurie de l'urine excédentaire) ;
//!   le plasma en reçoit la part `Vp/VEC` (→ `vol_shift`, hémo- et protéinoconcentration) ;
//! - `UK = min(DFG·K·fe_k_max, DFG·(K_b·FE_K,b + σ·(K − K_b))·(FE_Na/FE_Na,b)^k_flow·A^a_k)` :
//!   part de base adaptée aux apports, plus une réserve proportionnelle au DFG (σ_up au-dessus
//!   de K_b, σ_down en dessous ; Bourgoignie 1981, Young 1982) ; sécrétion distale portée par
//!   le débit distal et l'aldostérone ;
//! - potassium extracellulaire `Ke` et intracellulaire `Ki` (écarts, mmol) :
//!   `dKe/dt = K_in − UK − J`, `dKi/dt = J`, `J = (B·bw·ΔK − Ki)/τ_ic`, `ΔK = Ke/VEC` ;
//! - RAAS : `dA/dt = (A_t − A)/τ_A`,
//!   `A_t = exp(g_vol·max(0, −V)/VEC)·(1 − IEC)·(K − k0)/(K_b − k0)` (Young 1984) ;
//! - charges orales de K : dépôt digestif absorbé avec `τ_abs` ;
//! - créatinine et urée : `dC/dt = (DFG/V_d)·(C_ss − C)` (comme `kinetics::lagged_marker`).
//!
//! **Domaine validé : l'aigu (une administration, ≤ 24 h).** Calé sur Koh 2021 (2 mg/kg IV),
//! validé sur Koh 2021 per os et Adin 2021 (perfusion de 5 h, créatinine à 24 h). En prises
//! répétées sur plusieurs jours, la déplétion sodée est surestimée (l'adaptation chronique du
//! néphron n'est pas représentée) : pour le chronique, la voie stationnaire du diurétique
//! (`pkpd::timecourse`, `D`/`U`, adaptation φ) reste la référence.
//!
//! La physiologie (`evaluate`) est recalculée à chaque pas `dt_h` avec l'état rénal courant ;
//! entre deux pas, les équations rénales sont intégrées par sous-pas explicites à physiologie
//! figée et concentrations médicamenteuses exactes. Usage pédagogique uniquement.

use crate::engine::{evaluate, Evaluation};
use crate::error::{Error, LoadError};
use crate::modules::{child, parse_json, Ctx, DrugModule, Evidence, PdKind};
use crate::params::Params;
use crate::pkpd::{apply_drug_effects_sites, emax_effect, timecourse, Dose, VB_ML_KG};
use serde_json::{Map, Value as Json};

/// Constantes du rein dynamique (toutes lues dans `modules/renal/renal_model.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct RenalModel {
    /// Version.
    pub version: String,
    /// Fraction excrétée du sodium de base.
    pub fe_na0: f64,
    /// Fraction excrétée du potassium de base.
    pub fe_k0: f64,
    /// Diurèse de base (mL/kg/h).
    pub urine0_ml_kg_h: f64,
    /// Volume extracellulaire (L/kg).
    pub ecf_l_kg: f64,
    /// Capacité tampon intracellulaire du potassium (L/kg apparents).
    pub k_buffer_l_kg: f64,
    /// Constante d'échange extra/intracellulaire du potassium (h).
    pub k_ic_tau_h: f64,
    /// Hausse de la fraction excrétée du sodium à effet maximal du diurétique de l'anse.
    pub fe_loop_max: f64,
    /// Exposant du débit distal (indice `FE_Na/fe_na0`) sur la fraction excrétée du K.
    pub k_flow: f64,
    /// Exposant de la natriurèse de pression.
    pub n_pressure: f64,
    /// Constante de temps de l'activité RAAS (h).
    pub aldo_tau_h: f64,
    /// Gain de la déplétion extracellulaire relative sur l'activité RAAS (`exp(g·ΔV/VEC)`).
    pub aldo_g_vol: f64,
    /// Exposant de l'activité RAAS sur la réabsorption sodée (`A^−a_na`).
    pub aldo_a_na: f64,
    /// Exposant de l'activité RAAS sur la fraction excrétée du K (`A^a_k`).
    pub aldo_a_k: f64,
    /// Pente de l'excrétion de K au-dessus de la kaliémie de base, rapportée au DFG
    /// (`UK += σ_up·DFG·(K − K_b)`, sans dimension) : la réserve d'excrétion suit le DFG.
    pub k_sigma_up: f64,
    /// Même pente sous la kaliémie de base (`σ_down`).
    pub k_sigma_down: f64,
    /// Kaliémie à aldostérone nulle (mmol/L) : `A_t ∝ (K − k0)/(K_b − k0)`.
    pub aldo_k0: f64,
    /// Constante d'absorption d'une charge orale de K (h).
    pub k_abs_tau_h: f64,
    /// Concentration en Na de l'urine excédentaire (mmol/L) : l'excès de diurèse vaut
    /// `1000·(UNa − Na_in)/Na_u` ; l'eau libre perdue est supposée bue.
    pub urine_na_mmol_l: f64,
    /// Fraction excrétée maximale du K (capacité de sécrétion distale).
    pub fe_k_max: f64,
    /// Plancher de la diurèse (fraction de la diurèse de base).
    pub urine_floor_frac: f64,
    /// Niveau de preuve global.
    pub evidence: Evidence,
    /// Références.
    pub refs: Vec<String>,
}

/// Clés de `values`, dans l'ordre des champs.
pub const RENAL_KEYS: [&str; 20] = [
    "fe_na0",
    "fe_k0",
    "urine0_mL_kg_h",
    "ecf_L_kg",
    "k_buffer_L_kg",
    "k_ic_tau_h",
    "fe_loop_max",
    "k_flow",
    "n_pressure",
    "aldo_tau_h",
    "aldo_g_vol",
    "aldo_a_na",
    "aldo_a_k",
    "urine_floor_frac",
    "k_sigma_up",
    "urine_na_mmol_L",
    "fe_k_max",
    "k_sigma_down",
    "aldo_k0",
    "k_abs_tau_h",
];

impl RenalModel {
    /// Charge et valide le module de constantes (`renal_model.schema.json`).
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root = parse_json(json, origin)?;
        let mut c = Ctx::new();
        let Some(m) = c.as_obj(&root, "") else {
            return Err(c.fail(origin));
        };
        c.schema_version(m);
        let version = c.semver(m);
        let evidence = c.evidence(m);
        let refs = c.strings(m, "refs");
        let Some(v) = c.req(m, "", "values").and_then(|v| c.as_obj(v, "/values")) else {
            return Err(c.fail(origin));
        };
        let x: Vec<Option<f64>> = RENAL_KEYS.iter().map(|k| value(&mut c, v, k)).collect();
        if x.iter().any(Option::is_none) {
            return Err(c.fail(origin));
        }
        let g = |i: usize| x[i].expect("contrôlé");
        let out = Self {
            version,
            fe_na0: g(0),
            fe_k0: g(1),
            urine0_ml_kg_h: g(2),
            ecf_l_kg: g(3),
            k_buffer_l_kg: g(4),
            k_ic_tau_h: g(5),
            fe_loop_max: g(6),
            k_flow: g(7),
            n_pressure: g(8),
            aldo_tau_h: g(9),
            aldo_g_vol: g(10),
            aldo_a_na: g(11),
            aldo_a_k: g(12),
            urine_floor_frac: g(13),
            k_sigma_up: g(14),
            urine_na_mmol_l: g(15),
            fe_k_max: g(16),
            k_sigma_down: g(17),
            aldo_k0: g(18),
            k_abs_tau_h: g(19),
            evidence,
            refs,
        };
        for (i, ok) in [
            (0, out.fe_na0 > 0.0 && out.fe_na0 < 1.0),
            (1, out.fe_k0 > 0.0 && out.fe_k0 < 1.0),
            (2, out.urine0_ml_kg_h > 0.0),
            (3, out.ecf_l_kg > 0.0),
            (4, out.k_buffer_l_kg >= 0.0),
            (5, out.k_ic_tau_h > 0.0),
            (6, out.fe_loop_max >= 0.0 && out.fe_loop_max < 1.0),
            (9, out.aldo_tau_h > 0.0),
            (13, (0.0..=1.0).contains(&out.urine_floor_frac)),
            (15, out.urine_na_mmol_l > 0.0),
            (16, out.fe_k_max > 0.0),
            (14, out.k_sigma_up >= 0.0),
            (17, out.k_sigma_down >= 0.0),
            (19, out.k_abs_tau_h > 0.0),
        ] {
            if !ok {
                c.err(child("/values", RENAL_KEYS[i]), "valeur hors domaine");
            }
        }
        c.finish(origin, out)
    }
}

fn value(c: &mut Ctx, v: &Map<String, Json>, key: &str) -> Option<f64> {
    let p = child("/values", key);
    match c.req(v, "/values", key)? {
        Json::Object(o) => {
            if !o.contains_key("evidence") {
                c.err(
                    child(&p, "evidence"),
                    "niveau de preuve requis pour chaque valeur",
                );
            }
            c.req_num(o, &p, "value")
        }
        other => c.as_num(other, &p),
    }
}

/// Réglages d'une évolution rénale.
#[derive(Debug, Clone, PartialEq)]
pub struct RenalOptions {
    /// Horizon (h).
    pub t_end_h: f64,
    /// Pas de recalcul de la physiologie (h).
    pub dt_h: f64,
    /// Inhibition du RAAS (0–1) : `A_t ×= 1 − IEC` (levier pédagogique, pas un médicament).
    pub ace_inhibition: f64,
    /// Apport alimentaire de Na rapporté à l'apport de référence (1 : point fixe ; régime
    /// rénal pauvre en sel < 1).
    pub na_intake_factor: f64,
    /// Apport de K rapporté à l'apport de référence (anorexie < 1, supplémentation > 1).
    pub k_intake_factor: f64,
    /// Charges orales de K : `(instant h, mmol/kg)` (ex. KCl par sonde).
    pub k_loads: Vec<(f64, f64)>,
}

impl Default for RenalOptions {
    fn default() -> Self {
        Self {
            t_end_h: 24.0,
            dt_h: 0.25,
            ace_inhibition: 0.0,
            na_intake_factor: 1.0,
            k_intake_factor: 1.0,
            k_loads: Vec::new(),
        }
    }
}

/// État rénal et physiologique à un instant.
#[derive(Debug, Clone, PartialEq)]
pub struct RenalPoint {
    /// Instant (h).
    pub t_h: f64,
    /// Variation du volume extracellulaire (mL/kg).
    pub d_ecf_ml_kg: f64,
    /// Variation du volume plasmatique (mL/kg).
    pub d_plasma_ml_kg: f64,
    /// Kaliémie (mmol/L).
    pub k: f64,
    /// Variation du potassium intracellulaire (mmol/kg).
    pub d_k_ic_mmol_kg: f64,
    /// Activité RAAS (1 : départ).
    pub aldo: f64,
    /// Créatinine cinétique (µmol/L, unité du moteur).
    pub creat: f64,
    /// Urée cinétique (unité du moteur).
    pub urea: f64,
    /// Diurèse instantanée (mL/kg/h).
    pub urine_ml_kg_h: f64,
    /// Diurèse cumulée depuis 0 (mL/kg).
    pub urine_cum_ml_kg: f64,
    /// Natriurèse instantanée (mmol/kg/h).
    pub una_mmol_kg_h: f64,
    /// Natriurie (mmol/L d'urine).
    pub una_mmol_l: f64,
    /// Kaliurie (mmol/L d'urine).
    pub uk_mmol_l: f64,
    /// Kaliurèse cumulée depuis 0 (mmol/kg).
    pub uk_cum_mmol_kg: f64,
    /// Kaliurèse instantanée (mmol/kg/h).
    pub uk_mmol_kg_h: f64,
    /// Fraction excrétée du sodium.
    pub fe_na: f64,
    /// Fraction excrétée du potassium.
    pub fe_k: f64,
    /// Effet du diurétique de l'anse (0–1).
    pub e_loop: f64,
    /// Évaluation physiologique à cet instant (`None` : décès).
    pub eval: Option<Box<Evaluation>>,
}

/// Résultat d'une évolution rénale.
#[derive(Debug, Clone, PartialEq)]
pub struct RenalCourse {
    /// Points, un par pas de physiologie (0, dt, 2·dt, …, t_end).
    pub points: Vec<RenalPoint>,
    /// Instant du décès (h) et cause.
    pub death: Option<(f64, &'static str)>,
    /// Nombre d'évaluations physiologiques effectuées.
    pub n_evals: usize,
}

/// Écart relatif toléré sur chaque paramètre avant de recalculer la physiologie (0,2 %).
pub const EVAL_TOL: f64 = 2e-3;

/// Écart de kaliémie toléré avant de recalculer la physiologie (mmol/L).
pub const K_TOL: f64 = 0.03;

/// Mêmes entrées physiologiques à `EVAL_TOL` près (`vol_shift` : 0,2 % du volume sanguin ;
/// `k` : `K_TOL`).
fn same_inputs(a: &Params, b: &Params, vb: f64) -> bool {
    a.to_vec()
        .iter()
        .zip(b.to_vec())
        .all(|((name, x), (_, y))| {
            let d = (x - y).abs();
            if *name == "vol_shift" {
                d <= EVAL_TOL * vb
            } else if *name == "k" {
                d <= K_TOL
            } else {
                d <= EVAL_TOL * x.abs().max(y.abs()) + 1e-12
            }
        })
}

/// Effet du diurétique de l'anse : somme des effets Emax des modules `diuretic` (≤ 1), sur
/// `C·masse néphronique` si le module est piloté par l'excrétion urinaire.
fn loop_effect(drugs: &[&DrugModule], conc: &[f64], nephron_mass: f64) -> f64 {
    let mut e = 0.0;
    for (m, c) in drugs.iter().zip(conc) {
        if let PdKind::Diuretic { urinary_drive, .. } = m.pd.kind {
            let x = if urinary_drive { c * nephron_mass } else { *c };
            e += emax_effect(m, x);
        }
    }
    e.min(1.0)
}

/// Évolution du rein dynamique à partir de `base` (patient et maladies, sans médicament).
///
/// Les médicaments de `doses` agissent par leurs effets directs (avec `D = U = 0` : les
/// termes calés du diurétique stationnaire sont neutres) ; le diurétique de l'anse agit en
/// plus par `E` sur la natriurèse. Erreur si `base` n'est pas viable.
pub fn renal_course(
    model: &RenalModel,
    base: &Params,
    drugs: &[DrugModule],
    doses: &[Dose],
    opt: &RenalOptions,
) -> Result<RenalCourse, Error> {
    if !(opt.t_end_h.is_finite() && opt.t_end_h >= 0.0) {
        return Err(Error::InvalidInput(format!(
            "horizon {} h invalide",
            opt.t_end_h
        )));
    }
    if !(opt.dt_h.is_finite() && opt.dt_h > 0.0) {
        return Err(Error::InvalidInput(format!("pas {} h invalide", opt.dt_h)));
    }
    if !(0.0..=1.0).contains(&opt.ace_inhibition) {
        return Err(Error::InvalidInput(format!(
            "inhibition de l'enzyme de conversion {} hors de 0–1",
            opt.ace_inhibition
        )));
    }
    for (name, x) in [
        ("na_intake_factor", opt.na_intake_factor),
        ("k_intake_factor", opt.k_intake_factor),
    ] {
        if !(x.is_finite() && (0.0..=5.0).contains(&x)) {
            return Err(Error::InvalidInput(format!("{name} {x} hors de 0–5")));
        }
    }
    let n_steps = (opt.t_end_h / opt.dt_h).round() as usize;
    if n_steps > 20_000 {
        return Err(Error::InvalidInput(format!(
            "{n_steps} pas : au plus 20 000 (augmenter dt_h)"
        )));
    }
    let ev0 = evaluate(base)?;
    let bw = base.bw;
    let na = base.na;
    let k0 = base.k;
    let gfr_b = ev0.metabolic.chem.gfr * 0.06; // L/h
    let map_b = ev0.hemo.summary.map;
    // Apports alimentaires : ceux d'un chien sain de même poids (DFG de référence `gfr0`,
    // natrémie et kaliémie du patient), indépendants de la maladie. Le néphron restant s'y
    // adapte : FE de base du patient = apport / charge filtrée (×1/ratio_DFG en MRC).
    let gfr_ref = base.gfr0 * bw * 0.06; // L/h
    let na_ref = gfr_ref * na * model.fe_na0; // mmol/h
    let k_ref = gfr_ref * k0 * model.fe_k0;
    let fe_na_b = na_ref / (gfr_b * na);
    let fe_k_b = k_ref / (gfr_b * k0);
    let na_in = na_ref * opt.na_intake_factor;
    let k_in = k_ref * opt.k_intake_factor;
    let ecf_ml = model.ecf_l_kg * bw * 1000.0;
    let vb = VB_ML_KG * base.bvf * bw;
    let vp = vb * (1.0 - 3.0 * base.hb / 100.0);
    let plasma_frac = vp / ecf_ml;
    let u0 = model.urine0_ml_kg_h * bw;

    // PK aux sous-pas (concentrations exactes entre deux évaluations physiologiques)
    let n_sub = ((opt.dt_h / 0.02).ceil() as usize).max(1);
    let h = opt.dt_h / n_sub as f64;
    let grid: Vec<f64> = (0..=n_steps * n_sub).map(|i| i as f64 * h).collect();
    let t_end = n_steps as f64 * opt.dt_h;
    let tc = if doses.is_empty() {
        None
    } else {
        Some(timecourse(base, drugs, doses, t_end, &grid)?)
    };
    let used: Vec<&DrugModule> = tc
        .as_ref()
        .map(|tc| {
            tc.conc
                .iter()
                .map(|(id, _)| drugs.iter().find(|m| &m.id == id).expect("validé"))
                .collect()
        })
        .unwrap_or_default();
    let idx = |ti: f64| -> usize {
        tc.as_ref()
            .map(|tc| tc.t.partition_point(|x| *x < ti - 1e-9).min(tc.t.len() - 1))
            .unwrap_or(0)
    };
    let conc_at = |i: usize| -> Vec<f64> {
        tc.as_ref()
            .map(|tc| tc.conc.iter().map(|(_, v)| v[i]).collect())
            .unwrap_or_default()
    };
    let e_at = |ti: f64| -> f64 { loop_effect(&used, &conc_at(idx(ti)), base.nephron_mass) };

    // états : V (mL), Ke, Ki (mmol), A, créatinine, urée, diurèse cumulée (mL)
    let (mut v, mut ke, mut ki, mut a) = (0.0f64, 0.0f64, 0.0f64, 1.0f64);
    let (mut creat, mut urea) = (ev0.metabolic.chem.creat, ev0.metabolic.chem.urea);
    let mut cum = 0.0f64;
    let mut cum_k = 0.0f64; // kaliurèse cumulée (mmol)
    let mut gut_k = 0.0f64; // K oral non encore absorbé (mmol)
    let mut points = Vec::with_capacity(n_steps + 1);
    let mut death = None;
    let mut last: Option<(Params, Evaluation)> = None;
    let mut n_evals = 0usize;
    // RAAS : déplétion du VEC (rénine) et kaliémie (stimulation directe de la surrénale,
    // aldostérone ∝ K − k0)
    let k_span = (k0 - model.aldo_k0).max(0.1);
    let a_target = |v: f64, k: f64| -> f64 {
        crate::math::exp(model.aldo_g_vol * f64::max(0.0, -v) / ecf_ml)
            * (1.0 - opt.ace_inhibition)
            * ((k - model.aldo_k0) / k_span).max(0.05)
    };
    for step in 0..=n_steps {
        let t = step as f64 * opt.dt_h;
        // paramètres de l'instant : effets directs des médicaments, puis état rénal
        let mut p = match &tc {
            Some(tc) => {
                let i = idx(t);
                let at = |s: &[(String, Vec<f64>)]| -> Vec<(String, f64)> {
                    s.iter().map(|(d, x)| (d.clone(), x[i])).collect()
                };
                apply_drug_effects_sites(base, drugs, &at(&tc.conc), &at(&tc.conc_e), 0.0, 0.0)?
            }
            None => base.clone(),
        };
        let dvp = plasma_frac * v;
        p.vol_shift += dvp;
        p.hb *= vb / (vb + dvp).max(0.2 * vb);
        p.alb *= vp / (vp + dvp).max(0.2 * vp);
        let dk = ke / (ecf_ml / 1000.0);
        p.k = (p.k + dk).max(0.5);
        // physiologie recalculée seulement si un paramètre a bougé au-delà de `EVAL_TOL`
        let reuse = last
            .as_ref()
            .is_some_and(|(q, _): &(Params, Evaluation)| same_inputs(q, &p, vb));
        if !reuse {
            n_evals += 1;
            match evaluate(&p) {
                Ok(ev) => last = Some((p.clone(), ev)),
                Err(Error::NonViable { cause, .. }) => {
                    death = Some((t, cause));
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        let ev = &last.as_ref().expect("évalué").1;
        let gfr = ev.metabolic.chem.gfr * 0.06;
        let map = ev.hemo.summary.map;
        let k_pl = p.k;
        let k_drug_free = p.k - dk; // kaliémie hors écart rénal (maladie, médicaments)
        let p_fac = crate::math::pow((map / map_b).max(0.1), model.n_pressure);
        let fluxes = |e: f64, a: f64, k: f64| {
            let fe_na =
                ((fe_na_b + model.fe_loop_max * e) * crate::math::pow(a, -model.aldo_a_na) * p_fac)
                    .min(0.5);
            let una = gfr * na * fe_na;
            // part de base (adaptée aux apports) + réserve proportionnelle au DFG
            let sigma = if k >= k0 {
                model.k_sigma_up
            } else {
                model.k_sigma_down
            };
            let drive = (k0 * fe_k_b + sigma * (k - k0)).max(0.0);
            let uk = (gfr
                * drive
                * crate::math::pow(fe_na / fe_na_b, model.k_flow)
                * crate::math::pow(a, model.aldo_a_k))
            .min(gfr * k * model.fe_k_max);
            let fe_k = uk / (gfr * k).max(1e-12);
            let urine = (u0 + 1000.0 * (una - na_ref) / model.urine_na_mmol_l)
                .max(model.urine_floor_frac * u0);
            (fe_na, una, fe_k, uk, urine)
        };
        let e_now = e_at(t);
        let (fe_na, una, fe_k, uk, urine) = fluxes(e_now, a, k_pl);
        points.push(RenalPoint {
            t_h: t,
            d_ecf_ml_kg: v / bw,
            d_plasma_ml_kg: dvp / bw,
            k: k_pl,
            d_k_ic_mmol_kg: ki / bw,
            aldo: a,
            creat,
            urea,
            urine_ml_kg_h: urine / bw,
            urine_cum_ml_kg: cum / bw,
            una_mmol_kg_h: una / bw,
            una_mmol_l: 1000.0 * una / urine,
            uk_mmol_l: 1000.0 * uk / urine,
            uk_mmol_kg_h: uk / bw,
            uk_cum_mmol_kg: cum_k / bw,
            fe_na,
            fe_k,
            e_loop: e_now,
            eval: Some(Box::new(ev.clone())),
        });
        if step == n_steps {
            break;
        }
        // sous-pas explicites, physiologie figée
        let (c_ss, u_ss) = (ev.metabolic.chem.creat, ev.metabolic.chem.urea);
        let kc = gfr / (base.creat_vd * bw);
        let ku = gfr / (base.urea_vd * bw);
        for j in 0..n_sub {
            let ts = t + j as f64 * h;
            let e = e_at(ts);
            let dk = ke / (ecf_ml / 1000.0);
            let k_now = (k_drug_free + dk).max(0.5);
            let (_, una, _, uk, urine) = fluxes(e, a, k_now);
            // charges orales de K : dépôt digestif, absorption d'ordre 1
            for &(tl, mmol_kg) in &opt.k_loads {
                if tl >= ts - 1e-9 && tl < ts + h - 1e-9 {
                    gut_k += mmol_kg * bw;
                }
            }
            let absorb = gut_k / model.k_abs_tau_h;
            gut_k -= h * absorb;
            let jflux = (model.k_buffer_l_kg * bw * dk - ki) / model.k_ic_tau_h;
            v += h * 1000.0 * (na_in - una) / na;
            ke += h * (k_in + absorb - uk - jflux);
            ki += h * jflux;
            a += h * (a_target(v, k_now) - a) / model.aldo_tau_h;
            cum += h * urine;
            cum_k += h * uk;
        }
        creat = c_ss + (creat - c_ss) * crate::math::exp(-kc * opt.dt_h);
        urea = u_ss + (urea - u_ss) * crate::math::exp(-ku * opt.dt_h);
    }
    Ok(RenalCourse {
        points,
        death,
        n_evals,
    })
}
