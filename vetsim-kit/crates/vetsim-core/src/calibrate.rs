//! Banc de calibration / validation (miroir de `reference/python/calibrate.py`, jalon M5).
//!
//! - Cohorte saine : `n` chiens de 20 kg avec variabilité interindividuelle (graines 0..n),
//!   moyenne et écart-type de chaque variable comparés aux cibles de `calibration/targets.json`.
//! - PK : patient de 20 kg sans variabilité, Cmax, Tmax, ASC et demi-vie IV comparés aux
//!   valeurs publiées citées dans `calibrate.py`.
//! - Maladies : `n` cas par stade (graines `seed0 + i`, mêmes patients d'un stade à l'autre).
//!
//! Les lignes de VFC (section `hrv` des cibles) relèvent du modèle autonome (jalon M6).
//! Pas d'I/O : les cibles sont passées en texte JSON par l'appelant.

use crate::engine::{evaluate, Evaluation};
use crate::error::{Error, LoadError};
use crate::modules::Catalog;
use crate::patient::Patient;
use crate::pkpd::{timecourse, Dose, Route};
use crate::rng::SimRng;
use crate::Issue;
use serde_json::Value as Json;

/// Cible de la cohorte saine.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Bloc (Hémodynamique, Gaz du sang…).
    pub module: String,
    /// Variable (libellé de `targets.json`).
    pub var: String,
    /// Valeur cible.
    pub target: f64,
    /// Demi-largeur d'acceptation.
    pub tol: f64,
    /// Unité.
    pub unit: String,
    /// Source.
    pub source: String,
}

/// Lit la section `healthy` de `calibration/targets.json`.
pub fn parse_targets(json: &str, origin: &str) -> Result<Vec<Target>, LoadError> {
    let root: Json = serde_json::from_str(json).map_err(|source| LoadError::Json {
        origin: origin.into(),
        source,
    })?;
    let mut issues = Vec::new();
    let mut out = Vec::new();
    match root.get("healthy").and_then(Json::as_array) {
        None => issues.push(Issue::new("/healthy", "tableau de cibles attendu")),
        Some(rows) => {
            for (i, r) in rows.iter().enumerate() {
                let s = |k: &str| r.get(k).and_then(Json::as_str).map(str::to_string);
                let n = |k: &str| r.get(k).and_then(Json::as_f64);
                match (s("module"), s("var"), n("target"), n("tol")) {
                    (Some(module), Some(var), Some(target), Some(tol)) => {
                        if extractor(&var).is_none() {
                            issues.push(Issue::new(
                                format!("/healthy/{i}/var"),
                                format!("variable « {var} » sans extracteur connu"),
                            ));
                        }
                        out.push(Target {
                            module,
                            var,
                            target,
                            tol,
                            unit: s("unit").unwrap_or_default(),
                            source: s("source").unwrap_or_default(),
                        });
                    }
                    _ => issues.push(Issue::new(
                        format!("/healthy/{i}"),
                        "module, var, target et tol requis",
                    )),
                }
            }
        }
    }
    if issues.is_empty() {
        Ok(out)
    } else {
        Err(LoadError::Invalid {
            origin: origin.into(),
            issues,
        })
    }
}

type Extractor = fn(&Evaluation) -> f64;

/// Extracteur d'une variable de la cohorte saine (libellés de `calibrate.py`).
pub fn extractor(var: &str) -> Option<Extractor> {
    Some(match var {
        "FC" => |e| e.params.hr,
        "PAM" => |e| e.hemo.summary.map,
        "Débit cardiaque" => |e| e.hemo.summary.co,
        "PAP moy" => |e| e.hemo.summary.pap_mean,
        "POG (PAOP)" => |e| e.hemo.summary.la_mean,
        "pH artériel" => |e| e.metabolic.art.ph,
        "PaCO2" => |e| e.metabolic.art.pco2,
        "PaO2" => |e| e.metabolic.art.pao2,
        "pH veineux mêlé" => |e| e.metabolic.ven.ph,
        "PvCO2" => |e| e.metabolic.ven.pco2,
        "PvO2" => |e| e.metabolic.ven.po2,
        "Excès de base" => |e| e.metabolic.art.be,
        // P50 ramenée à pH 7,4 (correction de Bohr inverse)
        "P50 (pH 7,4)" => |e| e.metabolic.art.p50 * 10f64.powf(-0.37 * (7.4 - e.metabolic.art.ph)),
        "HCO3 art." => |e| e.metabolic.art.hco3,
        "Na" => |e| e.metabolic.chem.na,
        "K" => |e| e.metabolic.chem.k,
        "Cl" => |e| e.metabolic.chem.cl,
        "Albumine" => |e| e.metabolic.chem.alb,
        "Phosphate" => |e| e.metabolic.chem.phos,
        "Trou anionique" => |e| e.metabolic.chem.anion_gap,
        "VO2" => |e| e.params.vo2,
        _ => return None,
    })
}

/// Ligne de la cohorte saine.
#[derive(Debug, Clone, PartialEq)]
pub struct CohortRow {
    /// Cible.
    pub target: Target,
    /// Moyenne du modèle.
    pub model_mean: f64,
    /// Écart-type (population, ddof = 0 comme NumPy).
    pub model_sd: f64,
    /// |moyenne − cible| ≤ tol.
    pub ok: bool,
    /// Fraction des chiens à moins de 2 tol de la cible.
    pub frac_in: f64,
    /// Effectif.
    pub n: usize,
}

fn mean_sd(x: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    let v = x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / n;
    (m, v.sqrt())
}

/// Cohorte saine de `n` chiens de 20 kg (graines `0..n`).
pub fn healthy_cohort(
    cat: &Catalog,
    targets: &[Target],
    n: usize,
) -> Result<Vec<CohortRow>, Error> {
    let mut values: Vec<Vec<f64>> = vec![Vec::with_capacity(n); targets.len()];
    let ex: Vec<Extractor> = targets
        .iter()
        .map(|t| {
            extractor(&t.var).ok_or_else(|| Error::InvalidInput(format!("variable {}", t.var)))
        })
        .collect::<Result<_, _>>()?;
    for seed in 0..n as u64 {
        let mut rng = SimRng::new(seed);
        let pt = Patient::with_variability(&cat.patient, 20.0, &mut rng)?;
        let ev = evaluate(&pt.params)?;
        for (j, f) in ex.iter().enumerate() {
            values[j].push(f(&ev));
        }
    }
    Ok(targets
        .iter()
        .zip(values)
        .map(|(t, x)| {
            let (m, sd) = mean_sd(&x);
            let inside = x
                .iter()
                .filter(|v| (*v - t.target).abs() <= 2.0 * t.tol)
                .count();
            CohortRow {
                target: t.clone(),
                model_mean: m,
                model_sd: sd,
                ok: (m - t.target).abs() <= t.tol,
                frac_in: inside as f64 / x.len() as f64,
                n: x.len(),
            }
        })
        .collect())
}

/// Ligne PK : valeur du modèle face à une valeur publiée.
#[derive(Debug, Clone, PartialEq)]
pub struct PkRow {
    /// `PK <médicament>`.
    pub module: String,
    /// Indicateur et schéma (ex. `cmax (0.5 mg/kg PO)`).
    pub var: String,
    /// Valeur publiée.
    pub target: f64,
    /// Unité.
    pub unit: String,
    /// Source.
    pub source: String,
    /// Valeur du modèle.
    pub model: f64,
}

/// Indicateurs publiés utilisés par `calibrate.py::pk_checks` (valeurs recopiées de ce fichier).
/// (médicament, dose mg/kg PO, [(indicateur, cible, unité, source)])
/// (indicateur, cible, unité, source)
type PkRef = (&'static str, f64, &'static str, &'static str);

const PK_REFS: &[(&str, f64, &[PkRef])] = &[
    (
        "pimobendan",
        0.5,
        &[
            ("cmax", 49.1, "ng/mL", "Her 2020"),
            ("tmax", 2.1, "h", "Her 2020"),
            ("auc", 148.4, "ng.h/mL", "Her 2020"),
        ],
    ),
    (
        "furosemide",
        5.0,
        &[
            ("cmax", 2126.0, "ng/mL", "RCP Noah (5 mg/kg PO)"),
            ("tmax", 1.0, "h", "RCP Noah 0,8-1,1 h"),
        ],
    ),
    (
        "furosemide",
        1.9,
        &[("cmax", 900.0, "ng/mL", "RCP Noah (1,9 mg/kg PO)")],
    ),
];

/// `np.linspace(0, stop, num)`.
fn linspace(stop: f64, num: usize) -> Vec<f64> {
    let step = stop / (num - 1) as f64;
    (0..num).map(|i| i as f64 * step).collect()
}

/// Indicateurs PK du patient sain de 20 kg sans variabilité.
pub fn pk_checks(cat: &Catalog) -> Result<Vec<PkRow>, Error> {
    let p = Patient::healthy(&cat.patient, 20.0)?.params;
    let mut rows = Vec::new();
    let grid = linspace(24.0, 481);
    for (drug, dose, refs) in PK_REFS {
        let d = Dose {
            t_h: 0.0,
            drug: (*drug).into(),
            mg_per_kg: *dose,
            route: Route::Po,
        };
        let tc = timecourse(&p, &cat.drugs, &[d], 24.0, &grid)?;
        let c = tc.conc_of(drug).expect("médicament dosé");
        // Premier maximum (np.argmax).
        let mut i_max = 0;
        for (i, v) in c.iter().enumerate() {
            if *v > c[i_max] {
                i_max = i;
            }
        }
        let auc: f64 = (1..c.len())
            .map(|i| 0.5 * (c[i] + c[i - 1]) * (tc.t[i] - tc.t[i - 1]))
            .sum();
        for (k, target, unit, source) in refs.iter() {
            let model = match *k {
                "cmax" => c[i_max] * 1000.0,
                "tmax" => tc.t[i_max],
                _ => auc * 1000.0,
            };
            rows.push(PkRow {
                module: format!("PK {drug}"),
                var: format!("{k} ({dose:?} mg/kg PO)"),
                target: *target,
                unit: (*unit).into(),
                source: (*source).into(),
                model,
            });
        }
    }
    // Demi-vie IV du furosémide 2 mg/kg : pente de ln C sur ]2, 8[ h (BMC Vet Res 2021 : 1-1,5 h).
    let grid = linspace(12.0, 241);
    let d = Dose {
        t_h: 0.0,
        drug: "furosemide".into(),
        mg_per_kg: 2.0,
        route: Route::Iv,
    };
    let tc = timecourse(&p, &cat.drugs, &[d], 12.0, &grid)?;
    let c = tc.conc_of("furosemide").expect("dosé");
    let pts: Vec<(f64, f64)> =
        tc.t.iter()
            .zip(c)
            .filter(|(t, _)| **t > 2.0 && **t < 8.0)
            .map(|(t, c)| (*t, c.ln()))
            .collect();
    let n = pts.len() as f64;
    let (mt, my) = (
        pts.iter().map(|p| p.0).sum::<f64>() / n,
        pts.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let slope = pts.iter().map(|p| (p.0 - mt) * (p.1 - my)).sum::<f64>()
        / pts.iter().map(|p| (p.0 - mt).powi(2)).sum::<f64>();
    rows.push(PkRow {
        module: "PK furosemide".into(),
        var: "t1/2 IV".into(),
        target: 1.25,
        unit: "h".into(),
        source: "BMC Vet Res 2021 (1-1,5 h)".into(),
        model: std::f64::consts::LN_2 / -slope,
    });
    Ok(rows)
}

/// Variables relevées pour chaque cas de maladie (clés de `calibrate.py::_dis`).
pub const DISEASE_KEYS: &[&str] = &[
    "la", "rf", "map", "co", "creat", "k", "na", "hco3", "ph", "pao2", "hr", "urea",
];

fn disease_values(e: &Evaluation) -> Vec<f64> {
    let (h, m) = (&e.hemo.summary, &e.metabolic);
    vec![
        h.la_mean,
        h.reg_fraction,
        h.map,
        h.co,
        m.chem.creat,
        m.chem.k,
        m.chem.na,
        m.art.hco3,
        m.art.ph,
        m.art.pao2,
        e.params.hr,
        m.chem.urea,
    ]
}

/// Cas d'une maladie à un stade.
#[derive(Debug, Clone, PartialEq)]
pub struct DiseaseGroup {
    /// Identifiant du module.
    pub disease: String,
    /// Stade.
    pub stage: String,
    /// Une ligne par cas, valeurs dans l'ordre de [`DISEASE_KEYS`].
    pub samples: Vec<Vec<f64>>,
}

/// Cas par stade pour MMVD, MRC et Addison (`n` cas, graines `seed0 + i`).
pub fn disease_checks(cat: &Catalog, n: usize, seed0: u64) -> Result<Vec<DiseaseGroup>, Error> {
    let mut out = Vec::new();
    for id in ["mmvd", "ckd", "addison"] {
        let m = cat
            .disease(id)
            .ok_or_else(|| Error::UnknownDisease(id.into()))?;
        for st in &m.stages {
            let mut samples = Vec::with_capacity(n);
            for i in 0..n as u64 {
                let mut rng = SimRng::new(seed0 + i);
                let mut pt = Patient::with_variability(&cat.patient, 20.0, &mut rng)?;
                pt.add_disease_staged(m, Some(&st.name), &mut rng)?;
                samples.push(disease_values(&evaluate(&pt.params)?));
            }
            out.push(DiseaseGroup {
                disease: id.into(),
                stage: st.name.clone(),
                samples,
            });
        }
    }
    Ok(out)
}
