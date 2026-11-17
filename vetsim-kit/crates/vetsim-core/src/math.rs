//! Fonctions mathématiques du noyau, toutes issues du crate `libm` (Rust pur).
//!
//! Objectif : sorties identiques au bit près sur toutes les plateformes (macOS, Linux,
//! Windows, Android, iOS, wasm32) ; la libm du système peut différer au dernier bit.
//! Les méthodes `f64::exp`, `ln`, `powf`, `powi`, `tanh`, `cos`, `sin` sont interdites dans le
//! dépôt par `clippy.toml` (`disallowed-methods`). `sqrt` reste celle de la bibliothèque
//! standard : elle est correctement arrondie (IEEE 754), donc identique partout.

/// Exponentielle.
#[inline]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// Logarithme népérien.
#[inline]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

/// Puissance `x^y`.
#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// `10^x`.
#[inline]
pub fn pow10(x: f64) -> f64 {
    libm::pow(10.0, x)
}

/// Tangente hyperbolique.
#[inline]
pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

/// Cosinus.
#[inline]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// Sinus.
#[inline]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}
