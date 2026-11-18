//! Cinétique des marqueurs de filtration (créatinine, urée) au cours d'une évolution temporelle.
//!
//! L'évaluation stationnaire (`engine::metabolic_state`) donne la concentration d'équilibre
//! `C_ss = C0 / ratio_DFG` : elle suppose que le marqueur a eu le temps de s'accumuler. C'est
//! juste en maladie chronique (MRC), faux en aigu : la créatinine ne double pas en quelques
//! minutes quand le DFG chute. Ici, le marqueur est un compartiment unique de volume `V`
//! (eau corporelle), avec une production constante `P` et une élimination par filtration :
//!
//! `V·dC/dt = P − DFG(t)·C`, soit `dC/dt = (DFG(t)/V)·(C_ss(t) − C)` puisque `P = DFG·C_ss`.
//!
//! Entre deux instants d'évaluation, `DFG/V` et `C_ss` sont pris constants (moyenne des
//! extrémités) et l'équation est résolue exactement (exponentielle). Usage pédagogique uniquement.

/// Concentrations d'un marqueur aux instants `t_h` (h, croissants).
///
/// - `c0` : concentration à `t_h[0]` (état avant l'administration, en équilibre) ;
/// - `gfr_ml_min[i]` : DFG à l'instant `i` (mL/min) ;
/// - `c_ss[i]` : concentration d'équilibre correspondant au DFG de l'instant `i` ;
/// - `vd_l` : volume de distribution du marqueur (L).
///
/// Un instant sans valeur (patient décédé : `None`) arrête la série ; les instants suivants
/// valent `None`.
pub fn lagged_marker(
    t_h: &[f64],
    c0: f64,
    gfr_ml_min: &[Option<f64>],
    c_ss: &[Option<f64>],
    vd_l: f64,
) -> Vec<Option<f64>> {
    let n = t_h.len();
    let mut out = vec![None; n];
    if n == 0 || vd_l <= 0.0 {
        return out;
    }
    let mut c = c0;
    let mut prev: Option<(f64, f64, f64)> = None; // (t, k, c_ss) de l'instant précédent
    for i in 0..n {
        let (Some(g), Some(css)) = (gfr_ml_min[i], c_ss[i]) else {
            break;
        };
        let k = g * 0.06 / vd_l; // (mL/min → L/h) / L = 1/h
        if let Some((t_prev, k_prev, css_prev)) = prev {
            let dt = (t_h[i] - t_prev).max(0.0);
            let k_m = 0.5 * (k + k_prev);
            let css_m = 0.5 * (css + css_prev);
            c = css_m + (c - css_m) * crate::math::exp(-k_m * dt);
        }
        out[i] = Some(c);
        prev = Some((t_h[i], k, css));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_state_is_preserved() {
        let t = [0.0, 1.0, 5.0, 24.0];
        let g = [Some(70.0); 4];
        let css = [Some(75.0); 4];
        for c in lagged_marker(&t, 75.0, &g, &css, 12.0) {
            assert!((c.unwrap() - 75.0).abs() < 1e-12);
        }
    }

    #[test]
    fn acute_gfr_drop_rises_slowly_towards_new_equilibrium() {
        // DFG divisé par deux à t = 0 : C_ss double, mais C ne monte qu'avec t½ = ln2·V/DFG.
        let t = [0.0, 0.0833, 2.0, 200.0];
        let g = [Some(35.0); 4];
        let css = [Some(150.0); 4];
        let c = lagged_marker(&t, 75.0, &g, &css, 12.0);
        assert_eq!(c[0], Some(75.0));
        let c5 = c[1].unwrap();
        assert!(c5 > 75.0 && c5 < 77.0, "{c5}");
        let k: f64 = 35.0 * 0.06 / 12.0;
        let expected_2h = 150.0 - 75.0 * crate::math::exp(-k * 2.0);
        assert!((c[2].unwrap() - expected_2h).abs() < 1e-9);
        assert!((c[3].unwrap() - 150.0).abs() < 1e-6);
    }

    #[test]
    fn death_stops_the_series() {
        let t = [0.0, 1.0, 2.0];
        let c = lagged_marker(
            &t,
            75.0,
            &[Some(70.0), None, Some(70.0)],
            &[Some(75.0); 3],
            12.0,
        );
        assert_eq!(c, vec![Some(75.0), None, None]);
    }
}
