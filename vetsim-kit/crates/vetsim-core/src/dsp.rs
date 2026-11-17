//! Traitement du signal : filtres de Butterworth passe-bande (conception comme
//! `scipy.signal.butter`), interpolation linéaire, écart-type, codage WAV.

use crate::math;
use std::f64::consts::PI;

/// Nombre complexe minimal (conception des filtres).
#[derive(Debug, Clone, Copy, PartialEq)]
struct C {
    re: f64,
    im: f64,
}

impl C {
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }
    fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }
    fn mul(self, o: C) -> C {
        C::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
    fn scale(self, k: f64) -> C {
        C::new(self.re * k, self.im * k)
    }
    fn div(self, o: C) -> C {
        let d = o.re * o.re + o.im * o.im;
        C::new(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }
    /// Racine principale.
    fn sqrt(self) -> C {
        let r = math::hypot(self.re, self.im);
        let re = ((r + self.re) / 2.0).max(0.0).sqrt();
        let im = ((r - self.re) / 2.0).max(0.0).sqrt();
        C::new(re, if self.im < 0.0 { -im } else { im })
    }
}

/// Échec de conception d'un filtre.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FilterError {
    /// Bande invalide (0 < lo < hi < fs/2 requis) ou ordre nul.
    #[error("filtre invalide : ordre {order}, bande [{lo}, {hi}] Hz à fs = {fs} Hz")]
    Invalid {
        /// Ordre.
        order: usize,
        /// Fréquence basse.
        lo: f64,
        /// Fréquence haute.
        hi: f64,
        /// Fréquence d'échantillonnage.
        fs: f64,
    },
    /// Bande trop large : un pôle numérique réel apparaît (cas non pris en charge).
    #[error("bande [{0}, {1}] Hz trop large pour la mise en sections du second ordre")]
    RealPole(f64, f64),
}

/// Passe-bande de Butterworth d'ordre `order` sur `[lo, hi]` Hz à `fs` Hz, en sections du
/// second ordre `[b0, b1, b2, 1, a1, a2]` (même filtre que
/// `scipy.signal.butter(order, [lo, hi], 'band', fs=fs, output='sos')`).
///
/// Méthode de scipy : prototype analogique (pôles de Butterworth), pré-distorsion des
/// fréquences (`tan`), transformation passe-bas → passe-bande, transformation bilinéaire. Les
/// zéros numériques sont en +1 et −1 (N chacun) ; chaque section associe un zéro en +1, un zéro
/// en −1 et une paire de pôles conjugués. L'appariement diffère de `zpk2sos` mais le filtre
/// global est identique (vérifié contre scipy : réponse impulsionnelle à 1e-12 près).
pub fn butter_bandpass(
    order: usize,
    lo: f64,
    hi: f64,
    fs: f64,
) -> Result<Vec<[f64; 6]>, FilterError> {
    let invalid = || FilterError::Invalid { order, lo, hi, fs };
    if order == 0 || !(lo > 0.0 && lo < hi && hi < fs / 2.0) {
        return Err(invalid());
    }
    // Fréquences normalisées (Nyquist = 1) puis pré-distorsion, comme scipy (fs interne = 2).
    let fs2 = 2.0;
    let warp = |f: f64| 2.0 * fs2 * math::tan(PI * (2.0 * f / fs) / fs2);
    let (w1, w2) = (warp(lo), warp(hi));
    let bw = w2 - w1;
    let wo = (w1 * w2).sqrt();
    // Prototype analogique : p_k = −exp(iπ m / (2N)), m = −N+1, −N+3, …, N−1.
    let n = order as i64;
    let mut poles_bp = Vec::with_capacity(2 * order);
    for k in 0..order as i64 {
        let m = (-n + 1 + 2 * k) as f64;
        let ang = PI * m / (2.0 * n as f64);
        let p = C::new(-math::cos(ang), -math::sin(ang));
        // lp2bp : p·bw/2 ± √((p·bw/2)² − wo²)
        let pl = p.scale(bw / 2.0);
        let root = pl.mul(pl).sub(C::new(wo * wo, 0.0)).sqrt();
        poles_bp.push(pl.add(root));
        poles_bp.push(pl.sub(root));
    }
    // Gain analogique : k = bw^N (N zéros en 0 ajoutés par lp2bp).
    let k_bp = bw.powi_free(order);
    // Bilinéaire : z = (2fs + s)/(2fs − s), avec fs = 2 → 2fs = 4.
    let f4 = 2.0 * fs2;
    let zp: Vec<C> = poles_bp
        .iter()
        .map(|p| C::new(f4, 0.0).add(*p).div(C::new(f4, 0.0).sub(*p)))
        .collect();
    // k_z = k · Re( Π(4 − z_a) / Π(4 − p_a) ), zéros analogiques : N en 0.
    let mut num = C::new(1.0, 0.0);
    for _ in 0..order {
        num = num.mul(C::new(f4, 0.0));
    }
    let mut den = C::new(1.0, 0.0);
    for p in &poles_bp {
        den = den.mul(C::new(f4, 0.0).sub(*p));
    }
    let k_z = k_bp * num.div(den).re;
    // Paires de pôles conjugués (partie imaginaire > 0).
    let upper: Vec<C> = zp.iter().copied().filter(|p| p.im > 1e-12).collect();
    if upper.len() != order {
        return Err(FilterError::RealPole(lo, hi));
    }
    let mut sos = Vec::with_capacity(order);
    for (i, p) in upper.iter().enumerate() {
        let g = if i == 0 { k_z } else { 1.0 };
        sos.push([g, 0.0, -g, 1.0, -2.0 * p.re, p.re * p.re + p.im * p.im]);
    }
    Ok(sos)
}

trait PowiFree {
    fn powi_free(self, n: usize) -> f64;
}

impl PowiFree for f64 {
    /// Puissance entière par multiplications successives (déterministe).
    fn powi_free(self, n: usize) -> f64 {
        (0..n).fold(1.0, |acc, _| acc * self)
    }
}

/// `np.interp(x, xp, fp, right=right)` : linéaire, `fp[0]` à gauche, `right` au-delà de
/// `xp[n−1]` (`fp[n−1]` exactement en `xp[n−1]`).
pub fn interp_right(x: &[f64], xp: &[f64], fp: &[f64], right: f64) -> Vec<f64> {
    let n = xp.len();
    let mut j = 0;
    x.iter()
        .map(|&v| {
            if v > xp[n - 1] {
                return right;
            }
            if v == xp[n - 1] {
                return fp[n - 1];
            }
            if v <= xp[0] {
                return fp[0];
            }
            if v < xp[j] {
                j = 0;
            }
            while xp[j + 1] <= v {
                j += 1;
            }
            let slope = (fp[j + 1] - fp[j]) / (xp[j + 1] - xp[j]);
            slope * (v - xp[j]) + fp[j]
        })
        .collect()
}

/// Écart-type de population (ddof = 0, comme `np.std`).
pub fn std(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    (x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / n).sqrt()
}

/// WAV PCM 16 bits mono : échantillons dans [−1, 1] → `round(x · 32767)` (arrondi au pair le
/// plus proche, comme `np.round`).
pub fn wav_pcm16(samples: &[f64], fs: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&fs.to_le_bytes());
    out.extend_from_slice(&(fs * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round_ties_even() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Encodage base64 standard (RFC 4648, avec remplissage).
pub fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            A[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::{sosfilt, PCG_BANDPASS_SOS};

    #[test]
    fn design_matches_frozen_scipy_coefficients_response() {
        // Le filtre du PCG M6 (coefficients figés de scipy) et la conception générique doivent
        // avoir la même réponse impulsionnelle.
        let sos = butter_bandpass(3, 90.0, 450.0, 4000.0).unwrap();
        let mut imp = vec![0.0; 600];
        imp[0] = 1.0;
        let a = sosfilt(&sos, &imp);
        let b = sosfilt(&PCG_BANDPASS_SOS, &imp);
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            assert!((x - y).abs() < 1e-14, "h[{i}] {x} / {y}");
        }
    }

    #[test]
    fn invalid_bands_are_refused() {
        assert!(butter_bandpass(3, 250.0, 60.0, 4000.0).is_err());
        assert!(butter_bandpass(3, 60.0, 2500.0, 4000.0).is_err());
        assert!(butter_bandpass(0, 60.0, 250.0, 4000.0).is_err());
    }

    #[test]
    fn interp_right_matches_numpy_edges() {
        let xp = [0.0, 1.0, 2.0];
        let fp = [10.0, 20.0, 40.0];
        assert_eq!(
            interp_right(&[-1.0, 0.5, 2.0, 2.5], &xp, &fp, 0.0),
            vec![10.0, 15.0, 40.0, 0.0]
        );
    }

    #[test]
    fn base64_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn wav_header_and_rounding() {
        let w = wav_pcm16(&[0.0, 1.0, -1.0, 0.5], 4000);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(w.len(), 44 + 8);
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), 32767);
        assert_eq!(i16::from_le_bytes([w[48], w[49]]), -32767);
        assert_eq!(i16::from_le_bytes([w[50], w[51]]), 16384); // 16383,5 → pair
    }
}
