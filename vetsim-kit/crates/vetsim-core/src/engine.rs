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
