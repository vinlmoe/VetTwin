//! Échocardiographie virtuelle : géométrie des cavités tirée des volumes simulés.
//!
//! Rien n'est dessiné à la main : chaque dimension vient d'un volume du modèle hémodynamique.
//! - VG : obus (cylindre de hauteur L/2 et demi-ellipsoïde de hauteur L/2), `V = 5π/24·D²·L`,
//!   avec `L = L_ref·(V/V_ref)^λ` ; la paroi s'épaissit en systole parce que le volume
//!   myocardique reste constant (myocarde incompressible).
//! - OG et OD : **observateurs**. Le modèle confond l'OG avec les veines pulmonaires (et l'OD
//!   avec les veines systémiques) dans un compartiment à pression passive unique ; la part de
//!   l'oreillette est `V = s·v0 + s·c·P_passive`. Aucune rétroaction : l'hémodynamique, le
//!   golden et la calibration sont inchangés.
//! - VD : croissant autour de l'épicarde du VG, d'épaisseur calculée pour contenir `V_VD`.
//!
//! Les constantes viennent de `modules/echo.json` ([`EchoConfig`]) avec leur provenance.
//! Usage pédagogique uniquement.

use crate::error::{Issue, LoadError};
use crate::math;
use crate::params::Params;
use serde_json::Value as Json;
use std::f64::consts::PI;

/// Coefficients allométriques `Y = a·BW^b` (cm, kg).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Allometry {
    pub a: f64,
    pub b: f64,
}

impl Allometry {
    /// Valeur au poids `bw` (kg).
    pub fn at(&self, bw: f64) -> f64 {
        self.a * math::pow(bw, self.b)
    }
}

/// Oreillette observée : volume de base, compliance (au chien de 20 kg) et proportions de
/// l'ellipsoïde (transverse, crânio-caudal, dorso-ventral).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtriumConfig {
    pub v0_ml_20kg: f64,
    pub c_ml_per_mmhg_20kg: f64,
    pub axes: [f64; 3],
}

/// Constantes du Doppler (bloc `doppler` de `modules/echo.json`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DopplerConfig {
    pub mitral_open_deg: f64,
    pub tricuspid_open_deg: f64,
    pub area_floor_fraction: f64,
    pub pulmonic_to_aortic_ratio: f64,
    pub tricuspid_to_mitral_ratio: f64,
    pub jet_spread_rate: f64,
    pub jet_decay_b: f64,
}

/// Configuration du module (contenu de `modules/echo.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct EchoConfig {
    pub vref_ml_per_kg: f64,
    pub shape_ratio_ref: f64,
    pub long_exponent: f64,
    pub ivsd: Allometry,
    pub lvwd: Allometry,
    pub ao: Allometry,
    pub lvidd_exp: f64,
    pub la: AtriumConfig,
    pub ra: AtriumConfig,
    pub rv_coverage_deg: f64,
    pub rv_length_fraction: f64,
    pub rv_wall_ratio: f64,
    pub mitral_annulus_ratio: f64,
    pub leaflet_ratio: f64,
    pub open_ref_fraction: f64,
    /// Dilatation de l'anneau mitral avec le VG : diamètre ∝ (LVIDd / D_ref)^exposant.
    pub annulus_dilation_exponent: f64,
    /// Doppler (bloc `doppler`).
    pub doppler: DopplerConfig,
    /// Bloc `render` transmis tel quel au rendu (scène schématique, acoustique).
    pub render: Json,
}

struct Reader<'a> {
    root: &'a Json,
    issues: Vec<Issue>,
}

impl Reader<'_> {
    fn get(&self, path: &[&str]) -> Option<&Json> {
        path.iter().try_fold(self.root, |v, k| v.get(*k))
    }
    fn num(&mut self, path: &[&str], positive: bool) -> f64 {
        match self.get(path).and_then(Json::as_f64) {
            Some(x) if x.is_finite() && (!positive || x > 0.0) => x,
            _ => {
                self.issues.push(Issue::new(
                    format!("/{}", path.join("/")),
                    if positive {
                        "nombre > 0 attendu"
                    } else {
                        "nombre attendu"
                    },
                ));
                f64::NAN
            }
        }
    }
    fn allo(&mut self, path: &[&str]) -> Allometry {
        let v: Vec<f64> = self
            .get(path)
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_f64).collect())
            .unwrap_or_default();
        if v.len() == 2 && v[0] > 0.0 && v[1].is_finite() {
            Allometry { a: v[0], b: v[1] }
        } else {
            self.issues.push(Issue::new(
                format!("/{}", path.join("/")),
                "[a, b] attendu (Y = a·BW^b, a > 0)",
            ));
            Allometry {
                a: f64::NAN,
                b: f64::NAN,
            }
        }
    }
    fn atrium(&mut self, g: &str) -> AtriumConfig {
        let axes: Vec<f64> = self
            .get(&[g, "axes"])
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_f64).collect())
            .unwrap_or_default();
        let axes = if axes.len() == 3 && axes.iter().all(|x| *x > 0.0) {
            [axes[0], axes[1], axes[2]]
        } else {
            self.issues.push(Issue::new(
                format!("/{g}/axes"),
                "3 proportions > 0 attendues",
            ));
            [f64::NAN; 3]
        };
        AtriumConfig {
            v0_ml_20kg: self.num(&[g, "v0_ml_20kg"], false),
            c_ml_per_mmhg_20kg: self.num(&[g, "c_ml_per_mmhg_20kg"], true),
            axes,
        }
    }
}

impl EchoConfig {
    /// Charge et valide la configuration (provenance obligatoire dans chaque bloc).
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root: Json = serde_json::from_str(json).map_err(|source| LoadError::Json {
            origin: origin.into(),
            source,
        })?;
        let mut r = Reader {
            root: &root,
            issues: Vec::new(),
        };
        if root.get("schema_version").and_then(Json::as_str) != Some("1.0.0") {
            r.issues
                .push(Issue::new("/schema_version", "« 1.0.0 » attendu"));
        }
        if root.get("id").and_then(Json::as_str) != Some("echo") {
            r.issues.push(Issue::new("/id", "« echo » attendu"));
        }
        for g in [
            "lv",
            "wall",
            "aorta",
            "normalization",
            "la",
            "ra",
            "rv",
            "valves",
            "doppler",
            "render",
        ] {
            for k in ["source", "evidence", "calibre_sur"] {
                if r.get(&[g, k]).and_then(Json::as_str).is_none() {
                    r.issues.push(Issue::new(
                        format!("/{g}/{k}"),
                        "provenance requise (source, evidence, calibre_sur)",
                    ));
                }
            }
        }
        let cfg = EchoConfig {
            vref_ml_per_kg: r.num(&["lv", "vref_ml_per_kg"], true),
            shape_ratio_ref: r.num(&["lv", "shape_ratio_ref"], true),
            long_exponent: r.num(&["lv", "long_exponent"], false),
            ivsd: r.allo(&["wall", "ivsd"]),
            lvwd: r.allo(&["wall", "lvwd"]),
            ao: r.allo(&["aorta", "ao"]),
            lvidd_exp: r.num(&["normalization", "lvidd_exp"], true),
            la: r.atrium("la"),
            ra: r.atrium("ra"),
            rv_coverage_deg: r.num(&["rv", "coverage_deg"], true),
            rv_length_fraction: r.num(&["rv", "length_fraction"], true),
            rv_wall_ratio: r.num(&["rv", "wall_ratio"], true),
            mitral_annulus_ratio: r.num(&["valves", "mitral_annulus_ratio"], true),
            leaflet_ratio: r.num(&["valves", "leaflet_ratio"], true),
            open_ref_fraction: r.num(&["valves", "open_ref_fraction"], true),
            annulus_dilation_exponent: r.num(&["valves", "annulus_dilation_exponent"], false),
            doppler: DopplerConfig {
                mitral_open_deg: r.num(&["doppler", "mitral_open_deg"], true),
                tricuspid_open_deg: r.num(&["doppler", "tricuspid_open_deg"], true),
                area_floor_fraction: r.num(&["doppler", "area_floor_fraction"], true),
                pulmonic_to_aortic_ratio: r.num(&["doppler", "pulmonic_to_aortic_ratio"], true),
                tricuspid_to_mitral_ratio: r.num(&["doppler", "tricuspid_to_mitral_ratio"], true),
                jet_spread_rate: r.num(&["doppler", "jet_spread_rate"], true),
                jet_decay_b: r.num(&["doppler", "jet_decay_b"], true),
            },
            render: root.get("render").cloned().unwrap_or(Json::Null),
        };
        if !(0.0..1.0 / 3.0).contains(&cfg.long_exponent) {
            r.issues.push(Issue::new(
                "/lv/long_exponent",
                "entre 0 et 1/3 (part longitudinale du raccourcissement)",
            ));
        }
        if r.issues.is_empty() {
            Ok(cfg)
        } else {
            Err(LoadError::Invalid {
                origin: origin.into(),
                issues: r.issues,
            })
        }
    }
}

impl EchoConfig {
    /// Constantes de forme utiles au rendu (croissant du VD, proportions des oreillettes).
    pub fn geometry_json(&self) -> Json {
        serde_json::json!({
            "rv_coverage_deg": self.rv_coverage_deg,
            "rv_length_fraction": self.rv_length_fraction,
            "la_axes": self.la.axes,
            "mitral_open_deg": self.doppler.mitral_open_deg,
            "tricuspid_open_deg": self.doppler.tricuspid_open_deg,
            "jet_spread_rate": self.doppler.jet_spread_rate,
            "jet_decay_b": self.doppler.jet_decay_b,
            "ra_axes": self.ra.axes,
        })
    }
}

// ------------------------------------------------------------------ géométrie de l'obus

/// Volume d'un obus de diamètre `d` et de longueur `l` (mL si cm).
pub fn bullet_volume(d: f64, l: f64) -> f64 {
    5.0 * PI / 24.0 * d * d * l
}

/// Rayon de l'obus à la profondeur `z` depuis la base (0 au-delà de l'apex).
pub fn bullet_radius(d: f64, l: f64, z: f64) -> f64 {
    let h = l / 2.0;
    if z < 0.0 || z > l {
        0.0
    } else if z <= h {
        d / 2.0
    } else {
        let u = (z - h) / h;
        d / 2.0 * f64::sqrt((1.0 - u * u).max(0.0))
    }
}

/// Bissection sur `[lo, hi]` d'une fonction croissante (racine de `f = 0`).
fn bisect(mut lo: f64, mut hi: f64, f: impl Fn(f64) -> f64) -> f64 {
    for _ in 0..80 {
        let m = 0.5 * (lo + hi);
        if f(m) > 0.0 {
            hi = m;
        } else {
            lo = m;
        }
    }
    0.5 * (lo + hi)
}

/// Référence du VG d'un patient : volume, dimensions télédiastoliques de référence et
/// volume myocardique (conservé).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LvReference {
    pub v_ref: f64,
    pub d_ref: f64,
    pub l_ref: f64,
    pub w_ref: f64,
    pub v_myo: f64,
}

/// Géométrie instantanée du VG (cm) : diamètre et longueur de la cavité, épaisseur pariétale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LvGeometry {
    pub d: f64,
    pub l: f64,
    pub w: f64,
}

impl EchoConfig {
    /// Référence du VG au poids `bw`.
    pub fn lv_reference(&self, bw: f64) -> LvReference {
        let v_ref = self.vref_ml_per_kg * bw;
        let d_ref = math::cbrt(24.0 * v_ref / (5.0 * PI * self.shape_ratio_ref));
        let l_ref = self.shape_ratio_ref * d_ref;
        let w_ref = 0.5 * (self.ivsd.at(bw) + self.lvwd.at(bw));
        let v_myo = bullet_volume(d_ref + 2.0 * w_ref, l_ref + w_ref) - v_ref;
        LvReference {
            v_ref,
            d_ref,
            l_ref,
            w_ref,
            v_myo,
        }
    }

    /// Géométrie du VG pour le volume de cavité `v` (mL).
    pub fn lv_geometry(&self, rf: &LvReference, v: f64) -> LvGeometry {
        let v = v.max(1e-3);
        let l = rf.l_ref * math::pow(v / rf.v_ref, self.long_exponent);
        let d = f64::sqrt(24.0 * v / (5.0 * PI * l));
        let target = v + rf.v_myo;
        let w = bisect(0.0, 10.0 * rf.w_ref + d, |w| {
            bullet_volume(d + 2.0 * w, l + w) - target
        });
        LvGeometry { d, l, w }
    }

    /// Volume de l'OG (mL) pour le volume du compartiment OG + veines pulmonaires.
    pub fn la_volume(&self, p: &Params, v_pv: f64) -> f64 {
        let s = p.bw / 20.0;
        let pass = (v_pv - p.v0_pv) / p.c_pv;
        s * (self.la.v0_ml_20kg + self.la.c_ml_per_mmhg_20kg * pass)
    }

    /// Volume de l'OD (mL) pour le volume du compartiment OD + veines systémiques.
    pub fn ra_volume(&self, p: &Params, v_sv: f64) -> f64 {
        let s = p.bw / 20.0;
        let pass = (v_sv - p.v0_sv) / (p.c_sv * p.ven_cap);
        s * (self.ra.v0_ml_20kg + self.ra.c_ml_per_mmhg_20kg * pass)
    }

    /// Épaisseur du croissant du VD (cm) qui contient le volume `v_rv`, autour de l'épicarde
    /// du VG (diamètre `epi_d`, longueur `epi_l`).
    pub fn rv_thickness(&self, epi_d: f64, epi_l: f64, v_rv: f64) -> f64 {
        let frac = self.rv_coverage_deg / 180.0;
        let lr = self.rv_length_fraction * epi_l;
        let vol = |t: f64| {
            let n = 200;
            let h = (lr + t) / n as f64;
            let area = |z: f64| {
                let ro = bullet_radius(epi_d + 2.0 * t, lr + t, z);
                let ri = bullet_radius(epi_d, epi_l, z);
                PI * (ro * ro - ri * ri).max(0.0)
            };
            // Simpson
            let mut acc = area(0.0) + area(lr + t);
            for i in 1..n {
                acc += area(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
            }
            frac * acc * h / 3.0
        };
        bisect(0.0, 3.0 * epi_d, |t| vol(t) - v_rv.max(0.0))
    }
}

/// Axes (diamètres, cm) d'une ellipsoïde de volume `v` aux proportions `axes`.
pub fn ellipsoid_axes(axes: [f64; 3], v: f64) -> [f64; 3] {
    let k = math::cbrt(6.0 * v.max(0.0) / (PI * axes[0] * axes[1] * axes[2]));
    [k * axes[0], k * axes[1], k * axes[2]]
}

// ------------------------------------------------------------------ séries et mesures

/// Entrées : séries du modèle sur une même grille de pas `dt`.
#[derive(Debug, Clone, Copy)]
pub struct EchoInputs<'a> {
    pub v_lv: &'a [f64],
    pub v_rv: &'a [f64],
    pub v_pv: &'a [f64],
    pub v_sv: &'a [f64],
    pub q_mi: &'a [f64],
    pub q_ao: &'a [f64],
    pub q_tr: &'a [f64],
    pub q_pu: &'a [f64],
}

/// Séries géométriques (cm, mL ; ouvertures dans [0, 1]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EchoSeries {
    pub lv_d: Vec<f64>,
    pub lv_l: Vec<f64>,
    pub lv_w: Vec<f64>,
    pub rv_t: Vec<f64>,
    pub la_v: Vec<f64>,
    pub la_axes: Vec<[f64; 3]>,
    pub ra_v: Vec<f64>,
    pub ra_axes: Vec<[f64; 3]>,
    pub mi_open: Vec<f64>,
    pub ao_open: Vec<f64>,
    pub tr_open: Vec<f64>,
    pub pu_open: Vec<f64>,
}

/// Dimensions constantes du patient (cm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoStatic {
    pub ao_d: f64,
    pub mitral_annulus_d: f64,
    pub leaflet_l: f64,
    pub rv_wall: f64,
    pub lv: LvReference,
}

fn opening(q: &[f64], frac: f64) -> Vec<f64> {
    let qmax = q.iter().copied().fold(0.0, f64::max);
    if qmax <= 0.0 {
        return vec![0.0; q.len()];
    }
    let qr = frac * qmax;
    q.iter().map(|&x| (x / qr).clamp(0.0, 1.0)).collect()
}

/// Géométrie du patient au cours du temps.
pub fn echo_series(cfg: &EchoConfig, p: &Params, i: &EchoInputs) -> (EchoStatic, EchoSeries) {
    let rf = cfg.lv_reference(p.bw);
    let st = EchoStatic {
        ao_d: cfg.ao.at(p.bw),
        mitral_annulus_d: cfg.mitral_annulus_ratio * rf.d_ref,
        leaflet_l: cfg.leaflet_ratio * rf.d_ref,
        rv_wall: cfg.rv_wall_ratio * rf.w_ref,
        lv: rf,
    };
    let mut st = st;
    let mut s = EchoSeries::default();
    for k in 0..i.v_lv.len() {
        let g = cfg.lv_geometry(&rf, i.v_lv[k]);
        s.lv_d.push(g.d);
        s.lv_l.push(g.l);
        s.lv_w.push(g.w);
        s.rv_t
            .push(cfg.rv_thickness(g.d + 2.0 * g.w, g.l + g.w, i.v_rv[k]));
        let la = cfg.la_volume(p, i.v_pv[k]);
        s.la_v.push(la);
        s.la_axes.push(ellipsoid_axes(cfg.la.axes, la));
        let ra = cfg.ra_volume(p, i.v_sv[k]);
        s.ra_v.push(ra);
        s.ra_axes.push(ellipsoid_axes(cfg.ra.axes, ra));
    }
    // Anneau mitral : dilaté avec le VG (diamètre télédiastolique maximal de la fenêtre).
    let d_ed = s.lv_d.iter().copied().fold(0.0, f64::max);
    if d_ed > 0.0 {
        st.mitral_annulus_d *= math::pow(d_ed / rf.d_ref, cfg.annulus_dilation_exponent);
    }
    let f = cfg.open_ref_fraction;
    s.mi_open = opening(i.q_mi, f);
    s.ao_open = opening(i.q_ao, f);
    s.tr_open = opening(i.q_tr, f);
    s.pu_open = opening(i.q_pu, f);
    (st, s)
}

/// Mesures « vraies » (sur la géométrie du modèle, coupe parfaite), moyennées sur les
/// battements complets de la fenêtre.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EchoTruth {
    pub n_beats: usize,
    pub lvidd: f64,
    pub lvids: f64,
    pub fs: f64,
    pub ef: f64,
    pub wall_d: f64,
    pub wall_s: f64,
    pub lvidd_n: f64,
    pub ao: f64,
    /// Diamètre transverse de l'OG au premier instant après la fermeture aortique.
    pub la_d: f64,
    pub la_ao: f64,
    pub la_max: f64,
    pub la_min: f64,
    pub lv_mass_g: f64,
    /// Instants (s, grille d'entrée) des mesures de chaque battement : [TD, TS, fermeture Ao].
    pub instants: Vec<[f64; 3]>,
}

fn argext(v: &[f64], a: usize, b: usize, max: bool) -> usize {
    let mut k = a;
    for j in a..b.min(v.len()) {
        if (max && v[j] > v[k]) || (!max && v[j] < v[k]) {
            k = j;
        }
    }
    k
}

/// Mesures sur les battements commençant aux instants `beats` (début de l'activation
/// ventriculaire, s) ; `dt` : pas de la grille ; `lvidd_divisor` = BW^exposant (LVIDdN).
pub fn echo_truth(
    st: &EchoStatic,
    s: &EchoSeries,
    v_lv: &[f64],
    q_ao: &[f64],
    beats: &[f64],
    dt: f64,
    lvidd_divisor: f64,
) -> EchoTruth {
    let n = v_lv.len();
    let idx = |t: f64| ((t / dt).round().max(0.0) as usize).min(n.saturating_sub(1));
    let mut acc = EchoTruth::default();
    let (mut lvidd, mut lvids, mut ef, mut wd, mut ws, mut lad, mut lmax, mut lmin) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let mut nb = 0usize;
    for w in beats.windows(2) {
        let (a, b) = (idx(w[0]), idx(w[1]));
        if a == 0 || b + 1 >= n || b <= a + 4 {
            continue;
        }
        // TD : maximum du volume autour du début de l'activation ; TS : minimum ensuite.
        let ed = argext(
            v_lv,
            a.saturating_sub((0.08 / dt) as usize),
            a + (0.06 / dt) as usize + 1,
            true,
        );
        let es = argext(v_lv, ed, b, false);
        // fermeture aortique : premier instant après l'éjection où le débit redevient ≤ 0
        let qmax = q_ao[a..b].iter().copied().fold(0.0, f64::max);
        let mut ac = es;
        let mut ejecting = false;
        for (j, &q) in q_ao.iter().enumerate().take(b).skip(a) {
            if q > 0.1 * qmax {
                ejecting = true;
            } else if ejecting && q <= 0.0 {
                ac = j;
                break;
            }
        }
        lvidd += s.lv_d[ed];
        lvids += s.lv_d[es];
        ef += (v_lv[ed] - v_lv[es]) / v_lv[ed];
        wd += s.lv_w[ed];
        ws += s.lv_w[es];
        lad += s.la_axes[ac][0];
        lmax += s.la_v[a..b].iter().copied().fold(f64::MIN, f64::max);
        lmin += s.la_v[a..b].iter().copied().fold(f64::MAX, f64::min);
        acc.instants
            .push([ed as f64 * dt, es as f64 * dt, ac as f64 * dt]);
        nb += 1;
    }
    if nb == 0 {
        return acc;
    }
    let k = nb as f64;
    acc.n_beats = nb;
    acc.lvidd = lvidd / k;
    acc.lvids = lvids / k;
    acc.fs = 1.0 - acc.lvids / acc.lvidd;
    acc.ef = ef / k;
    acc.wall_d = wd / k;
    acc.wall_s = ws / k;
    acc.lvidd_n = acc.lvidd / lvidd_divisor;
    acc.ao = st.ao_d;
    acc.la_d = lad / k;
    acc.la_ao = acc.la_d / acc.ao;
    acc.la_max = lmax / k;
    acc.la_min = lmin / k;
    // masse VG : volume myocardique × 1,05 g/mL (densité du myocarde)
    acc.lv_mass_g = 1.05 * st.lv.v_myo;
    acc
}

// ------------------------------------------------------------------ Doppler

/// Pressions du modèle (mmHg) sur la grille des séries.
#[derive(Debug, Clone, Copy)]
pub struct Pressures<'a> {
    pub lv: &'a [f64],
    pub la: &'a [f64],
    pub rv: &'a [f64],
    pub ra: &'a [f64],
    pub ao: &'a [f64],
    pub pa: &'a [f64],
}

/// Séries Doppler (cm/s, cm²). Antérogrades par continuité, régurgitations par Bernoulli.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DopplerSeries {
    pub v_mi: Vec<f64>,
    pub v_ao: Vec<f64>,
    pub v_tr: Vec<f64>,
    pub v_pu: Vec<f64>,
    pub a_mi: Vec<f64>,
    pub a_tv: Vec<f64>,
    pub v_mr: Vec<f64>,
    pub v_trj: Vec<f64>,
    pub v_ar: Vec<f64>,
    pub v_pr: Vec<f64>,
    pub eroa_mr: Vec<f64>,
    pub eroa_tr: Vec<f64>,
}

/// Aires fixes (cm²) : anneaux aortique et pulmonaire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DopplerStatic {
    pub a_ao: f64,
    pub a_pu: f64,
    pub a_mi_open: f64,
    pub a_tv_open: f64,
}

/// Vitesse d'un jet (cm/s) par Bernoulli simplifiée : ΔP = 4v² (v en m/s, ΔP en mmHg).
pub fn bernoulli_cm_s(dp_mmhg: f64) -> f64 {
    100.0 * f64::sqrt(dp_mmhg.max(0.0) / 4.0)
}

/// Aire de l'orifice à l'extrémité de feuillets en entonnoir (rayon d'anneau `ra`, longueur
/// `lf`, ouverture `open` ∈ [0, 1] → angle `open^0,7 · max`), bornée à `floor` × l'aire ouverte.
pub fn tip_area(ra: f64, lf: f64, open: f64, max_deg: f64, floor: f64) -> (f64, f64) {
    let r = |a: f64| (ra - lf * math::cos(a.to_radians())).max(0.0);
    let sq = |x: f64| x * x;
    let full = PI * sq(r(max_deg));
    let a = PI * sq(r(math::pow(open.clamp(0.0, 1.0), 0.7) * max_deg));
    (a.max(floor * full), full)
}

/// Séries Doppler à partir des débits, des pressions et de la géométrie.
pub fn doppler_series(
    cfg: &EchoConfig,
    st: &EchoStatic,
    s: &EchoSeries,
    i: &EchoInputs,
    p: &Pressures,
) -> (DopplerStatic, DopplerSeries) {
    let dc = &cfg.doppler;
    let a_ao = PI * (st.ao_d / 2.0) * (st.ao_d / 2.0);
    let a_pu = a_ao * dc.pulmonic_to_aortic_ratio * dc.pulmonic_to_aortic_ratio;
    let ra_m = st.mitral_annulus_d / 2.0;
    let ra_t = ra_m * dc.tricuspid_to_mitral_ratio;
    let mut d = DopplerSeries::default();
    let mut full = (0.0, 0.0);
    // reflux de fermeture (volume de fermeture des valves) : pas un jet régurgitant ; seuil à
    // 3 % du débit antérograde maximal de la valve
    let fwd = |q: &[f64]| q.iter().copied().fold(0.0, f64::max);
    let (e_mi, e_ao, e_tr, e_pu) = (
        0.03 * fwd(i.q_mi),
        0.03 * fwd(i.q_ao),
        0.03 * fwd(i.q_tr),
        0.03 * fwd(i.q_pu),
    );
    for k in 0..i.v_lv.len() {
        let (am, fm) = tip_area(
            ra_m,
            st.leaflet_l,
            s.mi_open[k],
            dc.mitral_open_deg,
            dc.area_floor_fraction,
        );
        let (at, ft) = tip_area(
            ra_t,
            st.leaflet_l,
            s.tr_open[k],
            dc.tricuspid_open_deg,
            dc.area_floor_fraction,
        );
        full = (fm, ft);
        d.a_mi.push(am);
        d.a_tv.push(at);
        d.v_mi.push(i.q_mi[k].max(0.0) / am);
        d.v_tr.push(i.q_tr[k].max(0.0) / at);
        d.v_ao.push(i.q_ao[k].max(0.0) / a_ao);
        d.v_pu.push(i.q_pu[k].max(0.0) / a_pu);
        let jet = |q: f64, eps: f64, dp: f64| {
            if q < -eps.max(1.0) {
                bernoulli_cm_s(dp)
            } else {
                0.0
            }
        };
        let mr = jet(i.q_mi[k], e_mi, p.lv[k] - p.la[k]);
        let tr = jet(i.q_tr[k], e_tr, p.rv[k] - p.ra[k]);
        d.v_mr.push(mr);
        d.v_trj.push(tr);
        d.v_ar.push(jet(i.q_ao[k], e_ao, p.ao[k] - p.lv[k]));
        d.v_pr.push(jet(i.q_pu[k], e_pu, p.pa[k] - p.rv[k]));
        d.eroa_mr.push(if mr > 0.0 { -i.q_mi[k] / mr } else { 0.0 });
        d.eroa_tr.push(if tr > 0.0 { -i.q_tr[k] / tr } else { 0.0 });
    }
    (
        DopplerStatic {
            a_ao,
            a_pu,
            a_mi_open: full.0,
            a_tv_open: full.1,
        },
        d,
    )
}

/// Mesures Doppler « vraies », moyennées sur les battements complets (vitesses en cm/s).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DopplerTruth {
    pub n_beats: usize,
    pub e: f64,
    pub a: f64,
    pub e_a: f64,
    pub ao_peak: f64,
    pub ao_vti: f64,
    pub pu_peak: f64,
    pub mr_peak: f64,
    pub tr_peak: f64,
    pub eroa_mr: f64,
    /// PAP systolique estimée par la régurgitation tricuspide (4v² + POD moyenne) et vraie.
    pub pasp_est: f64,
    pub pasp_true: f64,
}

/// Mesures Doppler par battement (`beats` : débuts d'activation ventriculaire, s) ; l'onde A
/// est le pic du flux mitral pendant l'activation atriale `act_a`, l'onde E le pic hors de
/// celle-ci dans la même diastole.
pub fn doppler_truth(
    d: &DopplerSeries,
    act_a: &[f64],
    p: &Pressures,
    beats: &[f64],
    dt: f64,
) -> DopplerTruth {
    let n = d.v_ao.len();
    let idx = |t: f64| ((t / dt).round().max(0.0) as usize).min(n.saturating_sub(1));
    let mut tr = DopplerTruth::default();
    let mx = |v: &[f64], a: usize, b: usize| v[a..b].iter().copied().fold(0.0, f64::max);
    let mut acc = [0.0f64; 10];
    let mut ea_beats = 0usize;
    let mut nb = 0usize;
    for w in beats.windows(2) {
        let (a, b) = (idx(w[0]), idx(w[1]));
        if a == 0 || b + 1 >= n || b <= a + 4 {
            continue;
        }
        let (mut e, mut aa) = (0.0f64, 0.0f64);
        for k in a..b {
            if act_a.get(k).copied().unwrap_or(0.0) > 0.05 {
                aa = aa.max(d.v_mi[k]);
            } else if d.v_ao[k] <= 0.0 {
                e = e.max(d.v_mi[k]);
            }
        }
        // E mesurée à chaque battement ; A absente en fibrillation atriale (rapport E/A alors 0)
        acc[0] += e;
        if aa > 0.0 {
            acc[1] += aa;
            ea_beats += 1;
        }
        acc[2] += mx(&d.v_ao, a, b);
        acc[3] += d.v_ao[a..b].iter().sum::<f64>() * dt;
        acc[4] += mx(&d.v_pu, a, b);
        acc[5] += mx(&d.v_mr, a, b);
        acc[6] += mx(&d.v_trj, a, b);
        acc[7] += mx(&d.eroa_mr, a, b);
        acc[8] += p.ra[a..b].iter().sum::<f64>() / (b - a) as f64;
        acc[9] += mx(p.pa, a, b);
        nb += 1;
    }
    if nb == 0 {
        return tr;
    }
    let k = nb as f64;
    tr.n_beats = nb;
    tr.e = acc[0] / k;
    if ea_beats > 0 {
        tr.a = acc[1] / ea_beats as f64;
        tr.e_a = tr.e / tr.a;
    }
    tr.ao_peak = acc[2] / k;
    tr.ao_vti = acc[3] / k;
    tr.pu_peak = acc[4] / k;
    tr.mr_peak = acc[5] / k;
    tr.tr_peak = acc[6] / k;
    tr.eroa_mr = acc[7] / k;
    if tr.tr_peak > 0.0 {
        tr.pasp_est = 4.0 * (tr.tr_peak / 100.0) * (tr.tr_peak / 100.0) + acc[8] / k;
    }
    tr.pasp_true = acc[9] / k;
    tr
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> EchoConfig {
        let src = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/echo.json"),
        )
        .unwrap();
        EchoConfig::from_json(&src, "echo.json").unwrap()
    }

    #[test]
    fn bullet_inverts_exactly() {
        let c = cfg();
        let rf = c.lv_reference(20.0);
        assert!((bullet_volume(rf.d_ref, rf.l_ref) - rf.v_ref).abs() < 1e-9);
        for v in [10.0, 30.0, 68.4, 150.0] {
            let g = c.lv_geometry(&rf, v);
            assert!((bullet_volume(g.d, g.l) - v).abs() < 1e-9 * v);
            // volume myocardique conservé
            let vm = bullet_volume(g.d + 2.0 * g.w, g.l + g.w) - v;
            assert!((vm - rf.v_myo).abs() < 1e-6 * rf.v_myo, "{vm} {}", rf.v_myo);
        }
    }

    #[test]
    fn wall_thickens_when_cavity_empties() {
        let c = cfg();
        let rf = c.lv_reference(20.0);
        let ed = c.lv_geometry(&rf, 68.0);
        let es = c.lv_geometry(&rf, 31.0);
        assert!(es.w > 1.3 * ed.w, "épaississement {} → {}", ed.w, es.w);
        assert!(es.d < ed.d && es.l < ed.l);
        // raccourcissement radial supérieur au longitudinal
        assert!(1.0 - es.d / ed.d > 1.0 - es.l / ed.l);
        // Visser 2019, fig. 1 (chien sain, grand axe 4 cavités) : volume 62,0 → 31,9 mL et grand
        // axe 6,59 → 5,50 cm entre télédiastole et télésystole.
        let (a, b) = (c.lv_geometry(&rf, 62.0), c.lv_geometry(&rf, 31.9));
        assert!(
            (b.l / a.l - 5.50 / 6.59).abs() < 0.01,
            "grand axe {}",
            b.l / a.l
        );
    }

    #[test]
    fn dilation_makes_ventricle_more_spherical() {
        let c = cfg();
        let rf = c.lv_reference(20.0);
        let n = c.lv_geometry(&rf, 68.0);
        let big = c.lv_geometry(&rf, 140.0);
        assert!(big.l / big.d < n.l / n.d);
    }

    #[test]
    fn rv_crescent_contains_volume() {
        let c = cfg();
        let t1 = c.rv_thickness(5.0, 7.0, 30.0);
        let t2 = c.rv_thickness(5.0, 7.0, 60.0);
        assert!(t1 > 0.0 && t2 > t1);
    }

    #[test]
    fn bernoulli_and_tip_area() {
        // 4 m/s ⇔ 64 mmHg
        assert!((bernoulli_cm_s(64.0) - 400.0).abs() < 1e-9);
        let (a0, full) = tip_area(1.5, 1.6, 0.0, 75.0, 0.25);
        let (a1, _) = tip_area(1.5, 1.6, 1.0, 75.0, 0.25);
        assert!((a0 - 0.25 * full).abs() < 1e-12 && (a1 - full).abs() < 1e-12 && full > 0.0);
    }

    #[test]
    fn ellipsoid_volume() {
        let a = ellipsoid_axes([1.0, 0.85, 0.9], 20.0);
        assert!((PI / 6.0 * a[0] * a[1] * a[2] - 20.0).abs() < 1e-9);
    }

    #[test]
    fn config_requires_provenance() {
        let src = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/echo.json"),
        )
        .unwrap();
        let mut j: Json = serde_json::from_str(&src).unwrap();
        j["la"].as_object_mut().unwrap().remove("evidence");
        assert!(EchoConfig::from_json(&j.to_string(), "x").is_err());
    }
}
