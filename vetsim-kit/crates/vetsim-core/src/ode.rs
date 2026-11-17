//! Intégrateur Dormand-Prince 5(4) à pas adaptatif (« RK45 »), avec sortie dense d'ordre 4.
//!
//! Choix (règle 6 du projet) : la référence Python utilise LSODA (scipy), sans équivalent direct
//! en Rust. On implémente Dormand-Prince avec le contrôle de pas, le choix du pas initial et
//! l'interpolant de `scipy.integrate.RK45` (Shampine 1986), mêmes tolérances que la référence.
//! - Limites : méthode explicite, donc inadaptée à un système raide ; le pas maximal de 4 ms
//!   et les constantes de temps du modèle (≥ ~5 ms) maintiennent la stabilité. Les écarts avec
//!   LSODA sont de l'ordre des tolérances (1e-6) sur la trajectoire.
//! - Pas d'allocation dans la boucle : l'état est un tableau de taille fixe `[f64; N]`.

/// Options de l'intégrateur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Tolérance relative.
    pub rtol: f64,
    /// Tolérance absolue.
    pub atol: f64,
    /// Pas maximal.
    pub max_step: f64,
    /// Nombre maximal de pas acceptés (garde-fou).
    pub max_steps: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            rtol: 1e-6,
            atol: 1e-6,
            max_step: f64::INFINITY,
            max_steps: 1_000_000,
        }
    }
}

/// Échec de l'intégration.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum OdeError {
    /// Le pas est devenu inférieur au pas minimal représentable.
    #[error("pas trop petit à t = {0}")]
    StepTooSmall(f64),
    /// L'état ou sa dérivée n'est plus fini.
    #[error("état non fini à t = {0}")]
    NonFinite(f64),
    /// Nombre maximal de pas atteint.
    #[error("nombre maximal de pas atteint à t = {0}")]
    TooManySteps(f64),
}

const C: [f64; 6] = [0.0, 1.0 / 5.0, 3.0 / 10.0, 4.0 / 5.0, 8.0 / 9.0, 1.0];
const A: [[f64; 5]; 6] = [
    [0.0, 0.0, 0.0, 0.0, 0.0],
    [1.0 / 5.0, 0.0, 0.0, 0.0, 0.0],
    [3.0 / 40.0, 9.0 / 40.0, 0.0, 0.0, 0.0],
    [44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0, 0.0, 0.0],
    [
        19372.0 / 6561.0,
        -25360.0 / 2187.0,
        64448.0 / 6561.0,
        -212.0 / 729.0,
        0.0,
    ],
    [
        9017.0 / 3168.0,
        -355.0 / 33.0,
        46732.0 / 5247.0,
        49.0 / 176.0,
        -5103.0 / 18656.0,
    ],
];
const B: [f64; 6] = [
    35.0 / 384.0,
    0.0,
    500.0 / 1113.0,
    125.0 / 192.0,
    -2187.0 / 6784.0,
    11.0 / 84.0,
];
const E: [f64; 7] = [
    -71.0 / 57600.0,
    0.0,
    71.0 / 16695.0,
    -71.0 / 1920.0,
    17253.0 / 339200.0,
    -22.0 / 525.0,
    1.0 / 40.0,
];
/// Coefficients de l'interpolant dense d'ordre 4 (scipy RK45.P).
const P: [[f64; 4]; 7] = [
    [
        1.0,
        -8048581381.0 / 2820520608.0,
        8663915743.0 / 2820520608.0,
        -12715105075.0 / 11282082432.0,
    ],
    [0.0, 0.0, 0.0, 0.0],
    [
        0.0,
        131558114200.0 / 32700410799.0,
        -68118460800.0 / 10900136933.0,
        87487479700.0 / 32700410799.0,
    ],
    [
        0.0,
        -1754552775.0 / 470086768.0,
        14199869525.0 / 1410260304.0,
        -10690763975.0 / 1880347072.0,
    ],
    [
        0.0,
        127303824393.0 / 49829197408.0,
        -318862633887.0 / 49829197408.0,
        701980252875.0 / 199316789632.0,
    ],
    [
        0.0,
        -282668133.0 / 205662961.0,
        2019193451.0 / 616988883.0,
        -1453857185.0 / 822651844.0,
    ],
    [
        0.0,
        40617522.0 / 29380423.0,
        -110615467.0 / 29380423.0,
        69997945.0 / 29380423.0,
    ],
];
const SAFETY: f64 = 0.9;
const MIN_FACTOR: f64 = 0.2;
const MAX_FACTOR: f64 = 10.0;
const ERROR_EXPONENT: f64 = -1.0 / 5.0;

/// Pas accepté, avec interpolation dense sur `[t_old, t]`.
#[derive(Debug, Clone)]
pub struct Step<const N: usize> {
    /// Début du pas.
    pub t_old: f64,
    /// Fin du pas.
    pub t: f64,
    /// État en début de pas.
    pub y_old: [f64; N],
    /// État en fin de pas.
    pub y: [f64; N],
    q: [[f64; 4]; N],
}

impl<const N: usize> Step<N> {
    /// État interpolé à `t` ∈ [t_old, t] (ordre 4).
    pub fn interpolate(&self, t: f64) -> [f64; N] {
        let h = self.t - self.t_old;
        let x = (t - self.t_old) / h;
        let pw = [x, x * x, x * x * x, x * x * x * x];
        let mut out = self.y_old;
        for (i, o) in out.iter_mut().enumerate() {
            let q = &self.q[i];
            *o += h * (q[0] * pw[0] + q[1] * pw[1] + q[2] * pw[2] + q[3] * pw[3]);
        }
        out
    }
}

fn rms<const N: usize>(v: &[f64; N]) -> f64 {
    (v.iter().map(|x| x * x).sum::<f64>() / N as f64).sqrt()
}

fn all_finite<const N: usize>(v: &[f64; N]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// Intègre `dy/dt = f(t, y)` de `t0` à `t_end` ; `on_step` reçoit chaque pas accepté.
/// Renvoie l'état final.
pub fn solve<const N: usize>(
    mut f: impl FnMut(f64, &[f64; N]) -> [f64; N],
    t0: f64,
    y0: [f64; N],
    t_end: f64,
    opts: &Options,
    mut on_step: impl FnMut(&Step<N>),
) -> Result<[f64; N], OdeError> {
    let (rtol, atol) = (opts.rtol, opts.atol);
    let mut t = t0;
    let mut y = y0;
    let mut fy = f(t, &y);
    if !all_finite(&y) || !all_finite(&fy) {
        return Err(OdeError::NonFinite(t));
    }
    // Pas initial (Hairer, Nørsett & Wanner, comme scipy select_initial_step).
    let mut h_abs = {
        let mut sy = [0.0; N];
        let mut sf = [0.0; N];
        for i in 0..N {
            let scale = atol + y[i].abs() * rtol;
            sy[i] = y[i] / scale;
            sf[i] = fy[i] / scale;
        }
        let (d0, d1) = (rms(&sy), rms(&sf));
        let h0 = if d0 < 1e-5 || d1 < 1e-5 {
            1e-6
        } else {
            0.01 * d0 / d1
        };
        let mut y1 = y;
        for i in 0..N {
            y1[i] += h0 * fy[i];
        }
        let f1 = f(t + h0, &y1);
        let mut sd = [0.0; N];
        for i in 0..N {
            sd[i] = (f1[i] - fy[i]) / (atol + y[i].abs() * rtol);
        }
        let d2 = rms(&sd) / h0;
        let h1 = if d1 <= 1e-15 && d2 <= 1e-15 {
            f64::max(1e-6, h0 * 1e-3)
        } else {
            (0.01 / d1.max(d2)).powf(1.0 / 5.0)
        };
        f64::min(100.0 * h0, h1)
    };
    let mut k = [[0.0; N]; 7];
    let mut steps = 0;
    while t < t_end {
        steps += 1;
        if steps > opts.max_steps {
            return Err(OdeError::TooManySteps(t));
        }
        let min_step = 10.0 * (next_up(t) - t).abs();
        h_abs = h_abs.min(opts.max_step).max(min_step);
        let mut rejected = false;
        loop {
            if h_abs < min_step {
                return Err(OdeError::StepTooSmall(t));
            }
            let mut t_new = t + h_abs;
            if t_new > t_end {
                t_new = t_end;
            }
            let h = t_new - t;
            h_abs = h.abs();
            // Étages de Runge-Kutta.
            k[0] = fy;
            for s in 1..6 {
                let mut ys = y;
                for (j, kj) in k.iter().enumerate().take(s) {
                    let a = A[s][j];
                    if a != 0.0 {
                        for i in 0..N {
                            ys[i] += h * a * kj[i];
                        }
                    }
                }
                k[s] = f(t + C[s] * h, &ys);
            }
            let mut y_new = y;
            for (s, ks) in k.iter().enumerate().take(6) {
                if B[s] != 0.0 {
                    for i in 0..N {
                        y_new[i] += h * B[s] * ks[i];
                    }
                }
            }
            let f_new = f(t_new, &y_new);
            k[6] = f_new;
            if !all_finite(&y_new) || !all_finite(&f_new) {
                // Pas trop grand : on réduit (comme une erreur d'estimation).
                h_abs *= MIN_FACTOR;
                rejected = true;
                continue;
            }
            let mut err = [0.0; N];
            for i in 0..N {
                let mut e = 0.0;
                for (s, ks) in k.iter().enumerate() {
                    e += ks[i] * E[s];
                }
                let scale = atol + y[i].abs().max(y_new[i].abs()) * rtol;
                err[i] = e * h / scale;
            }
            let error_norm = rms(&err);
            if error_norm < 1.0 {
                let mut factor = if error_norm == 0.0 {
                    MAX_FACTOR
                } else {
                    f64::min(MAX_FACTOR, SAFETY * error_norm.powf(ERROR_EXPONENT))
                };
                if rejected {
                    factor = factor.min(1.0);
                }
                let mut q = [[0.0; 4]; N];
                for (i, qi) in q.iter_mut().enumerate() {
                    for (s, ks) in k.iter().enumerate() {
                        for (m, qim) in qi.iter_mut().enumerate() {
                            *qim += ks[i] * P[s][m];
                        }
                    }
                }
                let step = Step {
                    t_old: t,
                    t: t_new,
                    y_old: y,
                    y: y_new,
                    q,
                };
                on_step(&step);
                t = t_new;
                y = y_new;
                fy = f_new;
                h_abs *= factor;
                break;
            }
            h_abs *= f64::max(MIN_FACTOR, SAFETY * error_norm.powf(ERROR_EXPONENT));
            rejected = true;
        }
    }
    Ok(y)
}

/// Plus petit flottant strictement supérieur à `x` (x fini).
fn next_up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x == 0.0 {
        return f64::from_bits(1);
    }
    let bits = x.to_bits();
    f64::from_bits(if x > 0.0 { bits + 1 } else { bits - 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_decay_and_dense_output() {
        let opts = Options {
            rtol: 1e-9,
            atol: 1e-12,
            ..Options::default()
        };
        let mut worst: f64 = 0.0;
        let y = solve(
            |_, y: &[f64; 1]| [-2.0 * y[0]],
            0.0,
            [1.0],
            3.0,
            &opts,
            |s| {
                let tm = 0.5 * (s.t_old + s.t);
                worst = worst.max((s.interpolate(tm)[0] - (-2.0 * tm).exp()).abs());
            },
        )
        .unwrap();
        assert!((y[0] - (-6.0f64).exp()).abs() < 1e-10);
        assert!(worst < 1e-8, "{worst}");
    }

    #[test]
    fn harmonic_oscillator_with_max_step() {
        let opts = Options {
            max_step: 0.004,
            ..Options::default()
        };
        let mut n = 0;
        let mut max_h: f64 = 0.0;
        let y = solve(
            |_, y: &[f64; 2]| [y[1], -y[0]],
            0.0,
            [1.0, 0.0],
            2.0 * std::f64::consts::PI,
            &opts,
            |s| {
                n += 1;
                max_h = max_h.max(s.t - s.t_old);
            },
        )
        .unwrap();
        assert!((y[0] - 1.0).abs() < 1e-5 && y[1].abs() < 1e-5);
        assert!(max_h <= 0.004 + 1e-15);
        assert!(n >= 1570);
    }
}
