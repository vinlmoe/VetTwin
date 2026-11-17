//! Recherche de racine scalaire : port de `scipy.optimize.brentq`
//! (scipy/optimize/Zeros/brentq.c, Brent 1973, variante de Chandrupatla pour l'extrapolation).
//!
//! Même algorithme et mêmes valeurs par défaut que SciPy (`xtol = 2e-12`, `rtol = 4·ε`,
//! `maxiter = 100`) pour reproduire les itérés de la référence Python au plus près.

/// Tolérance absolue par défaut de `brentq`.
pub const XTOL: f64 = 2e-12;
/// Tolérance relative par défaut de `brentq` (4 × epsilon machine).
pub const RTOL: f64 = 4.0 * f64::EPSILON;
/// Nombre maximal d'itérations par défaut.
pub const MAXITER: usize = 100;

/// Échec de la recherche de racine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RootError {
    /// `f(a)` et `f(b)` sont de même signe (pas de changement de signe dans l'intervalle).
    NoSignChange {
        /// `f(a)`.
        fa: f64,
        /// `f(b)`.
        fb: f64,
    },
    /// Pas de convergence en `MAXITER` itérations.
    NoConvergence,
    /// La fonction a renvoyé une valeur non finie.
    NonFinite,
}

/// Racine de `f` dans `[a, b]` avec les tolérances par défaut de SciPy.
pub fn brentq(f: impl FnMut(f64) -> f64, a: f64, b: f64) -> Result<f64, RootError> {
    brentq_with(f, a, b, XTOL, RTOL, MAXITER)
}

/// Racine de `f` dans `[a, b]` (tolérances explicites).
pub fn brentq_with(
    mut f: impl FnMut(f64) -> f64,
    a: f64,
    b: f64,
    xtol: f64,
    rtol: f64,
    maxiter: usize,
) -> Result<f64, RootError> {
    let mut eval = |x: f64| {
        let y = f(x);
        if y.is_finite() {
            Ok(y)
        } else {
            Err(RootError::NonFinite)
        }
    };
    let (mut xpre, mut xcur) = (a, b);
    let (mut xblk, mut fblk, mut spre, mut scur) = (0.0, 0.0, 0.0, 0.0);
    let mut fpre = eval(xpre)?;
    let mut fcur = eval(xcur)?;
    if fpre == 0.0 {
        return Ok(xpre);
    }
    if fcur == 0.0 {
        return Ok(xcur);
    }
    if fpre.is_sign_negative() == fcur.is_sign_negative() {
        return Err(RootError::NoSignChange { fa: fpre, fb: fcur });
    }
    for _ in 0..maxiter {
        if fpre != 0.0 && fcur != 0.0 && fpre.is_sign_negative() != fcur.is_sign_negative() {
            xblk = xpre;
            fblk = fpre;
            spre = xcur - xpre;
            scur = spre;
        }
        if fblk.abs() < fcur.abs() {
            xpre = xcur;
            xcur = xblk;
            xblk = xpre;
            fpre = fcur;
            fcur = fblk;
            fblk = fpre;
        }
        let delta = (xtol + rtol * xcur.abs()) / 2.0;
        let sbis = (xblk - xcur) / 2.0;
        if fcur == 0.0 || sbis.abs() < delta {
            return Ok(xcur);
        }
        if spre.abs() > delta && fcur.abs() < fpre.abs() {
            let stry = if xpre == xblk {
                // interpolation linéaire
                -fcur * (xcur - xpre) / (fcur - fpre)
            } else {
                // extrapolation quadratique inverse
                let dpre = (fpre - fcur) / (xpre - xcur);
                let dblk = (fblk - fcur) / (xblk - xcur);
                -fcur * (fblk * dblk - fpre * dpre) / (dblk * dpre * (fblk - fpre))
            };
            if 2.0 * stry.abs() < spre.abs().min(3.0 * sbis.abs() - delta) {
                spre = scur;
                scur = stry;
            } else {
                spre = sbis;
                scur = sbis;
            }
        } else {
            spre = sbis;
            scur = sbis;
        }
        xpre = xcur;
        fpre = fcur;
        if scur.abs() > delta {
            xcur += scur;
        } else {
            xcur += if sbis > 0.0 { delta } else { -delta };
        }
        fcur = eval(xcur)?;
    }
    Err(RootError::NoConvergence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_roots() {
        let r = brentq(|x| x * x - 2.0, 0.0, 2.0).unwrap();
        assert!((r - 2f64.sqrt()).abs() < 1e-11);
        let r = brentq(|x| crate::math::cos(x) - x, 0.0, 1.0).unwrap();
        assert!((r - 0.739_085_133_215_160_6).abs() < 1e-11);
        assert_eq!(brentq(|x| x - 1.0, 1.0, 3.0), Ok(1.0));
    }

    #[test]
    fn reports_failures() {
        assert!(matches!(
            brentq(|x| x * x + 1.0, -1.0, 1.0),
            Err(RootError::NoSignChange { .. })
        ));
        assert_eq!(brentq(|_| f64::NAN, 0.0, 1.0), Err(RootError::NonFinite));
    }
}
