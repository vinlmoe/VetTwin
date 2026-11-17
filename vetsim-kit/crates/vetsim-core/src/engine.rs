//! Orchestration physiologique (miroir de `physio.evaluate`).
//!
//! M2 : partie **algébrique** en aval de l'hémodynamique — perfusion et transport d'O2,
//! lactate, gaz artériel et veineux mêlé, rein, biochimie ([`metabolic_state`]).
//! M3 y branchera la boucle baroréflexe et la simulation cardiovasculaire.

use crate::error::Error;
use crate::gas::{gas_exchange, po2_from_content, stewart_ph, ArterialGas};
use crate::params::Params;

/// Grandeurs hémodynamiques consommées par l'aval (issues du dernier battement simulé).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HemoInputs {
    /// Débit cardiaque (L/min).
    pub co: f64,
    /// Pression artérielle moyenne (mmHg).
    pub map: f64,
    /// Pression moyenne de l'OG / veine pulmonaire (mmHg).
    pub la_mean: f64,
}

/// Sang veineux mêlé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VenousGas {
    /// pH veineux.
    pub ph: f64,
    /// PvCO2 (mmHg).
    pub pco2: f64,
    /// HCO3 veineux (mmol/L).
    pub hco3: f64,
    /// PvO2 (mmHg).
    pub po2: f64,
}

/// Biochimie (unités de sortie du CLI).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chemistry {
    /// Na (mmol/L).
    pub na: f64,
    /// K (mmol/L).
    pub k: f64,
    /// Cl (mmol/L).
    pub cl: f64,
    /// Lactate (mmol/L).
    pub lactate: f64,
    /// Glucose (mmol/L).
    pub glucose: f64,
    /// Créatinine (µmol/L).
    pub creat: f64,
    /// Urée (mmol/L).
    pub urea: f64,
    /// Phosphate (mmol/L).
    pub phos: f64,
    /// Albumine (g/L).
    pub alb: f64,
    /// ALAT (U/L).
    pub alt: f64,
    /// Hématocrite (%).
    pub hct: f64,
    /// DFG (mL/min).
    pub gfr: f64,
    /// Trou anionique Na + K − Cl − HCO3 (mmol/L).
    pub anion_gap: f64,
}

/// État métabolique en aval de l'hémodynamique.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetabolicState {
    /// Gaz artériel (calculé avec le lactate final).
    pub art: ArterialGas,
    /// Lactate artériel (mmol/L).
    pub lactate: f64,
    /// Sang veineux mêlé.
    pub ven: VenousGas,
    /// Biochimie.
    pub chem: Chemistry,
    /// Transport d'O2 (mL/min), avec le CaO2 final.
    pub do2: f64,
    /// Transport d'O2 par kg (mL/min/kg).
    pub do2_kg: f64,
    /// Coefficient d'extraction d'O2 VO2/DO2 (premier passage, celui qui fixe le lactate).
    pub er: f64,
}

impl MetabolicState {
    /// Clés plates du contrat CLI (`eval`) produites par l'aval, dans l'ordre du contrat.
    pub fn to_flat(&self) -> Vec<(&'static str, f64)> {
        let (a, v, c) = (&self.art, &self.ven, &self.chem);
        vec![
            ("ph", a.ph),
            ("pco2", a.pco2),
            ("hco3", a.hco3),
            ("be", a.be),
            ("pao2", a.pao2),
            ("sao2", a.sao2),
            ("p50", a.p50),
            ("rr", a.rr),
            ("lactate", self.lactate),
            ("ph_v", v.ph),
            ("pco2_v", v.pco2),
            ("po2_v", v.po2),
            ("hco3_v", v.hco3),
            ("na", c.na),
            ("k", c.k),
            ("cl", c.cl),
            ("glucose", c.glucose),
            ("creat", c.creat),
            ("urea", c.urea),
            ("phos", c.phos),
            ("alb", c.alb),
            ("hct", c.hct),
            ("gfr", c.gfr),
            ("anion_gap", c.anion_gap),
            ("do2_kg", self.do2_kg),
            ("er", self.er),
        ]
    }
}

/// Perfusion, lactate, gaz du sang, rein et biochimie pour des paramètres `p` et une
/// hémodynamique `h` donnés (MODEL_SPEC §4–5). L'ordre des calculs suit la référence :
/// 1. gaz avec le lactate de base → DO2, extraction `er`, lactate
///    `lact0 + 9/(1 + exp(−(er − 0,55)/0,06))` (hyperlactatémie quand l'extraction dépasse ~55 %) ;
/// 2. gaz recalculés avec ce lactate → DO2 final ;
/// 3. sang veineux mêlé : `CvO2 = CaO2 − VO2/(DC·10)`, PvO2 par inversion de la courbe,
///    `PvCO2 = PaCO2 + 5,5·(0,125·bw)/max(DC, 0,2)`, pH veineux par Stewart ;
/// 4. rein : perfusion bornée par la PAM et le DC relatif, plateau à 0,25 (azotémie prérénale ;
///    l'IRA n'est pas modélisée), `DFG/DFG0 = max(0,03, masse néphronique × perfusion)`.
pub fn metabolic_state(p: &Params, h: &HemoInputs) -> Result<MetabolicState, Error> {
    for (name, v) in [("co", h.co), ("map", h.map), ("la_mean", h.la_mean)] {
        if !v.is_finite() {
            return Err(Error::InvalidInput(format!("{name} non fini")));
        }
    }
    if h.co <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "débit cardiaque {} L/min ≤ 0",
            h.co
        )));
    }
    let bw = p.bw;
    // --- perfusion -> DO2, lactate
    let g0 = gas_exchange(p, h.la_mean, p.lact0)?;
    let do2_0 = h.co * 1000.0 * g0.cao2 / 100.0; // mL/min
    let vo2 = p.vo2 * bw;
    let er = vo2 / do2_0;
    let lact = p.lact0 + 9.0 / (1.0 + (-(er - 0.55) / 0.06).exp());
    let g = gas_exchange(p, h.la_mean, lact)?;
    let do2 = h.co * 1000.0 * g.cao2 / 100.0;
    // --- sang veineux mêlé
    let cv = g.cao2 - vo2 / (h.co * 10.0); // mL/dL
    let pvo2 = po2_from_content(cv, p.hb, g.p50, p.hill_n)?;
    let dpco2 = 5.5 * (125.0 * bw / 1000.0) / h.co.max(0.2);
    let pvco2 = g.pco2 + dpco2;
    let (phv, hco3v) = stewart_ph(p, pvco2, lact)?;
    // --- rein
    let co_ref = 3.29 * (bw / 20.0).powf(0.75);
    let by_map = ((h.map - 40.0) / 35.0).clamp(0.0, 1.0);
    let by_co = ((h.co / co_ref - 0.5) / 0.5).clamp(0.0, 1.0);
    let perf = by_map.min(by_co).max(0.25);
    let gfr_ratio = (p.nephron_mass * perf).max(0.03);
    let chem = Chemistry {
        na: p.na,
        k: p.k,
        cl: p.cl,
        lactate: lact,
        glucose: p.glucose,
        creat: p.creat0 / gfr_ratio,
        urea: p.urea0 / gfr_ratio.powf(0.8),
        phos: p.phos,
        alb: p.alb * 10.0,
        alt: p.alt,
        hct: p.hb * 3.0,
        gfr: p.gfr0 * bw * gfr_ratio,
        anion_gap: p.na + p.k - p.cl - g.hco3,
    };
    Ok(MetabolicState {
        art: g,
        lactate: lact,
        ven: VenousGas {
            ph: phv,
            pco2: pvco2,
            hco3: hco3v,
            po2: pvo2,
        },
        chem,
        do2,
        do2_kg: do2 / bw,
        er,
    })
}

/// Tonus de référence (chien sain au repos) qui définit `hr` comme la FC de repos.
pub const S_REF: f64 = 0.2;
/// Tonus vagal de référence.
pub const V_REF: f64 = 0.6;
/// Nombre maximal d'itérations du baroréflexe.
pub const BARO_MAX_ITER: usize = 5;
/// Seuil d'arrêt sur la commande barorécepteur.
pub const BARO_TOL: f64 = 0.04;

/// Effet multiplicatif sympathique (accélérateur) et vagal (frein) sur la FC du nœud sinusal.
pub fn hr_factor(s: f64, v: f64, p: &Params) -> f64 {
    (1.0 + p.a_s * s) * (1.0 - p.a_v * v)
}

/// État autonome en régime permanent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Autonomic {
    /// Tonus sympathique total (0–1).
    pub s: f64,
    /// Tonus vagal total (0–1).
    pub v: f64,
    /// Tonus sympathique effectif (après bêtabloquant).
    pub s_eff: f64,
    /// Tonus vagal effectif (après atropine).
    pub v_eff: f64,
    /// Commande barorécepteur (> 0 : hypotension).
    pub d: f64,
    /// PAM du dernier passage (mmHg).
    pub map: f64,
}

/// Évaluation complète d'un patient (miroir de `physio.evaluate`).
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// Paramètres effectifs après baroréflexe (`hr`, `ees`, `r_sys`, `vol_shift`, `sympathetic`).
    pub params: Params,
    /// Hémodynamique du dernier passage.
    pub hemo: crate::hemo::HemoResult,
    /// Aval métabolique.
    pub metabolic: MetabolicState,
    /// Tonus autonomes.
    pub autonomic: Autonomic,
    /// Nombre de passages du baroréflexe effectués (1–5).
    pub baro_iterations: usize,
}

impl Evaluation {
    /// Sortie plate du contrat CLI `eval`, dans l'ordre de docs/CLI_CONTRACT.md.
    pub fn to_flat(&self) -> Vec<(&'static str, f64)> {
        let h = &self.hemo.summary;
        let mut out = vec![
            ("hr", self.params.hr),
            ("map", h.map),
            ("sap", h.sap),
            ("dap", h.dap),
            ("co", h.co),
            ("sv", h.sv),
            ("edv", h.edv),
            ("esv", h.esv),
            ("ef", h.ef),
            ("reg_fraction", h.reg_fraction),
            ("la_mean", h.la_mean),
            ("pap_mean", h.pap_mean),
            ("cvp", h.cvp),
            ("lv_edp", h.lv_edp),
        ];
        out.extend(self.metabolic.to_flat());
        out.push(("s_tone", self.autonomic.s));
        out.push(("v_tone", self.autonomic.v));
        out
    }
}

/// Évalue le patient : baroréflexe en régime permanent puis aval métabolique (MODEL_SPEC §3).
///
/// À chaque passage, la commande `d` (> 0 en hypotension) augmente le tonus sympathique et
/// réduit le tonus vagal ; ils modulent la FC (avec la dépression par l'hyperkaliémie au-delà de
/// 6 mmol/L), l'inotropisme, la résistance systémique et le recrutement du volume non contraint.
/// Arrêt si `|d_t − d| < 0,04`, sinon `d ← ½(d + d_t)`, au plus 5 passages.
pub fn evaluate(base: &Params) -> Result<Evaluation, Error> {
    use crate::hemo::{simulate_hemo, N_BEATS};
    let mut d = 0.0;
    let mut last = None;
    let mut iterations = 0;
    for _ in 0..BARO_MAX_ITER {
        iterations += 1;
        let s_tot = (base.s0 + 0.8 * d).clamp(0.0, 1.0);
        let v_tot = (base.v0 + base.vagal_add - 0.6 * d).clamp(0.0, 1.0);
        let s_eff = s_tot * (1.0 - base.beta_block);
        let v_eff = v_tot * (1.0 - base.vagal_block);
        let kfac = f64::max(0.4, 1.0 - 0.30 * f64::max(0.0, base.k - 6.0));
        let mut q = base.clone();
        q.hr = base.hr * hr_factor(s_eff, v_eff, base) / hr_factor(S_REF, V_REF, base) * kfac;
        q.ees_lv = base.ees_lv * (1.0 + 0.4 * (s_eff - base.s0));
        q.ees_rv = base.ees_rv * (1.0 + 0.4 * (s_eff - base.s0));
        q.r_sys = base.r_sys * (1.0 + 0.5 * (s_tot - base.s0));
        q.vol_shift = base.vol_shift + 0.20 * 85.0 * base.bw * (s_tot - base.s0);
        let h = simulate_hemo(&q, N_BEATS)?;
        let d_t = (base.baro_gain * (base.map_set - h.summary.map) / 45.0).clamp(-1.0, 1.0);
        let done = (d_t - d).abs() < BARO_TOL;
        last = Some((q, h, s_tot, v_tot, s_eff, v_eff));
        if done {
            break;
        }
        d = 0.5 * d + 0.5 * d_t;
    }
    let (mut q, h, s_tot, v_tot, s_eff, v_eff) = last.expect("au moins un passage");
    q.sympathetic = s_tot;
    let hs = &h.summary;
    let metabolic = metabolic_state(
        &q,
        &HemoInputs {
            co: hs.co,
            map: hs.map,
            la_mean: hs.la_mean,
        },
    )?;
    let autonomic = Autonomic {
        s: s_tot,
        v: v_tot,
        s_eff,
        v_eff,
        d,
        map: hs.map,
    };
    Ok(Evaluation {
        params: q,
        hemo: h,
        metabolic,
        autonomic,
        baro_iterations: iterations,
    })
}
