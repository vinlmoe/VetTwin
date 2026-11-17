//! Couche de conduction : génération et propagation de l'influx, battement par battement
//! (miroir de `reference/python/rhythm.py`, qui fait foi).
//!
//! Simulation par événements ; chaque trouble du rythme naît d'un mécanisme :
//! - nœud sinusal : décharges à la période sinusale, bloc de sortie (`sa_block`) ;
//! - oreillettes : activées par le nœud sinusal, ou en fibrillation (`af`) par une suite
//!   d'activations rapides et irrégulières sans contraction organisée ; inexcitables au-delà de
//!   `k_atrial_standstill` (arrêt atrial) avec conduction sino-ventriculaire conservée ;
//! - nœud AV : période réfractaire, délai dépendant de la récupération, conduction cachée ;
//!   allongés par le tonus vagal et par K⁺ ;
//! - His-Purkinje : conduction probabiliste, en échec au-delà de `k_hp_block` ;
//! - échappements jonctionnel et ventriculaire ; foyer ectopique ventriculaire (Poisson),
//!   accru en hypokaliémie ; réfractarité ventriculaire = QT.
//!
//! L'aléa ne passe que par des tirages uniformes `u ∈ [0, 1)` fournis par l'appelant : ils sont
//! rejouables à l'identique depuis la référence Python.

use crate::math;
use crate::params::Params;

const SQRT3: f64 = 1.732_050_807_568_877_2;

/// Générateur SplitMix64 (Steele, Lea, Flood 2014), identique à `rhythm.SplitMix64` de la
/// référence : l'aléa de conduction du moteur est le même au bit près en Python et en Rust.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Générateur initialisé par `seed`.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Entier 64 bits suivant.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniforme sur [0, 1) : `(x >> 11)·2⁻⁵³`.
    pub fn random(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

/// Activation atriale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtrialEvent {
    /// Instant (s).
    pub t: f64,
    /// Contraction organisée (onde P) ; `false` en fibrillation.
    pub organized: bool,
}

/// Origine d'une activation ventriculaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Influx supraventriculaire conduit par le nœud AV (sinusal ou fibrillation).
    Sinus,
    /// Échappement jonctionnel (QRS fin).
    Junctional,
    /// Échappement ventriculaire (QRS large).
    VentricularEscape,
    /// Battement ectopique ventriculaire (QRS large).
    Ectopic,
}

impl Origin {
    /// Identifiant (identique à la référence Python).
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Sinus => "sinus",
            Origin::Junctional => "junctional",
            Origin::VentricularEscape => "ventricular_escape",
            Origin::Ectopic => "ectopic",
        }
    }
}

/// Activation ventriculaire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VentricularEvent {
    /// Instant (s).
    pub t: f64,
    /// Origine.
    pub origin: Origin,
    /// Instant de l'influx supraventriculaire conduit (s), si conduit.
    pub t_a: Option<f64>,
    /// Délai oreillette → ventricule (s), si conduit.
    pub pr: Option<f64>,
    /// QRS large.
    pub wide: bool,
}

/// Séquence d'événements de conduction.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conduction {
    /// Activations atriales.
    pub atrial: Vec<AtrialEvent>,
    /// Activations ventriculaires.
    pub ventricular: Vec<VentricularEvent>,
}

/// Délai nodal minimal `AH0` et période réfractaire nodale `ERP` (s), modulés par le vagal et K⁺.
pub fn av_constants(p: &Params, v_eff: f64) -> (f64, f64) {
    let vag = 1.0 + p.av_vagal * (v_eff - 0.6);
    let kf = 1.0 + p.av_k_gain * f64::max(0.0, p.k - 5.5);
    let ah0 = f64::max(0.0, p.pr - p.hv) * vag * kf;
    let erp = p.av_erp * vag * kf;
    (ah0, erp)
}

/// Condition nécessaire (paramètres) d'un rythme sinusal régulier : aucun processus aléatoire
/// ni arrêt atrial.
pub fn may_be_trivial(p: &Params) -> bool {
    p.af == 0.0
        && p.sa_block == 0.0
        && p.vpc_rate == 0.0
        && p.hp_conduction >= 1.0
        && p.k < p.k_atrial_standstill
        && p.k < p.k_hp_block
        && p.k >= p.k_vpc
}

/// Vrai si la conduction produit un rythme sinusal conduit 1:1 à PR stabilisé (40 battements,
/// 10 derniers) : l'hémodynamique garde alors la simulation périodique.
pub fn is_trivial(p: &Params, v_eff: f64, sinus_period: f64) -> bool {
    const N: usize = 40;
    if !may_be_trivial(p) {
        return false;
    }
    let ev = simulate_conduction(
        p,
        v_eff,
        sinus_period,
        N as f64 * sinus_period,
        &mut || 0.0,
        None,
    );
    let v = &ev.ventricular;
    if v.len() < N || v.iter().any(|b| b.origin != Origin::Sinus) {
        return false;
    }
    let prs: Vec<f64> = v[v.len() - 10..].iter().filter_map(|b| b.pr).collect();
    let (lo, hi) = prs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
            (a.min(x), b.max(x))
        });
    hi - lo < 1e-6
}

/// PR (s) du rythme sinusal régulier stabilisé.
pub fn steady_pr(p: &Params, v_eff: f64, sinus_period: f64) -> Option<f64> {
    let ev = simulate_conduction(
        p,
        v_eff,
        sinus_period,
        40.0 * sinus_period,
        &mut || 0.0,
        None,
    );
    ev.ventricular.last().and_then(|b| b.pr)
}

/// Événements de conduction sur `[0, t_end]`.
///
/// `sinus_period` : période sinusale (s), ignorée si `sinus_times` est fourni ; `rand` : tirages
/// uniformes dans `[0, 1)`, consommés dans le même ordre que la référence Python.
pub fn simulate_conduction(
    p: &Params,
    v_eff: f64,
    sinus_period: f64,
    t_end: f64,
    rand: &mut dyn FnMut() -> f64,
    sinus_times: Option<&[f64]>,
) -> Conduction {
    let (ah0, erp) = av_constants(p, v_eff);
    let k = p.k;
    let standstill = k >= p.k_atrial_standstill;
    let hp_p = p.hp_conduction
        * if k < p.k_hp_block {
            1.0
        } else {
            f64::max(0.0, 1.0 - p.hp_k_slope * (k - p.k_hp_block))
        };
    let vpc_rate = p.vpc_rate + p.vpc_k_gain * f64::max(0.0, p.k_vpc - k);
    let vrp = p.qt;
    let kesc = f64::max(0.0, 1.0 - p.esc_k_gain * f64::max(0.0, k - 5.5));
    let cycle = |rate: f64| {
        if rate * kesc > 0.0 {
            60.0 / (rate * kesc)
        } else {
            f64::INFINITY
        }
    };
    let junct_cycle = cycle(p.junct_rate);
    let vesc_cycle = cycle(p.vent_escape_rate);
    // le foyer jonctionnel est au-dessus d'un bloc infranodal complet
    let junct_ok = junct_cycle.is_finite() && hp_p > 0.0;

    let mut out = Conduction::default();
    // --- impulsions supraventriculaires arrivant au nœud AV
    let mut supra = Vec::new();
    if p.af >= 0.5 {
        let mut t = rand() * p.af_cycle;
        while t < t_end {
            out.atrial.push(AtrialEvent {
                t,
                organized: false,
            });
            supra.push(t);
            t += f64::max(
                0.02,
                p.af_cycle * (1.0 + p.af_cv * SQRT3 * (2.0 * rand() - 1.0)),
            );
        }
    } else {
        let owned: Vec<f64>;
        let times: &[f64] = match sinus_times {
            Some(s) => s,
            None => {
                let n = (t_end / sinus_period) as usize + 1;
                owned = (0..n).map(|i| i as f64 * sinus_period).collect();
                &owned
            }
        };
        for &t in times {
            if t > t_end {
                break;
            }
            if p.sa_block > 0.0 && rand() < p.sa_block {
                continue; // bloc de sortie sino-atrial
            }
            if !standstill {
                out.atrial.push(AtrialEvent { t, organized: true });
            }
            supra.push(t); // sino-ventriculaire si arrêt atrial
        }
    }
    // --- décharges ectopiques (Poisson)
    let mut ect = Vec::new();
    if vpc_rate > 0.0 {
        let lam = vpc_rate / 60.0;
        let mut t = -math::ln(1.0 - rand()) / lam;
        while t < t_end {
            ect.push(t);
            t += -math::ln(1.0 - rand()) / lam;
        }
    }

    // --- propagation chronologique
    let mut last_v = f64::NEG_INFINITY;
    let mut last_his = f64::NEG_INFINITY;
    let (mut i_s, mut i_e) = (0usize, 0usize);
    let mut pending: Option<(f64, f64, f64)> = None;
    loop {
        // prochain événement : plus petit (t, type), comme `min` sur des tuples en Python
        let mut best: Option<(f64, u8)> = None;
        let mut offer = |t: f64, kind: u8| {
            let better = match best {
                None => true,
                Some((bt, bk)) => t < bt || (t == bt && kind < bk),
            };
            if better {
                best = Some((t, kind));
            }
        };
        if i_s < supra.len() {
            offer(supra[i_s], 0);
        }
        if i_e < ect.len() {
            offer(ect[i_e], 1);
        }
        if let Some((tv, _, _)) = pending {
            offer(tv, 2);
        }
        let base = if last_v.is_finite() { last_v } else { 0.0 };
        if junct_ok {
            offer(base + junct_cycle, 3);
        }
        if vesc_cycle.is_finite() {
            offer(base + vesc_cycle, 4);
        }
        let Some((t, kind)) = best else {
            break; // asystolie
        };
        if t > t_end {
            break;
        }
        match kind {
            0 => {
                i_s += 1;
                let rec = t - last_his;
                if rec < erp || pending.is_some() {
                    // conduction cachée en réfractarité relative
                    if pending.is_none() && rec >= 0.5 * erp {
                        last_his += p.av_conceal * rec;
                    }
                    continue;
                }
                let ah = ah0 + p.av_beta * math::exp(-(rec - erp) / p.av_tau);
                last_his = t + ah;
                if rand() >= hp_p {
                    continue; // bloc infranodal
                }
                pending = Some((t + ah + p.hv, t, ah + p.hv));
            }
            2 => {
                let (tv, ta, pr) = pending.take().expect("influx en cours");
                if tv - last_v < vrp {
                    continue;
                }
                out.ventricular.push(VentricularEvent {
                    t: tv,
                    origin: Origin::Sinus,
                    t_a: Some(ta),
                    pr: Some(pr),
                    wide: false,
                });
                last_v = tv;
            }
            1 => {
                i_e += 1;
                if t - last_v < vrp {
                    continue;
                }
                out.ventricular.push(VentricularEvent {
                    t,
                    origin: Origin::Ectopic,
                    t_a: None,
                    pr: None,
                    wide: true,
                });
                last_v = t;
                last_his = t; // conduction rétrograde : nœud AV réfractaire
            }
            3 => {
                out.ventricular.push(VentricularEvent {
                    t,
                    origin: Origin::Junctional,
                    t_a: None,
                    pr: None,
                    wide: false,
                });
                last_v = t;
                last_his = t;
            }
            _ => {
                out.ventricular.push(VentricularEvent {
                    t,
                    origin: Origin::VentricularEscape,
                    t_a: None,
                    pr: None,
                    wide: true,
                });
                last_v = t;
            }
        }
    }
    out
}

/// Indicateurs sur `[t0, t1]`.
#[derive(Debug, Clone, PartialEq)]
pub struct RhythmSummary {
    /// FC ventriculaire (/min).
    pub hr_v: f64,
    /// Fréquence des activations atriales (/min).
    pub hr_a: f64,
    /// Coefficient de variation des RR.
    pub cv_rr: f64,
    /// Plus long RR (s).
    pub max_rr: f64,
    /// Nombre d'activations ventriculaires.
    pub n: usize,
    /// Nombre d'activations par origine.
    pub origins: Vec<(Origin, usize)>,
}

/// Résumé d'une séquence sur `[t0, t1]` (comme `rhythm.summarize`).
pub fn summarize(ev: &Conduction, t0: f64, t1: f64) -> RhythmSummary {
    let v: Vec<&VentricularEvent> = ev
        .ventricular
        .iter()
        .filter(|b| t0 <= b.t && b.t <= t1)
        .collect();
    let na = ev.atrial.iter().filter(|a| t0 <= a.t && a.t <= t1).count();
    let rr: Vec<f64> = v.windows(2).map(|w| w[1].t - w[0].t).collect();
    let (hr_v, cv_rr, max_rr) = if rr.is_empty() {
        (0.0, f64::NAN, f64::NAN)
    } else {
        let mean = rr.iter().sum::<f64>() / rr.len() as f64;
        let var = rr.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / rr.len() as f64;
        (
            60.0 / mean,
            var.sqrt() / mean,
            rr.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    };
    let mut origins: Vec<(Origin, usize)> = Vec::new();
    for b in &v {
        match origins.iter_mut().find(|(o, _)| *o == b.origin) {
            Some((_, c)) => *c += 1,
            None => origins.push((b.origin, 1)),
        }
    }
    RhythmSummary {
        hr_v,
        hr_a: na as f64 / (t1 - t0) * 60.0,
        cv_rr,
        max_rr,
        n: v.len(),
        origins,
    }
}

#[cfg(test)]
mod tests {
    use super::SplitMix64;

    /// Vecteurs de référence de SplitMix64 (graine 0), identiques à la référence Python.
    #[test]
    fn splitmix64_reference_vectors() {
        let mut g = SplitMix64::new(0);
        assert_eq!(g.next_u64(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(g.next_u64(), 0x6e78_9e6a_a1b9_65f4);
        assert_eq!(g.next_u64(), 0x06c4_5d18_8009_454f);
    }
}
