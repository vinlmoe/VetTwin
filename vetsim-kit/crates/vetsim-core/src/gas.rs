//! Gaz du sang et acide-base (miroir de `physio.py` : `hb_o2_sat`, `o2_content`,
//! `po2_from_content`, `stewart_ph`, `base_excess`, `gas_exchange`). MODEL_SPEC §5.

use crate::error::Error;
use crate::math;
use crate::params::Params;
use crate::roots::{brentq, RootError};

fn root_err(what: &'static str, lo: f64, hi: f64) -> impl Fn(RootError) -> Error {
    move |e| Error::Root {
        what,
        lo,
        hi,
        cause: e,
    }
}

/// Saturation de l'hémoglobine (0–1), équation de Hill `S = P^n / (P^n + P50^n)`.
/// PO2 est plancher à 1e-3 mmHg comme dans la référence.
pub fn hb_o2_sat(po2: f64, p50: f64, n: f64) -> f64 {
    let po2 = po2.max(1e-3);
    math::pow(po2, n) / (math::pow(po2, n) + math::pow(p50, n))
}

/// Contenu en O2 (mL O2/dL) : O2 lié `1,34·Hb·S` + O2 dissous `0,003·PO2`.
pub fn o2_content(po2: f64, hb: f64, p50: f64, n: f64) -> f64 {
    1.34 * hb * hb_o2_sat(po2, p50, n) + 0.003 * po2
}

/// PO2 (mmHg) correspondant au contenu `c`, par inversion de la courbe sur [1, 600] mmHg.
///
/// Comme la référence : hors de l'intervalle, renvoie 1 si `c < 0`, sinon 600. Conséquence
/// (comportement de la référence conservé tel quel) : un contenu positif mais inférieur au
/// contenu à 1 mmHg donne 600 et non 1.
pub fn po2_from_content(c: f64, hb: f64, p50: f64, n: f64) -> Result<f64, Error> {
    let f = |x: f64| o2_content(x, hb, p50, n) - c;
    if f(1.0) < 0.0 && 0.0 < f(600.0) {
        brentq(f, 1.0, 600.0).map_err(root_err("po2_from_content", 1.0, 600.0))
    } else if c < 0.0 {
        Ok(1.0)
    } else {
        Ok(600.0)
    }
}

/// Bicarbonate (mmol/L) par Henderson-Hasselbalch : `0,0307·PCO2·10^(pH − 6,1)`.
pub fn hco3(pco2: f64, ph: f64) -> f64 {
    0.0307 * pco2 * math::pow10(ph - 6.1)
}

/// pH par l'équilibre de charges de Stewart, et HCO3 correspondant.
///
/// `SID = Na + K − Cl − lactate − XA` ; on résout `SID + [H+] − HCO3 − A_alb − A_phos = 0`
/// sur pH ∈ [5,5 ; 10,5] avec :
/// - `A_alb = alb(g/L)·(0,28 + 0,469/(1 + 10^(7,77 − pH)))` (albumine canine,
///   Constable & Stämpfli 2005 : Atot 0,469 mmol/g, pKa 7,77) ;
/// - `A_phos = phos·(0,309·pH − 0,469)` (coefficient humain, faute d'équivalent canin publié).
pub fn stewart_ph(p: &Params, pco2: f64, lactate: f64) -> Result<(f64, f64), Error> {
    let sid = p.na + p.k - p.cl - lactate - p.xa;
    let alb_gl = p.alb * 10.0;
    let f = |ph: f64| {
        let h = 1e3 * math::pow10(-ph);
        let a_alb = alb_gl * (0.28 + 0.469 / (1.0 + math::pow10(7.77 - ph)));
        let a_pi = p.phos * (0.309 * ph - 0.469);
        sid + h - hco3(pco2, ph) - a_alb - a_pi
    };
    let ph = brentq(f, 5.5, 10.5).map_err(root_err("stewart_ph", 5.5, 10.5))?;
    Ok((ph, hco3(pco2, ph)))
}

/// Excès de base (mmol/L), van Slyke simplifié avec Hb en mmol/L (`hb·0,6206`).
pub fn base_excess(ph: f64, hco3: f64, hb: f64) -> f64 {
    let hbm = hb * 0.6206;
    (1.0 - 0.014 * hbm) * (hco3 - 24.4 + (2.3 * hbm + 7.7) * (ph - 7.4))
}

/// Gaz artériel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArterialGas {
    /// PaCO2 (mmHg).
    pub pco2: f64,
    /// pH artériel.
    pub ph: f64,
    /// HCO3 (mmol/L).
    pub hco3: f64,
    /// Excès de base (mmol/L).
    pub be: f64,
    /// PaO2 (mmHg).
    pub pao2: f64,
    /// SaO2 (%).
    pub sao2: f64,
    /// CaO2 (mL/dL).
    pub cao2: f64,
    /// Ventilation alvéolaire (L/min).
    pub va: f64,
    /// P50 corrigée du pH (mmHg).
    pub p50: f64,
    /// Fréquence respiratoire (/min).
    pub rr: f64,
    /// Gradient alvéolo-artériel (mmHg).
    pub aa: f64,
}

/// Gain de la commande ventilatoire à l'acidémie : `exp(0,05 − PH_GAIN·(pH − 7,38))`, pivot au pH du
/// chien sain (PaCO2 40,2 inchangée). Calé le 20/11/2026 sur la compensation respiratoire canine des
/// désordres métaboliques : ΔPaCO2 = 0,74 mmHg par mmol/L de HCO3− (Madias 1984, J Appl Physiol
/// 56:1640, résumé ; 0,7 dans la table de DiBartola 2005, lue dans Mazzaferro & Ford 2012). Avant :
/// `exp(−2,5(pH − 7,4))`, pente 0,41 (docs/VALIDATION_HORS_CALIBRAGE.tex).
pub const PH_GAIN: f64 = 6.0;

/// Facteur de la commande ventilatoire dû au pH artériel.
pub fn ph_drive(ph: f64) -> f64 {
    math::exp(0.05 - PH_GAIN * (ph - 7.38))
}

/// Échanges gazeux : contrôle ventilatoire (PaCO2), oxygénation (PaO2), Bohr.
///
/// PaCO2 résout `PCO2 = 0,863·VCO2/VA` sur [5, 250] mmHg avec
/// `VA = va0·bw·drive·max(0,12, 1 + 0,08(PCO2 − 37))·ph_drive(pH)·(1 + 0,03·max(0, 70 − PaO2))`
/// (stimulation par l'hypercapnie, l'acidémie et l'hypoxémie). 3 itérations externes sur PaO2,
/// en partant de 90 mmHg. `PAO2 = FiO2(Pb − 47) − PCO2/QR` ; le gradient A-a augmente avec la
/// POG au-delà de 18 mmHg (œdème). `P50 = p50·10^(0,37(7,4 − pH))` (Bohr canin, Cambier 2004).
pub fn gas_exchange(p: &Params, la_mean: f64, lactate: f64) -> Result<ArterialGas, Error> {
    let bw = p.bw;
    let vco2 = p.vco2 * bw;
    let va0 = p.va0 * bw;
    let mut pao2 = 90.0;
    let mut out = None;
    for _ in 0..3 {
        // Une erreur de Stewart dans le résidu est remontée après la recherche.
        let mut inner_err = None;
        let resid = |pco2: f64| match stewart_ph(p, pco2, lactate) {
            Ok((ph, _)) => {
                let va = va0
                    * p.vent_drive
                    * f64::max(0.12, 1.0 + 0.08 * (pco2 - 37.0))
                    * ph_drive(ph)
                    * (1.0 + 0.03 * f64::max(0.0, 70.0 - pao2));
                pco2 - 0.863 * vco2 / va.max(1e-3)
            }
            Err(e) => {
                inner_err.get_or_insert(e);
                f64::NAN
            }
        };
        let r = brentq(resid, 5.0, 250.0);
        if let Some(e) = inner_err {
            return Err(e);
        }
        let pco2 = r.map_err(root_err("gas_exchange (PaCO2)", 5.0, 250.0))?;
        let (ph, hco3) = stewart_ph(p, pco2, lactate)?;
        let va = 0.863 * vco2 / pco2;
        let p_alv_o2 = p.fio2 * (p.pb - 47.0) - pco2 / p.rq;
        let aa = p.aagrad + p.edema_k * f64::max(0.0, la_mean - 18.0);
        pao2 = f64::max(p_alv_o2 - aa, 15.0);
        out = Some((pco2, ph, hco3, va, aa));
    }
    let (pco2, ph, hco3, va, aa) = out.expect("3 itérations");
    let p50 = p.p50 * math::pow10(0.37 * (7.4 - ph));
    Ok(ArterialGas {
        pco2,
        ph,
        hco3,
        be: base_excess(ph, hco3, p.hb),
        pao2,
        sao2: 100.0 * hb_o2_sat(pao2, p50, p.hill_n),
        cao2: o2_content(pao2, p.hb, p50, p.hill_n),
        va,
        p50,
        rr: 20.0 * (va / va0).sqrt(),
        aa,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hill_is_half_at_p50() {
        assert!((hb_o2_sat(30.0, 30.0, 2.7) - 0.5).abs() < 1e-15);
        assert_eq!(hb_o2_sat(-5.0, 30.0, 2.7), hb_o2_sat(1e-3, 30.0, 2.7));
    }

    #[test]
    fn po2_inversion_round_trips() {
        let c = o2_content(40.0, 15.0, 30.0, 2.7);
        let po2 = po2_from_content(c, 15.0, 30.0, 2.7).unwrap();
        assert!((po2 - 40.0).abs() < 1e-9);
        assert_eq!(po2_from_content(-1.0, 15.0, 30.0, 2.7).unwrap(), 1.0);
        assert_eq!(po2_from_content(1e3, 15.0, 30.0, 2.7).unwrap(), 600.0);
    }
}
