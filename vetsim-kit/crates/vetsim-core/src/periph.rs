//! Périphérie : thermorégulation, pupille, tube digestif et sécrétions, système nerveux central
//! (miroir de `reference/python/periph.py` ; docs/GUIDE_MODELE.tex, section « Périphérie »).
//!
//! Ces blocs sont des effecteurs : ils lisent les paramètres (modifiés par les médicaments et le
//! scénario : ambiance, anesthésie, éclairage) et le tonus autonome du régime permanent, et
//! produisent des indicateurs cliniques (température, diamètre pupillaire, motilité, salive,
//! score de sédation). Seule la température revient sur l'hémodynamique et le métabolisme
//! (FC intrinsèque, VO2, P50) ; elle est lente (heures) et intégrée par [`thermal_course`],
//! l'évaluation la recevant comme entrée (`temp_c`, séparation des échelles de temps).
//!
//! Provenance (`evidence`) de chaque constante : A–C sourcée, D placeholder borné. Usage
//! pédagogique uniquement.

use crate::engine::Autonomic;
use crate::error::Error;
use crate::math;
use crate::modules::DrugModule;
use crate::params::Params;
use crate::pkpd::{apply_drug_effects_sites, timecourse, Dose};

// ---------------------------------------------------------------- thermorégulation

/// Température centrale de référence (°C) : moyenne 38,6, IR 37,7–39,5 (Dorn 2024, n = 9117).
pub const T_REF: f64 = 38.6;
/// Chaleur spécifique du corps (J/kg/°C) : valeur humaine (D, aucune valeur canine trouvée).
pub const C_BODY: f64 = 3470.0;
/// Fraction de la masse dans le noyau : calée sur la chute de température sous anesthésie
/// (Redondo 2012, n = 1525 : −2,0 / −2,6 / −2,9 °C à 1 / 2 / 3 h), C.
pub const CORE_FRAC: f64 = 0.69;
/// Température moyenne de l'enveloppe en neutralité (°C), D (gradient noyau–enveloppe humain).
pub const T_SHELL0: f64 = 34.6;
/// Limite basse de la neutralité thermique (°C) : 26–29 °C en été, 22–26 °C en hiver
/// (Sugano 1981), C.
pub const T_NEUTRAL: f64 = 26.0;
/// Part de la production de chaleur perdue par voie respiratoire au repos, D.
pub const E_RESP0: f64 = 0.20;
/// Équivalent énergétique de l'O2 (J/mL), conversion standard.
pub const J_PER_ML_O2: f64 = 20.1;
/// VO2 de référence du chien de 20 kg (mL/kg/min) qui fixe les conductances (module sain).
pub const VO2_REF: f64 = 5.5;
/// Q10 de la VO2 : 4,7 → 3,2 mL/kg/min de 37,7 à 30,5 °C (Gutierrez 1986), C.
pub const Q10_VO2: f64 = 1.7;
/// Q10 de la FC : −17 % de 38,5 à ≈ 34 °C sous isoflurane (Yoshida 2001), C (dérivé).
pub const Q10_HR: f64 = 1.51;
/// Effet de la température sur la P50 : Δlog10 P50/ΔT = 0,022 (Reeves 1982) et 0,020
/// (Cambier 2004), B.
pub const BOHR_T: f64 = 0.021;
/// Puissance de l'halogéné selon la température : CAM 1,20 → 0,89 % à 34,5 °C, −26 %
/// (Wilson 2006), C : CAM(T) = CAM·exp(MAC_T·(T − T_REF)).
pub const MAC_T: f64 = 0.086;
/// Gain vasomoteur (log du débit noyau→enveloppe par °C d'écart à la consigne), D.
pub const VM_GAIN: f64 = 3.0;
/// Vasoconstriction maximale (facteur du débit noyau→enveloppe), D.
pub const VM_MIN: f64 = 0.2;
/// Vasodilatation maximale, D.
pub const VM_MAX: f64 = 3.0;
/// Frisson maximal : VO2 × 4 (+300 %, Auld 1980 ; × 4,5 à −21 °C, Minaire 1982), B.
pub const SHIVER_MAX: f64 = 3.0;
/// Seuil du frisson sous la consigne (°C), D (non trouvé chez le chien).
pub const SHIVER_OFFSET: f64 = 0.5;
/// Plage de recrutement du frisson (°C), D.
pub const SHIVER_SPAN: f64 = 1.5;
/// Halètement maximal (pertes évaporatives en multiples de la production basale), D.
pub const PANT_MAX: f64 = 4.0;
/// Seuil du halètement au-dessus de la consigne (°C), D.
pub const PANT_OFFSET: f64 = 0.2;
/// Plage de recrutement du halètement (°C), D.
pub const PANT_SPAN: f64 = 1.0;
/// Part des pertes évaporatives respiratoires due à la glande nasale latérale : 19–36 %
/// (Blatt 1972) ; sa commande cholinergique (bloquée par `musc_block`) est une hypothèse, D.
pub const NASAL_GLAND_FRAC: f64 = 0.27;
/// Baisse du métabolisme sous anesthésie : 4,7 mL/kg/min à 37,7 °C chez le chien anesthésié
/// paralysé (Gutierrez 1986), soit −10 % à 38,6 °C par rapport au vigile (5,5), C.
pub const ANESTH_MET: f64 = 0.10;
/// Pas d'intégration thermique (s).
pub const THERMO_DT: f64 = 60.0;

/// État thermique : températures du noyau et de l'enveloppe (°C).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThermoState {
    /// Noyau (°C).
    pub tc: f64,
    /// Enveloppe (°C).
    pub ts: f64,
}

/// Effecteurs thermiques à la température centrale `tc`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThermoEffectors {
    /// Inhibition des réponses thermorégulatrices par l'anesthésie (0–1).
    pub inhib: f64,
    /// Facteur vasomoteur du débit noyau→enveloppe (1 : neutre).
    pub vasomotor: f64,
    /// Frisson (VO2 ajoutée, en multiples de la VO2 basale).
    pub shiver: f64,
    /// Halètement (pertes évaporatives ajoutées, en multiples de la production basale).
    pub panting: f64,
}

/// Réponses thermorégulatrices proportionnelles à l'écart à la consigne (Hammel 1963) ;
/// l'anesthésie les supprime (inhibition = min(CAM, 1), D).
pub fn effectors(p: &Params, tc: f64) -> ThermoEffectors {
    let inhib = p.anesth_mac.clamp(0.0, 1.0);
    let e = tc - p.t_set;
    let vm = (VM_GAIN * e).clamp(math::ln(VM_MIN), math::ln(VM_MAX));
    ThermoEffectors {
        inhib,
        vasomotor: math::exp((1.0 - inhib) * vm),
        shiver: (1.0 - inhib) * SHIVER_MAX * ((-e - SHIVER_OFFSET) / SHIVER_SPAN).clamp(0.0, 1.0),
        panting: (1.0 - inhib) * PANT_MAX * ((e - PANT_OFFSET) / PANT_SPAN).clamp(0.0, 1.0),
    }
}

/// Production basale de référence (W/kg) et conductances (W/kg/°C) du patient : noyau→enveloppe
/// `k0` (débit sanguin, ∝ BW^−0,25) et enveloppe→ambiance `g` (surface/masse, ∝ BW^−1/3),
/// fixées pour l'équilibre du chien de référence en neutralité.
fn conductances(p: &Params) -> (f64, f64) {
    let m_ref = VO2_REF * J_PER_ML_O2 / 60.0;
    let s = p.bw / 20.0;
    let k0 = (1.0 - E_RESP0) * m_ref / (T_REF - T_SHELL0) * math::pow(s, -0.25);
    let g = (1.0 - E_RESP0) * m_ref / (T_SHELL0 - T_NEUTRAL) * math::pow(s, -1.0 / 3.0);
    (k0, g)
}

/// Dérivées (°C/s) du modèle noyau/enveloppe :
/// `fc·c·dTc/dt = M − E − K·(Tc − Ts)`, `(1 − fc)·c·dTs/dt = K·(Tc − Ts) − G·(Ts − Ta) + W`,
/// `M = M0·(1 − 0,1·I)·Q10^((Tc − 38,6)/10) + M0·frisson`,
/// `E = (E_resp0·M_basal + M0·halètement)·(1 − 0,27·musc_block)`.
pub fn thermo_rhs(p: &Params, st: ThermoState) -> (f64, f64) {
    let m0 = p.vo2 * J_PER_ML_O2 / 60.0;
    let (k0, g) = conductances(p);
    let ef = effectors(p, st.tc);
    let m_basal = m0 * (1.0 - ANESTH_MET * ef.inhib) * math::pow(Q10_VO2, (st.tc - T_REF) / 10.0);
    let m = m_basal + m0 * ef.shiver;
    let evap = (E_RESP0 * m_basal + m0 * ef.panting) * (1.0 - NASAL_GLAND_FRAC * p.musc_block);
    let k = k0 * ef.vasomotor;
    let q_cs = k * (st.tc - st.ts);
    (
        (m - evap - q_cs) / (CORE_FRAC * C_BODY),
        (q_cs - g * (st.ts - p.t_amb) + p.warming) / ((1.0 - CORE_FRAC) * C_BODY),
    )
}

fn rk4(p: &Params, st: ThermoState, dt: f64) -> ThermoState {
    let add = |s: ThermoState, d: (f64, f64), h: f64| ThermoState {
        tc: s.tc + h * d.0,
        ts: s.ts + h * d.1,
    };
    let k1 = thermo_rhs(p, st);
    let k2 = thermo_rhs(p, add(st, k1, dt / 2.0));
    let k3 = thermo_rhs(p, add(st, k2, dt / 2.0));
    let k4 = thermo_rhs(p, add(st, k3, dt));
    ThermoState {
        tc: st.tc + dt / 6.0 * (k1.0 + 2.0 * k2.0 + 2.0 * k3.0 + k4.0),
        ts: st.ts + dt / 6.0 * (k1.1 + 2.0 * k2.1 + 2.0 * k3.1 + k4.1),
    }
}

/// Durée d'intégration de l'équilibre thermique initial (h).
pub const THERMO_EQ_H: f64 = 72.0;

/// État thermique d'équilibre pour les paramètres `p` (intégration RK4 sur 72 h depuis la
/// consigne et l'enveloppe neutre).
pub fn thermo_equilibrium(p: &Params) -> ThermoState {
    let mut st = ThermoState {
        tc: p.t_set,
        ts: T_SHELL0,
    };
    let n = (THERMO_EQ_H * 3600.0 / THERMO_DT).round() as usize;
    for _ in 0..n {
        st = rk4(p, st, THERMO_DT);
    }
    st
}

/// Évolution thermique avec médicaments : départ à l'équilibre des paramètres sans médicament,
/// puis intégration RK4 au pas de 60 s, les effets des médicaments étant appliqués aux
/// paramètres au début de chaque pas (concentrations de [`timecourse`] sur la grille de 1 min).
/// Renvoie l'état aux instants `t_eval_h` (interpolation linéaire entre deux pas).
pub fn thermal_course(
    p: &Params,
    catalog_drugs: &[DrugModule],
    doses: &[Dose],
    t_eval_h: &[f64],
) -> Result<Vec<ThermoState>, Error> {
    for &t in t_eval_h {
        if !(t.is_finite() && t >= 0.0) {
            return Err(Error::InvalidInput(format!("instant {t} h invalide (≥ 0)")));
        }
    }
    let t_end = t_eval_h.iter().copied().fold(0.0, f64::max);
    let n = (t_end * 3600.0 / THERMO_DT).ceil() as usize;
    let grid: Vec<f64> = (0..=n).map(|k| k as f64 * THERMO_DT / 3600.0).collect();
    let tc = if doses.is_empty() {
        None
    } else {
        Some(timecourse(p, catalog_drugs, doses, grid[n], &grid)?)
    };
    let mut states = Vec::with_capacity(n + 1);
    let mut st = thermo_equilibrium(p);
    states.push(st);
    for k in 0..n {
        let q = match &tc {
            None => p.clone(),
            Some(tc) => {
                let conc: Vec<(String, f64)> =
                    tc.conc.iter().map(|(d, v)| (d.clone(), v[k])).collect();
                let conc_e: Vec<(String, f64)> =
                    tc.conc_e.iter().map(|(d, v)| (d.clone(), v[k])).collect();
                apply_drug_effects_sites(p, catalog_drugs, &conc, &conc_e, tc.d[k], tc.u[k])?
            }
        };
        st = rk4(&q, st, THERMO_DT);
        states.push(st);
    }
    Ok(t_eval_h
        .iter()
        .map(|&t| {
            let x = t * 3600.0 / THERMO_DT;
            let i = (x.floor() as usize).min(n);
            let j = (i + 1).min(n);
            let w = x - i as f64;
            ThermoState {
                tc: states[i].tc + w * (states[j].tc - states[i].tc),
                ts: states[i].ts + w * (states[j].ts - states[i].ts),
            }
        })
        .collect())
}

/// Effets de la température centrale `temp_c` sur l'évaluation : FC intrinsèque (Q10 1,51),
/// VO2 (Q10 1,7, anesthésie −10 %, frisson), P50 (Bohr thermique). Identité exacte à 38,6 °C
/// sans anesthésie ni frisson.
pub fn apply_temperature(p: &Params) -> Params {
    let mut q = p.clone();
    let dt = p.temp_c - T_REF;
    let ef = effectors(p, p.temp_c);
    q.hr = p.hr * math::pow(Q10_HR, dt / 10.0);
    q.vo2 = p.vo2 * ((1.0 - ANESTH_MET * ef.inhib) * math::pow(Q10_VO2, dt / 10.0) + ef.shiver);
    q.p50 = p.p50 * math::pow10(BOHR_T * dt);
    q
}

// ---------------------------------------------------------------- pupille

/// Diamètre minimal (myosis maximal, mm) : 1,02 ± 0,32 (Weisse 1975, cité par Mills 2022), B.
pub const PUPIL_MIN: f64 = 1.0;
/// Diamètre maximal (mydriase pharmacologique, mm) : 11,02 ± 0,83 (Weisse 1975), B.
pub const PUPIL_MAX: f64 = 11.0;
/// Diamètre en lumière ambiante, vigile (mm) : 8,53 ± 1,30 (Mills 2022, 209–236 lux), A.
pub const PUPIL_AMBIENT: f64 = 8.53;
/// Diamètre à l'obscurité, vigile (mm) : 10,06 ± 0,54 (Whiting 2013, cité par Mills 2022), B.
pub const PUPIL_DARK: f64 = 10.06;
/// Diamètre minimal après flash, lumière ambiante (mm) : 6,57 ± 1,41 (Mills 2022), A.
pub const PUPIL_FLASH: f64 = 6.57;
/// Tonus sympathique total de référence (chien sain au repos).
pub const S_REF: f64 = 0.2;

/// Indicateurs pupillaires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pupil {
    /// Diamètre (mm).
    pub diameter: f64,
    /// Diamètre minimal après flash (mm).
    pub flash: f64,
    /// Amplitude du réflexe photomoteur (%).
    pub plr_pct: f64,
}

fn open_fraction(dil: f64, sph: f64) -> f64 {
    if dil + sph > 0.0 {
        dil / (dil + sph)
    } else {
        0.5
    }
}

/// Pupille : équilibre du dilatateur (α1, tonus sympathique total `s`) et du sphincter
/// (muscarinique M3, commande du noyau d'Edinger-Westphal modulée par l'éclairement),
/// `D = Dmin + (Dmax − Dmin)·dil/(dil + sph)` (innervation réciproque : Yoshitomi 1986).
/// Les gains sont fixés par trois diamètres mesurés : ambiant, obscurité, après flash.
pub fn pupil(p: &Params, s_total: f64) -> Pupil {
    let span = PUPIL_MAX - PUPIL_MIN;
    let f = |d: f64| (d - PUPIL_MIN) / span;
    // sphincter = 1 en lumière ambiante : dilatateur de référence
    let dil0 = f(PUPIL_AMBIENT) / (1.0 - f(PUPIL_AMBIENT));
    let sph_dark = dil0 * (1.0 - f(PUPIL_DARK)) / f(PUPIL_DARK);
    let flash_gain = dil0 * (1.0 - f(PUPIL_FLASH)) / f(PUPIL_FLASH);
    let dil = dil0 * f64::max(0.0, s_total) / S_REF;
    let light = p.light.clamp(0.0, 1.0);
    let sph = (1.0 + p.pupil_ew)
        * (sph_dark + (1.0 - sph_dark) * light)
        * (1.0 - p.pupil_block.clamp(0.0, 1.0));
    let d = PUPIL_MIN + span * open_fraction(dil, sph);
    let df = PUPIL_MIN + span * open_fraction(dil, sph * flash_gain);
    Pupil {
        diameter: d,
        flash: df,
        plr_pct: 100.0 * (d - df) / d,
    }
}

// ---------------------------------------------------------------- digestif, SNC

/// Pression du sphincter œsophagien inférieur (mmHg) : 47,9 ± 1,2, 13,2 ± 2,0 sous atropine
/// (Strombeck 1985), B.
pub const LES_BASE: f64 = 47.9;
/// Baisse relative de la pression du SOI au blocage muscarinique complet (Strombeck 1985).
pub const LES_MUSC: f64 = 1.0 - 13.2 / 47.9;
/// Baisse de la force de contraction gastrique sous anesthésie générale : 11 → 3 mmHg sous
/// sévoflurane (Boscan 2014), C.
pub const GI_ANESTH: f64 = 1.0 - 3.0 / 11.0;
/// CAM à laquelle la sédation est à mi-chemin de la perte de conscience, D.
pub const MAC_SED50: f64 = 0.4;
/// Score de sédation de base (échelle de Grint/Wagner 0–21 : base médiane 1, Wagner 2017), B.
pub const SED_BASE: f64 = 1.0;
/// Index bispectral selon la CAM d'isoflurane (Greene 2003, chiens curarisés) : (CAM, BIS).
pub const BIS_TABLE: [(f64, f64); 4] = [(0.8, 65.0), (1.0, 60.0), (1.5, 52.0), (2.0, 31.0)];

/// Indicateurs de la périphérie.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peripheral {
    /// Température centrale (°C).
    pub temp_c: f64,
    /// Effecteurs thermiques à `temp_c`.
    pub thermo: ThermoEffectors,
    /// Pupille.
    pub pupil: Pupil,
    /// Motilité digestive (fraction de la normale).
    pub gi_motility: f64,
    /// Pression du sphincter œsophagien inférieur (mmHg).
    pub les_mmhg: f64,
    /// Sécrétion salivaire (fraction de la normale).
    pub salivation: f64,
    /// CAM effective (corrigée de la température).
    pub mac_eff: f64,
    /// Score de sédation (0–21).
    pub sedation_score: f64,
    /// Excitation centrale (0–1).
    pub excitation: f64,
    /// Index bispectral, dans le domaine mesuré (CAM 0,8–2,0), sinon `None`.
    pub bis: Option<f64>,
}

/// BIS interpolé linéairement dans [`BIS_TABLE`] ; `None` hors du domaine mesuré.
pub fn bis(mac: f64) -> Option<f64> {
    let (lo, hi) = (BIS_TABLE[0].0, BIS_TABLE[BIS_TABLE.len() - 1].0);
    if !(lo..=hi).contains(&mac) {
        return None;
    }
    BIS_TABLE.windows(2).find_map(|w| {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        (mac <= x1).then(|| y0 + (y1 - y0) * (mac - x0) / (x1 - x0))
    })
}

/// Indicateurs de la périphérie pour les paramètres `p` et le tonus autonome du régime permanent.
pub fn peripheral(p: &Params, a: &Autonomic) -> Peripheral {
    let mac_eff = f64::max(0.0, p.anesth_mac) * math::exp(-MAC_T * (p.temp_c - T_REF));
    let musc = p.musc_block.clamp(0.0, 1.0);
    let drive = f64::max(0.0, p.sedation) + math::pow(mac_eff / MAC_SED50, 4.0);
    Peripheral {
        temp_c: p.temp_c,
        thermo: effectors(p, p.temp_c),
        pupil: pupil(p, a.s),
        gi_motility: (1.0 - musc)
            * (1.0 - p.gi_inhib.clamp(0.0, 1.0))
            * (1.0 - GI_ANESTH * mac_eff.min(1.0)),
        les_mmhg: LES_BASE * (1.0 - LES_MUSC * musc),
        salivation: 1.0 - musc,
        mac_eff,
        sedation_score: SED_BASE + (21.0 - SED_BASE) * drive / (1.0 + drive),
        excitation: p.excitation.clamp(0.0, 1.0),
        bis: bis(mac_eff),
    }
}

impl Peripheral {
    /// Sortie plate (contrat CLI `eval`) ; `bis` absent hors du domaine mesuré.
    pub fn to_flat(&self) -> Vec<(&'static str, f64)> {
        let mut out = vec![
            ("temp_c", self.temp_c),
            ("shiver", self.thermo.shiver),
            ("panting", self.thermo.panting),
            ("vasomotor", self.thermo.vasomotor),
            ("pupil_mm", self.pupil.diameter),
            ("plr_pct", self.pupil.plr_pct),
            ("gi_motility", self.gi_motility),
            ("les_mmhg", self.les_mmhg),
            ("salivation", self.salivation),
            ("mac_eff", self.mac_eff),
            ("sedation_score", self.sedation_score),
            ("excitation", self.excitation),
        ];
        if let Some(b) = self.bis {
            out.push(("bis", b));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::PatientModule;
    use crate::patient::healthy_params;

    fn healthy() -> Params {
        let src = include_str!("../../../modules/healthy_dog.json");
        let m = PatientModule::from_json(src, "healthy_dog").unwrap();
        healthy_params(&m, 20.0).unwrap()
    }

    #[test]
    fn neutral_equilibrium_is_reference_temperature() {
        let p = healthy();
        let st = thermo_equilibrium(&p);
        assert!((st.tc - T_REF).abs() < 1e-6, "{st:?}");
        assert!((st.ts - T_SHELL0).abs() < 1e-6, "{st:?}");
        // identité exacte de l'évaluation à 38,6 °C
        assert_eq!(apply_temperature(&p), p);
    }

    #[test]
    fn anaesthesia_cooling_follows_clinical_cohort() {
        // Redondo 2012 (n = 1525) : −2,0 ± 1,3 / −2,6 ± 1,4 / −2,9 ± 1,5 °C à 1 / 2 / 3 h
        // (salle à ≈ 21 °C, hypothèse) ; départ à l'équilibre vigile à 21 °C.
        let mut p = healthy();
        p.t_amb = 21.0;
        let mut st = thermo_equilibrium(&p);
        let t0 = st.tc;
        assert!(t0 > 38.0, "vigile à 21 °C : {t0}");
        p.anesth_mac = 1.0;
        let mut drop = Vec::new();
        for k in 1..=180 {
            st = rk4(&p, st, THERMO_DT);
            if k % 60 == 0 {
                drop.push(st.tc - t0);
            }
        }
        for (d, (obs, sd)) in drop.iter().zip([(-2.0, 1.3), (-2.6, 1.4), (-2.9, 1.5)]) {
            assert!((d - obs).abs() < 0.5 * sd, "chute {d:.2} vs {obs}");
        }
        // le réchauffement actif ralentit la chute (Jones 2023 : 37,7 vs 36,6 °C)
        let mut q = p.clone();
        q.warming = 1.0;
        let mut sw = thermo_equilibrium(&{
            let mut a = q.clone();
            a.anesth_mac = 0.0;
            a
        });
        for _ in 0..180 {
            sw = rk4(&q, sw, THERMO_DT);
        }
        assert!(sw.tc > st.tc + 0.5);
    }

    #[test]
    fn cold_and_heat_recruit_shivering_and_panting() {
        let mut p = healthy();
        p.t_amb = -10.0;
        let st = thermo_equilibrium(&p);
        let e = effectors(&p, st.tc);
        assert!(e.shiver > 0.0 && e.vasomotor < 0.5, "{e:?} {st:?}");
        assert!(st.tc > 36.5, "défense contre le froid : {st:?}");
        p.t_amb = 38.0;
        let st = thermo_equilibrium(&p);
        let e = effectors(&p, st.tc);
        assert!(e.panting > 0.0 && e.vasomotor > 1.0, "{e:?} {st:?}");
        assert!(st.tc < 40.5, "défense contre la chaleur : {st:?}");
    }

    #[test]
    fn pupil_reproduces_measured_diameters() {
        let mut p = healthy();
        let pp = pupil(&p, S_REF);
        assert!((pp.diameter - PUPIL_AMBIENT).abs() < 1e-9);
        assert!((pp.flash - PUPIL_FLASH).abs() < 1e-9);
        // amplitude du réflexe : 23,5 ± 7,0 % (Mills 2022)
        assert!((pp.plr_pct - 23.0).abs() < 7.0, "{}", pp.plr_pct);
        p.light = 0.0;
        assert!((pupil(&p, S_REF).diameter - PUPIL_DARK).abs() < 1e-9);
        p.light = 1.0;
        p.pupil_block = 1.0;
        let m = pupil(&p, S_REF);
        assert!(m.diameter > 10.9 && m.plr_pct < 1e-9, "{m:?}");
    }

    #[test]
    fn bis_table_domain() {
        assert_eq!(bis(0.5), None);
        assert_eq!(bis(1.0), Some(60.0));
        assert!((bis(1.25).unwrap() - 56.0).abs() < 1e-12);
    }
}
