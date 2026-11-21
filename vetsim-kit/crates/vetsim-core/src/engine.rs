//! Orchestration physiologique (miroir de `physio.evaluate`).
//!
//! M2 : partie **algébrique** en aval de l'hémodynamique — perfusion et transport d'O2,
//! lactate, gaz artériel et veineux mêlé, rein, biochimie ([`metabolic_state`]).
//! M3 y branchera la boucle baroréflexe et la simulation cardiovasculaire.

use crate::error::Error;
use crate::gas::{gas_exchange, po2_from_content, stewart_ph, ArterialGas};
use crate::math;
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
    /// Pression veineuse centrale (mmHg) : congestion veineuse rénale.
    pub cvp: f64,
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
    let lact = p.lact0 + 9.0 / (1.0 + math::exp(-(er - 0.55) / 0.06));
    let g = gas_exchange(p, h.la_mean, lact)?;
    let do2 = h.co * 1000.0 * g.cao2 / 100.0;
    // --- sang veineux mêlé
    let cv = g.cao2 - vo2 / (h.co * 10.0); // mL/dL
    let pvo2 = po2_from_content(cv, p.hb, g.p50, p.hill_n)?;
    let dpco2 = 5.5 * (125.0 * bw / 1000.0) / h.co.max(0.2);
    let pvco2 = g.pco2 + dpco2;
    let (phv, hco3v) = stewart_ph(p, pvco2, lact)?;
    // --- rein
    let co_ref = 3.29 * math::pow(bw / 20.0, 0.75);
    let by_map = ((h.map - 40.0) / 35.0).clamp(0.0, 1.0);
    // autorégulation : le débit ne limite la filtration que sous `renal_co_auto`·débit de
    // référence (rampe de largeur 0,5 ; 1,0 redonne la rampe historique 0,5–1,0)
    let by_co = ((h.co / co_ref - (p.renal_co_auto - 0.5)) / 0.5).clamp(0.0, 1.0);
    let perf = by_map.min(by_co).max(0.25);
    // axe cardio-rénal : congestion veineuse rénale (PVC) et vasoconstriction rénale
    // neurohormonale (tonus sympathique, RAAS) abaissent le DFG à PAM et débit conservés
    let f_cong = (1.0 - p.renal_k_cong * f64::max(0.0, h.cvp - p.renal_cvp_ref)).max(0.3);
    let f_symp = (1.0 - p.renal_k_symp * f64::max(0.0, p.sympathetic - p.renal_s_ref)).max(0.3);
    let gfr_ratio = (p.nephron_mass * perf * f_cong * f_symp).max(0.03);
    let chem = Chemistry {
        na: p.na,
        k: p.k,
        cl: p.cl,
        lactate: lact,
        glucose: p.glucose,
        creat: p.creat0 / gfr_ratio,
        urea: p.urea0 / math::pow(gfr_ratio, 0.8),
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
/// Nombre maximal d'évaluations hémodynamiques du solveur du baroréflexe.
pub const BARO_MAX_ITER: usize = 16;
/// Seuil d'arrêt sur le résidu de la commande barorécepteur `|d_t(d) − d|` (0,01 ≈ 0,45 mmHg).
pub const BARO_TOL: f64 = 0.01;

/// Prochaine commande barorécepteur du solveur du point fixe `g(d) = d_t(d) − d = 0` sur
/// `[−1, 1]`. `g` décroît avec `d` (plus de sympathique, PAM plus haute), `g(−1) ≥ 0 ≥ g(1)` :
/// la racine est unique et encadrée. Premier pas amorti `d + g/2` (l'ancienne itération), puis
/// sécante sur les deux derniers points ; bisection de l'encadrement courant si la sécante en
/// sort (ou est dégénérée). `lo` : plus grand `d` avec `g > 0`, `hi` : plus petit `d` avec `g < 0`.
pub fn baro_next(d: f64, g: f64, prev: Option<(f64, f64)>, lo: f64, hi: f64) -> f64 {
    let mut next = match prev {
        Some((dp, gp)) if (g - gp).abs() > 1e-12 => d - g * (d - dp) / (g - gp),
        _ => d + 0.5 * g,
    };
    if !(next > lo && next < hi) {
        next = 0.5 * (lo + hi);
    }
    next.clamp(-1.0, 1.0)
}

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
    /// FC sinusale (/min) ; `params.hr` est la FC ventriculaire.
    pub sinus_hr: f64,
    /// Séquence de conduction (rythme non trivial), sinon `None` (sinusal 1:1).
    pub conduction: Option<crate::rhythm::Conduction>,
    /// Indicateurs battement par battement (rythme non trivial).
    pub beats: Vec<crate::hemo::BeatMetrics>,
    /// Facteurs de couplage de décompensation retenus.
    pub decomp: Decompensation,
    /// Issue clinique mécanistique (stable ou décompensé ; le décès est une erreur `NonViable`).
    pub outcome: Outcome,
}

/// Facteurs de couplage appliqués à la passe retenue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decompensation {
    /// Facteur de contractilité (acidémie × hypoxie tissulaire × ischémie coronaire).
    pub f_contract: f64,
    /// Facteur de résistance systémique (vasoplégie acidémique).
    pub f_rsys: f64,
    /// Facteur de commande ventilatoire (faiblesse hypokaliémique).
    pub f_vent: f64,
    /// Nombre de passes du point fixe (1 si aucun couplage actif).
    pub iterations: usize,
}

/// Mécanisme de décompensation actif.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mechanism {
    /// Identifiant (`acidemia_depression`, `hypoxic_depression`, `coronary_ischemia`,
    /// `hypokalemic_weakness`, `atrial_standstill`, `conduction_block`, `anaerobic_metabolism`,
    /// `pulmonary_edema`, `renal_hypoperfusion`, `baroreflex_exhausted`).
    pub id: &'static str,
    /// Valeur de la variable qui le déclenche.
    pub value: f64,
    /// Seuil du modèle.
    pub threshold: f64,
    /// Facteur appliqué (1 pour un mécanisme déjà présent dans le modèle).
    pub factor: f64,
}

/// Issue d'une évaluation viable.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// `true` si au moins un mécanisme de décompensation est actif.
    pub decompensated: bool,
    /// Mécanismes actifs, dans un ordre fixe.
    pub mechanisms: Vec<Mechanism>,
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
        out.extend(self.peripheral().to_flat());
        out
    }

    /// Indicateurs de la périphérie (température, pupille, digestif, SNC).
    pub fn peripheral(&self) -> crate::periph::Peripheral {
        crate::periph::peripheral(&self.params, &self.autonomic)
    }
}

/// Rythme non trivial : séquence de conduction sur `RHYTHM_T_END` s, moyennes après
/// `RHYTHM_WARM` s ; aléa SplitMix64 à graine fixe, identique à la référence Python.
pub const RHYTHM_T_END: f64 = 30.0;
/// Chauffe de la simulation battement par battement (s).
pub const RHYTHM_WARM: f64 = 10.0;
/// Graine fixe de l'aléa de conduction du moteur.
pub const RHYTHM_SEED: u64 = 0;

type RhythmHemo = (
    crate::hemo::HemoResult,
    Option<crate::rhythm::Conduction>,
    Vec<crate::hemo::BeatMetrics>,
);

/// Fuite sous laquelle l'orifice dynamique est ignoré (chien sain : `g_mr` 0,0005 mL/s/mmHg).
const MR_DYN_MIN_G: f64 = 0.05;
/// Itérations au plus du point fixe de l'orifice dynamique.
const MR_DYN_MAX_ITER: usize = 12;

/// Hémodynamique d'une passe avec orifice mitral dynamique (Yoran 1979 : l'orifice effectif
/// grandit avec le VG). L'orifice est fixe pendant le battement et suit la taille du VG :
/// `g = g_mr·(VTD/mr_vref)^mr_k`, point fixe amorti sur ln g (relaxation ½, tolérance 1e-3).
/// Renvoie aussi la conductance retenue (`g_mr` si l'orifice est fixe).
fn hemo_with_rhythm(q: &Params, v_eff: f64, n_beats: usize) -> Result<(RhythmHemo, f64), Error> {
    if q.mr_k == 0.0 || q.g_mr < MR_DYN_MIN_G {
        return Ok((hemo_with_rhythm_static(q, v_eff, n_beats)?, q.g_mr));
    }
    let mut qq = q.clone();
    let mut lg = 0.0; // ln(g/g_mr)
    let mut out = None;
    for _ in 0..MR_DYN_MAX_ITER {
        qq.g_mr = q.g_mr * math::exp(lg);
        let r = hemo_with_rhythm_static(&qq, v_eff, n_beats)?;
        let target = q.mr_k * math::ln(f64::max(r.0.summary.edv, 1e-6) / q.mr_vref);
        let done = (target - lg).abs() < 1e-3;
        out = Some(r);
        if done {
            break;
        }
        lg = 0.5 * lg + 0.5 * target;
    }
    Ok((out.expect("au moins une itération"), qq.g_mr))
}

/// Hémodynamique d'une passe à orifice fixe : périodique si la conduction est sinusale 1:1 à PR
/// constant (PR de la conduction), sinon battement par battement (miroir de
/// `physio._hemo_with_rhythm`).
fn hemo_with_rhythm_static(q: &Params, v_eff: f64, n_beats: usize) -> Result<RhythmHemo, Error> {
    use crate::hemo::{simulate_hemo_beats, simulate_hemo_pr};
    use crate::rhythm::{is_trivial, simulate_conduction, steady_pr};
    if !(q.hr.is_finite() && q.hr > 0.0) {
        return Err(Error::InvalidInput(format!("FC {} /min", q.hr)));
    }
    let ts = 60.0 / q.hr;
    if is_trivial(q, v_eff, ts) {
        let h = simulate_hemo_pr(q, n_beats, steady_pr(q, v_eff, ts))?;
        return Ok((h, None, Vec::new()));
    }
    let mut rng = crate::rhythm::SplitMix64::new(RHYTHM_SEED);
    let cond = simulate_conduction(q, v_eff, ts, RHYTHM_T_END, &mut || rng.random(), None);
    if cond
        .ventricular
        .iter()
        .filter(|b| b.t >= RHYTHM_WARM)
        .count()
        < 3
    {
        return Err(Error::NonViable {
            cause: "cardiac_arrest",
            last: None,
        });
    }
    let b = simulate_hemo_beats(q, &cond, RHYTHM_T_END, RHYTHM_WARM, ts)?;
    let beats = b.beats.clone();
    Ok((b.into_hemo_result(q), Some(cond), beats))
}

/// Une passe : baroréflexe en régime permanent puis aval métabolique (MODEL_SPEC §3).
///
/// À chaque passage, la commande `d` (> 0 en hypotension) augmente le tonus sympathique et
/// réduit le tonus vagal ; ils modulent la FC (avec la dépression par l'hyperkaliémie au-delà de
/// 6 mmol/L), l'inotropisme, la résistance systémique et le recrutement du volume non contraint.
/// Le point fixe `d = d_t(d)` est résolu par [`baro_next`] (arrêt si `|d_t − d| < BARO_TOL`) ;
/// l'état retenu est celui de la dernière évaluation.
fn evaluate_once(base: &Params) -> Result<Evaluation, Error> {
    use crate::hemo::N_BEATS;
    let mut d = 0.0;
    let mut last = None;
    let mut iterations = 0;
    let (mut lo, mut hi) = (-1.0_f64, 1.0_f64);
    let mut prev: Option<(f64, f64)> = None;
    for _ in 0..BARO_MAX_ITER {
        iterations += 1;
        let s_tot = (base.s0 + 0.8 * d).clamp(0.0, 1.0);
        let v_tot = (base.v0 + base.vagal_add - 0.6 * d).clamp(0.0, 1.0);
        let s_eff = s_tot * (1.0 - base.beta_block);
        let v_eff = v_tot * (1.0 - base.vagal_block);
        let mut q = base.clone();
        // FC sinusale ; K⁺ agit par la conduction (rhythm), plus par un frein multiplicatif
        q.hr = base.hr * hr_factor(s_eff, v_eff, base) / hr_factor(S_REF, V_REF, base);
        q.ees_lv = base.ees_lv * (1.0 + 0.4 * (s_eff - base.s0));
        q.ees_rv = base.ees_rv * (1.0 + 0.4 * (s_eff - base.s0));
        // Effet dromotrope du sympathique (bloquable par les bêtabloquants) : ERP et AH nodaux.
        let symp = f64::max(0.1, 1.0 - base.av_symp * (s_eff - S_REF));
        q.av_erp = base.av_erp * symp;
        // AH seul est modulé (la conduction intra-atriale pa_ia et HV ne le sont pas)
        q.pr = base.hv + base.pa_ia + (base.pr - base.hv - base.pa_ia) * symp;
        // Foyers d'échappement : accélérés par le sympathique, le jonctionnel ralenti par le vague
        // (Tanahashi 1979, chien vigile : atropine et practolol modifient la pause d'échappement).
        let esc_s = (1.0 + base.esc_a_s * s_eff) / (1.0 + base.esc_a_s * S_REF);
        let esc_v = f64::max(0.0, 1.0 - base.esc_a_v * v_eff) / (1.0 - base.esc_a_v * V_REF);
        q.junct_rate = base.junct_rate * esc_s * esc_v;
        q.vent_escape_rate = base.vent_escape_rate * esc_s;
        q.r_sys = base.r_sys * (1.0 + 0.5 * (s_tot - base.s0));
        q.vol_shift = base.vol_shift + 0.20 * 85.0 * base.bw * (s_tot - base.s0);
        let ((h, cond, beats), g_eff) = hemo_with_rhythm(&q, v_eff, N_BEATS)?;
        // orifice retenu : les recalculs en aval (signaux, son) le réutilisent tel quel
        q.g_mr = g_eff;
        q.mr_k = 0.0;
        let d_t = (base.baro_gain * (base.map_set - h.summary.map) / 45.0).clamp(-1.0, 1.0);
        let g = d_t - d;
        last = Some((q, h, s_tot, v_tot, s_eff, v_eff, cond, beats));
        if g.abs() < BARO_TOL {
            break;
        }
        if g > 0.0 {
            lo = lo.max(d);
        } else {
            hi = hi.min(d);
        }
        let next = baro_next(d, g, prev, lo, hi);
        prev = Some((d, g));
        d = next;
    }
    let (mut q, h, s_tot, v_tot, s_eff, v_eff, conduction, beats) =
        last.expect("au moins un passage");
    let sinus_hr = q.hr;
    q.hr = 60.0 / h.timing.period; // FC rapportée : FC ventriculaire
    q.sympathetic = s_tot;
    let hs = &h.summary;
    let metabolic = metabolic_state(
        &q,
        &HemoInputs {
            co: hs.co,
            map: hs.map,
            la_mean: hs.la_mean,
            cvp: hs.cvp,
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
        sinus_hr,
        conduction,
        beats,
        decomp: Decompensation {
            f_contract: 1.0,
            f_rsys: 1.0,
            f_vent: 1.0,
            iterations: 1,
        },
        outcome: Outcome {
            decompensated: false,
            mechanisms: Vec::new(),
        },
    })
}

// ------------------------------------------------------------------ décompensation et viabilité
// Couplages mécanistiques en retour de l'aval métabolique sur la pompe et les vaisseaux
// (miroir de `physio.evaluate`). Seuils et pentes : paramètres du patient, placeholders D.

/// Passes au plus avant de conclure à une spirale non résolue.
pub const DECOMP_MAX_ITER: usize = 30;
/// Convergence des facteurs de couplage.
pub const DECOMP_TOL: f64 = 1e-3;
/// Plancher des facteurs de couplage.
pub const F_FLOOR: f64 = 0.1;
/// Extraction d'O2 au point d'inflexion du lactate.
pub const ER_ANAEROBIC: f64 = 0.55;
/// POG au-delà de laquelle le gradient A-a s'ouvre (œdème pulmonaire). 24 mmHg depuis le
/// 21/11/2026 (18 avant) : œdème au-delà d'environ 24 mmHg chez le chien (Guyton & Lindsey 1959,
/// cité par Yao & Aoki 2025) ; voir docs/VALIDATION_HORS_CALIBRAGE.tex.
pub const LA_EDEMA: f64 = 24.0;
/// PAM sous laquelle la perfusion rénale baisse (40 + 35).
pub const MAP_RENAL: f64 = 75.0;

/// Commande de la réponse à l'hypoxémie,
/// `h = clip((hypox_pao2 − PaO2)/(hypox_pao2 − hypox_pao2_sat), 0, 1)^hypox_n` : tachycardie,
/// inotropisme et vasodilatation du chien conscient en hypoxie aiguë (Krasney & Koehler 1977 ;
/// Krebs 1999 ; présentes aussi après dénervation sinoaortique, donc pas seulement chémoréflexes).
/// Saturée sous `hypox_pao2_sat` : au-delà, la dépression hypoxique (PvO2) l'emporte. Calée le
/// 20/11/2026 (docs/VALIDATION_HORS_CALIBRAGE.tex).
pub fn hypoxemic_drive(p: &Params, pao2: f64) -> f64 {
    let span = p.hypox_pao2 - p.hypox_pao2_sat;
    if p.hypox_pao2 <= 0.0 || span <= 0.0 {
        return 0.0;
    }
    let x = ((p.hypox_pao2 - pao2) / span).clamp(0.0, 1.0);
    math::pow(x, p.hypox_n)
}

/// Facteurs cibles (contractilité, résistance, acidémie, hypoxie, coronaire) d'une passe.
fn coupling_targets(p: &Params, e: &Evaluation) -> (f64, f64, f64, f64, f64) {
    let ph = e.metabolic.art.ph;
    let pvo2 = e.metabolic.ven.po2;
    let f_acid_c = f64::max(F_FLOOR, 1.0 - p.acid_ees * f64::max(0.0, p.acid_ph0 - ph));
    let f_acid_r = f64::max(F_FLOOR, 1.0 - p.acid_rsys * f64::max(0.0, p.acid_ph0 - ph));
    let f_hyp = f64::max(
        F_FLOOR,
        1.0 - p.hypox_ees * f64::max(0.0, p.hypox_pvo2 - pvo2),
    );
    let cpp = e.hemo.summary.dap - e.hemo.summary.lv_edp;
    let f_cor = f64::max(F_FLOOR, 1.0 - p.cpp_ees * f64::max(0.0, p.cpp_min - cpp));
    (f_acid_c * f_hyp * f_cor, f_acid_r, f_acid_c, f_hyp, f_cor)
}

/// Erreurs d'une passe qui traduisent l'absence d'état stationnaire (racine introuvable,
/// intégration impossible, débit ou FC non physiologiques) plutôt qu'une entrée invalide.
fn is_physiological_failure(e: &Error) -> bool {
    matches!(
        e,
        Error::Root { .. } | Error::Ode(_) | Error::InvalidInput(_)
    )
}

/// Évalue le patient avec les couplages de décompensation (point fixe amorti).
///
/// Hypokaliémie → faiblesse ventilatoire (direct) ; acidémie, hypoxie tissulaire et ischémie
/// coronaire → dépression de la contractilité ; acidémie → vasoplégie (boucle,
/// `f ← ½(f + f_cible)` jusqu'à `|f_cible − f| < 1e-3`). Sans couplage actif, une seule passe :
/// résultat identique à la passe simple.
///
/// Renvoie [`Error::NonViable`] si la pompe s'arrête (FC < `hr_arrest`), si la circulation
/// s'effondre (DC < `co_arrest_frac` × DC de référence), si la PAM est sous `map_arrest`
/// (choc irréversible), si VO2 dépasse DO2 (`er ≥ 1`), si
/// aucun état stationnaire n'existe ou si la spirale ne converge pas en 30 passes.
pub fn evaluate(p0: &Params) -> Result<Evaluation, Error> {
    // température centrale : FC intrinsèque, VO2, P50 (identité à 38,6 °C sans anesthésie)
    let p0 = &crate::periph::apply_temperature(p0);
    let co_ref = 3.29 * math::pow(p0.bw / 20.0, 0.75);
    let f_vent = f64::max(
        F_FLOOR,
        1.0 - p0.k_weak_vent * f64::max(0.0, p0.k_weak - p0.k),
    );
    let (mut fc, mut fr, mut fh) = (1.0, 1.0, 0.0);
    let mut last: Option<Evaluation> = None;
    for it in 0..DECOMP_MAX_ITER {
        let mut q = p0.clone();
        q.vent_drive = p0.vent_drive * f_vent;
        // réponse à l'hypoxémie (commande fh) : sympathique de base, inotropisme, vasodilatation
        q.s0 = (p0.s0 + p0.hypox_symp * fh).min(1.0);
        q.ees_lv = p0.ees_lv * fc * (1.0 + p0.hypox_inotropy * fh);
        q.ees_rv = p0.ees_rv * fc * (1.0 + p0.hypox_inotropy * fh);
        q.r_sys = p0.r_sys * fr * (1.0 - p0.hypox_vaso * fh).max(0.2);
        // rein → cœur : l'anémie (MRC) abaisse la viscosité, donc les résistances systémiques
        if p0.anemia_visc != 0.0 && p0.hb_ref > 0.0 {
            q.r_sys *= math::pow(p0.hb / p0.hb_ref, p0.anemia_visc);
        }
        let e = match evaluate_once(&q) {
            Ok(e) => e,
            Err(err) if is_physiological_failure(&err) => {
                return Err(Error::NonViable {
                    cause: "no_steady_state",
                    last: last.map(Box::new),
                })
            }
            Err(err) => return Err(err),
        };
        let dead = if e.params.hr < p0.hr_arrest {
            Some("cardiac_arrest")
        } else if e.hemo.summary.co < p0.co_arrest_frac * co_ref {
            Some("circulatory_collapse")
        } else if e.hemo.summary.map < p0.map_arrest {
            Some("irreversible_shock")
        } else if e.metabolic.er >= 1.0 {
            Some("oxygen_debt")
        } else {
            None
        };
        if let Some(cause) = dead {
            return Err(Error::NonViable {
                cause,
                last: Some(Box::new(e)),
            });
        }
        let (tc, tr, f_acid, f_hyp, f_cor) = coupling_targets(p0, &e);
        let th = hypoxemic_drive(p0, e.metabolic.art.pao2);
        if (tc - fc).abs() < DECOMP_TOL
            && (tr - fr).abs() < DECOMP_TOL
            && (th - fh).abs() < DECOMP_TOL
        {
            let mut e = finish(p0, e, fc, fr, f_vent, it + 1, f_acid, f_hyp, f_cor);
            if fh > 1e-3 {
                e.outcome.decompensated = true;
                e.outcome.mechanisms.push(Mechanism {
                    id: "hypoxemic_drive",
                    value: e.metabolic.art.pao2,
                    threshold: p0.hypox_pao2,
                    factor: fh,
                });
            }
            return Ok(e);
        }
        last = Some(e);
        fc = 0.5 * fc + 0.5 * tc;
        fr = 0.5 * fr + 0.5 * tr;
        fh = 0.5 * fh + 0.5 * th;
    }
    Err(Error::NonViable {
        cause: "vicious_circle",
        last: last.map(Box::new),
    })
}

#[allow(clippy::too_many_arguments)]
fn finish(
    p0: &Params,
    mut e: Evaluation,
    fc: f64,
    fr: f64,
    f_vent: f64,
    iterations: usize,
    f_acid: f64,
    f_hyp: f64,
    f_cor: f64,
) -> Evaluation {
    let hs = &e.hemo.summary;
    let mut mech = Vec::new();
    let mut push = |id, value, threshold, factor| {
        mech.push(Mechanism {
            id,
            value,
            threshold,
            factor,
        })
    };
    if f_acid < 1.0 {
        push(
            "acidemia_depression",
            e.metabolic.art.ph,
            p0.acid_ph0,
            f_acid,
        );
    }
    if f_hyp < 1.0 {
        push(
            "hypoxic_depression",
            e.metabolic.ven.po2,
            p0.hypox_pvo2,
            f_hyp,
        );
    }
    if f_cor < 1.0 {
        push("coronary_ischemia", hs.dap - hs.lv_edp, p0.cpp_min, f_cor);
    }
    if f_vent < 1.0 {
        push("hypokalemic_weakness", p0.k, p0.k_weak, f_vent);
    }
    if p0.k >= p0.k_atrial_standstill {
        push("atrial_standstill", p0.k, p0.k_atrial_standstill, 1.0);
    }
    if e.params.hr < 0.98 * e.sinus_hr {
        push(
            "conduction_block",
            e.params.hr,
            e.sinus_hr,
            e.params.hr / e.sinus_hr,
        );
    }
    if e.metabolic.er > ER_ANAEROBIC {
        push("anaerobic_metabolism", e.metabolic.er, ER_ANAEROBIC, 1.0);
    }
    if hs.la_mean > LA_EDEMA {
        push("pulmonary_edema", hs.la_mean, LA_EDEMA, 1.0);
    }
    if hs.map < MAP_RENAL {
        push("renal_hypoperfusion", hs.map, MAP_RENAL, 1.0);
    }
    if e.autonomic.s >= 1.0 {
        push("baroreflex_exhausted", e.autonomic.s, 1.0, 1.0);
    }
    e.decomp = Decompensation {
        f_contract: fc,
        f_rsys: fr,
        f_vent,
        iterations,
    };
    e.outcome = Outcome {
        decompensated: !mech.is_empty(),
        mechanisms: mech,
    };
    e
}
