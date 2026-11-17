//! PK/PD (miroir de `reference/python/pkpd.py`, MODEL_SPEC §6).
//!
//! PK : 2 compartiments + dépôt oral, clairance allométrique (BW^0,75) modulée par la masse
//! néphronique et la fonction hépatique. PD : effet Emax normalisé `E`, appliqué aux paramètres
//! par les effets JSON du module ; pour un diurétique, la diurèse crée un déficit de volume `D`
//! et des pertes électrolytiques `U`, qui se récupèrent lentement.
//!
//! ATTENTION : les valeurs PD de tous les médicaments et la PK de l'atropine, du bêtabloquant et
//! de l'opioïde sont des placeholders pédagogiques (docs/PROVENANCE.md). Ne jamais utiliser pour
//! décider d'une dose réelle.

use crate::case::{apply_modifiers, evaluate_effects};
use crate::dsl::Env;
use crate::error::Error;
use crate::math;
use crate::modules::{DrugModule, PdKind};
use crate::ode::{solve, Options};
use crate::params::Params;

/// Voie d'administration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    /// Bolus intraveineux (compartiment central).
    Iv,
    /// Voie orale (dépôt, biodisponibilité `f`, absorption `ka`).
    Po,
}

impl Route {
    /// Nom (`iv`, `po`).
    pub fn as_str(self) -> &'static str {
        match self {
            Route::Iv => "iv",
            Route::Po => "po",
        }
    }
}

/// Administration d'une dose.
#[derive(Debug, Clone, PartialEq)]
pub struct Dose {
    /// Instant (h).
    pub t_h: f64,
    /// Identifiant du médicament.
    pub drug: String,
    /// Dose (mg/kg).
    pub mg_per_kg: f64,
    /// Voie.
    pub route: Route,
}

/// Constantes PK du patient : clairance (L/h), volumes central et périphérique (L),
/// clairance intercompartimentale (L/h).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PkConstants {
    /// Clairance (L/h).
    pub cl: f64,
    /// Volume central (L).
    pub vc: f64,
    /// Volume périphérique (L).
    pub vp: f64,
    /// Clairance intercompartimentale (L/h).
    pub q: f64,
}

/// Constantes PK pour le patient `p` : `CL = cl·1e-3·60·20·(bw/20)^0,75·néphrons^renal_exp·foie^hep_exp`.
pub fn pk_constants(drug: &DrugModule, p: &Params) -> PkConstants {
    let k = &drug.pk;
    let bw = p.bw;
    let f = math::pow(bw / 20.0, 0.75);
    let cl_scale = math::pow(p.nephron_mass, k.renal_exp) * math::pow(p.hepatic_fn, k.hep_exp);
    PkConstants {
        cl: k.cl_ml_min_kg * 1e-3 * 60.0 * 20.0 * f * cl_scale,
        vc: k.vc_l_kg * bw,
        vp: k.vp_l_kg * bw,
        q: k.q_ml_min_kg * 1e-3 * bw * 60.0,
    }
}

/// Effet Emax normalisé : `E = emax·C^h/(EC50^h + C^h)`, concentration négative ramenée à 0.
pub fn emax_effect(drug: &DrugModule, conc_mg_l: f64) -> f64 {
    let pd = &drug.pd;
    let c = math::pow(conc_mg_l.max(0.0), pd.hill);
    pd.emax * c / (math::pow(pd.ec50_mg_l, pd.hill) + c)
}

/// Effet toxique normalisé : `T = emax·C^h/(EC50^h + C^h)` (même forme, constantes propres).
pub fn toxic_effect(t: &crate::modules::Toxicity, conc_mg_l: f64) -> f64 {
    let c = math::pow(conc_mg_l.max(0.0), t.hill);
    t.emax * c / (math::pow(t.ec50_mg_l, t.hill) + c)
}

/// Paramètres modifiés par les concentrations plasmatiques `conc` (mg/L) et par l'état de
/// volume du diurétique (`d` : déficit, mL ; `u` : diurèse effective, mL/kg).
///
/// Chaque médicament présent dans `conc` applique ses effets avec `E` = effet Emax. Les effets
/// d'un module diurétique utilisent aussi `D` et `U` ; ils sont appliqués dès que le médicament
/// est dans `conc` **ou** que `D` ou `U` est non nul (comme `pkpd.apply_drug_effects`).
/// Puis, si le médicament a une concentration, chacun de ses termes toxiques avec `T`.
/// Ordre d'application : celui de `drugs` (le catalogue est trié par identifiant). Les modules
/// fournis ne se recouvrent que par des effets multiplicatifs (`ees_lv` : pimobendane et
/// toxicité du bêtabloquant) : l'ordre ne change le résultat qu'à l'arrondi près.
pub fn apply_drug_effects(
    p: &Params,
    drugs: &[DrugModule],
    conc: &[(String, f64)],
    d: f64,
    u: f64,
    uk: f64,
) -> Result<Params, Error> {
    for (id, c) in conc {
        if !drugs.iter().any(|m| &m.id == id) {
            return Err(Error::UnknownDrug(id.clone()));
        }
        if !c.is_finite() {
            return Err(Error::InvalidInput(format!(
                "concentration de {id} non finie"
            )));
        }
    }
    if !(d.is_finite() && u.is_finite() && uk.is_finite()) {
        return Err(Error::InvalidInput("D, U ou UK non fini".into()));
    }
    let volume_state = d != 0.0 || u != 0.0 || uk != 0.0;
    if volume_state
        && !drugs
            .iter()
            .any(|m| matches!(m.pd.kind, PdKind::Diuretic { .. }))
    {
        return Err(Error::InvalidInput(
            "D/U/UK fournis mais aucun module diurétique chargé".into(),
        ));
    }
    let mut q = p.clone();
    for m in drugs {
        // Dernière valeur si le médicament est cité plusieurs fois (comme un dict Python).
        let c = conc
            .iter()
            .rev()
            .find(|(id, _)| id == &m.id)
            .map(|(_, c)| *c);
        let diuretic = matches!(m.pd.kind, PdKind::Diuretic { .. });
        if c.is_none() && !(diuretic && volume_state) {
            continue;
        }
        let mut env = Env::new();
        env.set_num("E", c.map_or(0.0, |c| emax_effect(m, c)));
        if diuretic {
            env.set_num("D", d);
            env.set_num("U", u);
            env.set_num("UK", uk);
        }
        let mods = evaluate_effects(&m.pd.effects, &env, &m.id)?;
        let relabel = |e: Error| match e {
            Error::Eval {
                location, source, ..
            } => Error::Eval {
                module: m.id.clone(),
                location,
                source,
            },
            other => other,
        };
        apply_modifiers(&mut q, &mods).map_err(relabel)?;
        if let Some(c) = c {
            for t in &m.pd.toxicity {
                let mut env = Env::new();
                env.set_num("T", toxic_effect(t, c));
                let mods = evaluate_effects(&t.effects, &env, &m.id)?;
                apply_modifiers(&mut q, &mods).map_err(relabel)?;
            }
        }
    }
    Ok(q)
}

/// Évolution temporelle : concentrations plasmatiques et état de volume aux instants demandés.
#[derive(Debug, Clone, PartialEq)]
pub struct Timecourse {
    /// Instants enregistrés (h), triés, sans doublon.
    pub t: Vec<f64>,
    /// Concentration plasmatique centrale (mg/L) par médicament (ordre alphabétique).
    pub conc: Vec<(String, Vec<f64>)>,
    /// Déficit de volume (mL).
    pub d: Vec<f64>,
    /// Diurèse effective (mL/kg), récupération `tau_elect_h` (pertes de Na⁺ et Cl⁻).
    pub u: Vec<f64>,
    /// Diurèse effective (mL/kg), récupération `tau_k_h` (pertes de K⁺).
    pub uk: Vec<f64>,
}

impl Timecourse {
    /// Série de concentration d'un médicament.
    pub fn conc_of(&self, drug: &str) -> Option<&[f64]> {
        self.conc
            .iter()
            .find(|(d, _)| d == drug)
            .map(|(_, v)| v.as_slice())
    }
}

/// Tolérances de l'intégration PK (celles de la référence).
pub const PK_RTOL: f64 = 1e-8;
/// Tolérance absolue PK.
pub const PK_ATOL: f64 = 1e-10;

/// Simule les doses `doses` jusqu'à `t_end_h` et enregistre l'état aux instants `t_eval_h`.
///
/// États par médicament : dépôt digestif, central, périphérique (mg), concentration au site
/// d'effet (mg/L, `dCe/dt = ke0·(C − Ce)`, figée sans `ke0`) ; puis `D`, `U` et `UK`.
/// L'intégration est relancée à chaque instant remarquable (doses, enregistrements),
/// comme la référence. Diurétique : `u = urine_max·E(Ce)` (mL/kg/min ; `E(C)` sans `ke0`),
/// `dD/dt = f_plasma·u·bw·60 − D/τ_refill`, `dU/dt = 60·u − U/τ_elect`,
/// `dUK/dt = 60·u − UK/τ_k` (par heure ; `τ_k = τ_elect` si absent).
pub fn timecourse(
    p: &Params,
    catalog_drugs: &[DrugModule],
    doses: &[Dose],
    t_end_h: f64,
    t_eval_h: &[f64],
) -> Result<Timecourse, Error> {
    for x in doses.iter().map(|d| d.t_h).chain(t_eval_h.iter().copied()) {
        if !(x.is_finite() && x >= 0.0) {
            return Err(Error::InvalidInput(format!("instant {x} h invalide (≥ 0)")));
        }
    }
    if !(t_end_h.is_finite() && t_end_h >= 0.0) {
        return Err(Error::InvalidInput(format!("horizon {t_end_h} h invalide")));
    }
    for d in doses {
        if !(d.mg_per_kg.is_finite() && d.mg_per_kg >= 0.0) {
            return Err(Error::InvalidInput(format!(
                "dose de {} invalide : {} mg/kg",
                d.drug, d.mg_per_kg
            )));
        }
    }
    let bw = p.bw;
    let mut ids: Vec<&str> = doses.iter().map(|d| d.drug.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mods: Vec<&DrugModule> = ids
        .iter()
        .map(|id| {
            catalog_drugs
                .iter()
                .find(|m| m.id == *id)
                .ok_or_else(|| Error::UnknownDrug((*id).to_string()))
        })
        .collect::<Result<_, _>>()?;
    let pars: Vec<PkConstants> = mods.iter().map(|m| pk_constants(m, p)).collect();
    let n = 4 * mods.len() + 3;
    let mut state = vec![0.0; n];
    let mut events: Vec<&Dose> = doses.iter().collect();
    events.sort_by(|a, b| {
        (a.t_h, &a.drug, a.mg_per_kg, a.route)
            .partial_cmp(&(b.t_h, &b.drug, b.mg_per_kg, b.route))
            .expect("valeurs finies")
    });
    let mut times: Vec<f64> = vec![0.0, t_end_h];
    times.extend(events.iter().map(|e| e.t_h));
    times.extend_from_slice(t_eval_h);
    times.sort_by(|a, b| a.partial_cmp(b).expect("valeurs finies"));
    times.dedup();

    let rhs = |_t: f64, y: &Vec<f64>| {
        let mut dy = vec![0.0; n];
        let mut urine = 0.0; // mL/kg/min
        for (j, (m, k)) in mods.iter().zip(&pars).enumerate() {
            let i = 4 * j;
            let (gut, ac, ap, ce) = (y[i], y[i + 1], y[i + 2], y[i + 3]);
            let ka = m.pk.ka_per_h;
            dy[i] = -ka * gut;
            dy[i + 1] = ka * gut - (k.cl / k.vc) * ac - (k.q / k.vc) * ac + (k.q / k.vp) * ap;
            dy[i + 2] = (k.q / k.vc) * ac - (k.q / k.vp) * ap;
            if let PdKind::Diuretic {
                urine_max_ml_kg_min,
                ke0_per_h,
                ..
            } = m.pd.kind
            {
                // site d'effet : action depuis la lumière tubulaire (retard sur le plasma)
                let c_eff = match ke0_per_h {
                    Some(ke0) => {
                        dy[i + 3] = ke0 * (ac / k.vc - ce);
                        ce
                    }
                    None => ac / k.vc,
                };
                urine += urine_max_ml_kg_min * emax_effect(m, c_eff);
            }
        }
        // Récupération : constantes du (premier) diurétique dosé ; sans diurétique, D, U et UK
        // restent nuls.
        if let Some(PdKind::Diuretic {
            f_plasma,
            tau_refill_h,
            tau_elect_h,
            tau_k_h,
            ..
        }) = mods
            .iter()
            .map(|m| &m.pd.kind)
            .find(|k| matches!(k, PdKind::Diuretic { .. }))
        {
            dy[n - 3] = f_plasma * urine * bw * 60.0 - y[n - 3] / tau_refill_h;
            dy[n - 2] = urine * 60.0 - y[n - 2] / tau_elect_h;
            dy[n - 1] = urine * 60.0 - y[n - 1] / tau_k_h.unwrap_or(*tau_elect_h);
        }
        dy
    };
    let opts = Options {
        rtol: PK_RTOL,
        atol: PK_ATOL,
        ..Options::default()
    };
    let mut out = Timecourse {
        t: Vec::new(),
        conc: ids.iter().map(|id| (id.to_string(), Vec::new())).collect(),
        d: Vec::new(),
        u: Vec::new(),
        uk: Vec::new(),
    };
    let mut ev_i = 0;
    for (j, &a) in times.iter().enumerate() {
        while ev_i < events.len() && (events[ev_i].t_h - a).abs() < 1e-9 {
            let e = events[ev_i];
            let k = ids.iter().position(|id| *id == e.drug).expect("indexé");
            let mg = e.mg_per_kg * bw;
            match e.route {
                Route::Iv => state[4 * k + 1] += mg,
                Route::Po => state[4 * k] += mg * mods[k].pk.f,
            }
            ev_i += 1;
        }
        if t_eval_h.contains(&a) {
            out.t.push(a);
            out.d.push(state[n - 3]);
            out.u.push(state[n - 2]);
            out.uk.push(state[n - 1]);
            for (k, (_, series)) in out.conc.iter_mut().enumerate() {
                series.push(state[4 * k + 1] / pars[k].vc);
            }
        }
        if let Some(&b) = times.get(j + 1) {
            state = solve(&rhs, a, state, b, &opts, |_| {}).map_err(Error::Ode)?;
        }
    }
    Ok(out)
}
