//! Générateur pseudo-aléatoire à graine explicite (règle 5 du projet).
//!
//! ChaCha8 (`rand_chacha`) : flux identique sur toutes les plateformes pour une même graine.
//! Les tirages continus n'utilisent que des opérations IEEE exactes et les fonctions du crate
//! `libm` (implémentation Rust pure, mêmes bits partout, y compris wasm32), et non la libm
//! du système, dont le dernier bit peut varier d'une plateforme à l'autre.
//!
//! Le flux de NumPy (PCG64) n'est pas reproduit : la comparaison avec la référence Python est
//! statistique (jalon M5).

use crate::case::DrawSource;
use crate::math;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Générateur du moteur.
#[derive(Debug, Clone)]
pub struct SimRng {
    inner: ChaCha8Rng,
    /// Second tirage normal de Box-Muller en attente.
    spare: Option<f64>,
}

impl SimRng {
    /// Générateur initialisé par une graine.
    pub fn new(seed: u64) -> Self {
        Self {
            inner: ChaCha8Rng::seed_from_u64(seed),
            spare: None,
        }
    }

    /// Flux indépendant `stream` pour une même graine (ChaCha « stream id ») : sert à séparer
    /// les usages (variabilité, autonome, ECG, PCG) sans corréler leurs tirages.
    pub fn for_stream(seed: u64, stream: u64) -> Self {
        let mut inner = ChaCha8Rng::seed_from_u64(seed);
        inner.set_stream(stream);
        Self { inner, spare: None }
    }

    /// Entier 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    /// Uniforme sur [0, 1) avec 53 bits de mantisse.
    pub fn uniform01(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniforme sur [lo, hi).
    pub fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform01()
    }

    /// Normale centrée réduite (Box-Muller, les deux valeurs sont utilisées).
    pub fn standard_normal(&mut self) -> f64 {
        if let Some(z) = self.spare.take() {
            return z;
        }
        // u1 ∈ (0, 1] pour éviter ln(0).
        let u1 = 1.0 - self.uniform01();
        let u2 = self.uniform01();
        let r = (-2.0 * math::ln(u1)).sqrt();
        let theta = 2.0 * std::f64::consts::PI * u2;
        self.spare = Some(r * math::sin(theta));
        r * math::cos(theta)
    }

    /// Normale de moyenne `mu` et d'écart-type `sd`.
    pub fn normal(&mut self, mu: f64, sd: f64) -> f64 {
        mu + sd * self.standard_normal()
    }

    /// Indice tiré selon des poids positifs (non normalisés).
    pub fn weighted_index(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut x = self.uniform01() * total;
        for (i, w) in weights.iter().enumerate() {
            if x < *w {
                return i;
            }
            x -= w;
        }
        // Arrondi : dernier poids non nul.
        weights.iter().rposition(|w| *w > 0.0).unwrap_or(0)
    }
}

/// Source de bruit des signaux (autonome, ECG, PCG).
pub trait NoiseSource {
    /// Uniforme sur [0, 1).
    fn uniform01(&mut self) -> f64;
    /// Normale centrée réduite.
    fn standard_normal(&mut self) -> f64;
}

impl NoiseSource for SimRng {
    fn uniform01(&mut self) -> f64 {
        SimRng::uniform01(self)
    }
    fn standard_normal(&mut self) -> f64 {
        SimRng::standard_normal(self)
    }
}

/// Rejoue une suite de nombres enregistrée (tests de conformité avec la référence Python :
/// on y injecte exactement les tirages de NumPy).
#[derive(Debug, Clone)]
pub struct Replay {
    values: Vec<f64>,
    pos: usize,
}

impl Replay {
    /// Source qui renvoie `values` dans l'ordre.
    pub fn new(values: Vec<f64>) -> Self {
        Self { values, pos: 0 }
    }

    /// Nombre de valeurs consommées.
    pub fn consumed(&self) -> usize {
        self.pos
    }

    fn next(&mut self) -> f64 {
        let v = *self
            .values
            .get(self.pos)
            .expect("Replay : suite enregistrée épuisée");
        self.pos += 1;
        v
    }
}

impl NoiseSource for Replay {
    fn uniform01(&mut self) -> f64 {
        self.next()
    }
    fn standard_normal(&mut self) -> f64 {
        self.next()
    }
}

impl DrawSource for SimRng {
    /// `exp(N(−σ²/2, σ²))`, de moyenne 1.
    fn lognormal_mean1(&mut self, sd: f64) -> f64 {
        if sd == 0.0 {
            return 1.0;
        }
        math::exp(self.normal(-0.5 * sd * sd, sd))
    }

    fn bernoulli(&mut self, p: f64) -> bool {
        self.uniform01() < p
    }

    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        SimRng::uniform(self, lo, hi)
    }

    fn weighted_index(&mut self, weights: &[f64]) -> usize {
        SimRng::weighted_index(self, weights)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = SimRng::new(42);
        let mut b = SimRng::new(42);
        let mut c = SimRng::new(43);
        let xa: Vec<f64> = (0..16).map(|_| a.standard_normal()).collect();
        let xb: Vec<f64> = (0..16).map(|_| b.standard_normal()).collect();
        let xc: Vec<f64> = (0..16).map(|_| c.standard_normal()).collect();
        assert_eq!(xa, xb);
        assert_ne!(xa, xc);
    }

    /// Instantané du flux : détecte un changement de générateur ou de plateforme.
    #[test]
    fn stream_snapshot() {
        let mut r = SimRng::new(2024);
        let u: Vec<u64> = (0..3).map(|_| r.next_u64()).collect();
        let mut r2 = SimRng::new(2024);
        let n: Vec<u64> = (0..3).map(|_| r2.standard_normal().to_bits()).collect();
        assert_eq!(u, SNAPSHOT_U64, "flux ChaCha8 modifié");
        assert_eq!(n, SNAPSHOT_NORMAL_BITS, "tirages normaux modifiés");
    }

    const SNAPSHOT_U64: [u64; 3] = [
        3080959604347521991,
        18123447844947586703,
        12649239169944512436,
    ];
    const SNAPSHOT_NORMAL_BITS: [u64; 3] = [
        4603587603424191804,
        13812823858247345306,
        4608414444819030760,
    ];

    #[test]
    fn streams_are_independent() {
        let mut a = SimRng::for_stream(5, 1);
        let mut b = SimRng::for_stream(5, 2);
        let mut c = SimRng::for_stream(5, 1);
        let xa: Vec<u64> = (0..4).map(|_| a.next_u64()).collect();
        assert_ne!(xa, (0..4).map(|_| b.next_u64()).collect::<Vec<_>>());
        assert_eq!(xa, (0..4).map(|_| c.next_u64()).collect::<Vec<_>>());
        assert_eq!(
            SimRng::for_stream(5, 0).next_u64(),
            SimRng::new(5).next_u64()
        );
    }

    #[test]
    fn moments_are_right() {
        let mut r = SimRng::new(7);
        let n = 200_000;
        let z: Vec<f64> = (0..n).map(|_| r.standard_normal()).collect();
        let mean = z.iter().sum::<f64>() / n as f64;
        let var = z.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n as f64;
        assert!(
            mean.abs() < 0.01 && (var - 1.0).abs() < 0.01,
            "{mean} {var}"
        );
        let l: f64 = (0..n).map(|_| r.lognormal_mean1(0.25)).sum::<f64>() / n as f64;
        assert!((l - 1.0).abs() < 0.003, "moyenne lognormale {l}");
        let u: Vec<f64> = (0..n).map(|_| r.uniform(0.3, 0.6)).collect();
        assert!(u.iter().all(|x| (0.3..0.6).contains(x)));
        let mut counts = [0usize; 3];
        for _ in 0..n {
            counts[r.weighted_index(&[0.3, 0.4, 0.3])] += 1;
        }
        assert!(
            (counts[1] as f64 / n as f64 - 0.4).abs() < 0.005,
            "{counts:?}"
        );
    }
}
