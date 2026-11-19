//! Variabilité intra-individuelle au cours du temps : trajectoires Monte-Carlo d'un même animal.
//!
//! Un animal est généré une fois (patient sain avec sa variabilité interindividuelle, puis ses
//! maladies). Une **trajectoire** est une réalisation de son évolution aux instants demandés,
//! avec quatre sources d'aléa (`modules/variation.json`, amplitudes placeholders, preuve D) :
//!
//! 1. **fluctuation naturelle** : chaque paramètre listé suit, sur l'échelle logarithmique, un
//!    processus d'Ornstein-Uhlenbeck stationnaire `dx = −x/τ dt + √(2/τ) dW` (x réduit) ; le
//!    paramètre vaut `p̄·exp(cv·x − cv²/2)` (multiplicateur de moyenne 1). Discrétisation exacte
//!    entre deux instants : `x' = x·e^(−Δt/τ) + √(1 − e^(−2Δt/τ))·z` ;
//! 2. **maladie** : la sévérité de chaque cas évolue, `s(t) = s0 + r·t/24 + sd·x(t)` borné à
//!    [0, 1] (r : progression par jour, x : Ornstein-Uhlenbeck réduit) ; les effets sont
//!    réévalués à chaque instant avec les **mêmes tirages** du cas (même animal, même maladie) ;
//! 3. **médicament** : clairance, volume central et EC50 thérapeutique de chaque médicament sont
//!    multipliés par un lognormal de moyenne 1 tiré une fois par trajectoire ;
//! 4. **mesure** : imprécision analytique, bruit multiplicatif indépendant à chaque prélèvement
//!    ([`VariationConfig::measure`]).
//!
//! Ordre d'application à l'instant t : base de l'animal × fluctuation naturelle → effets des
//! maladies à s(t) → effets des médicaments (C(t), D(t), U(t)). La PK de la trajectoire utilise
//! les paramètres de l'animal à t = 0 (fonction rénale et hépatique de départ).
//!
//! Déterminisme : toute trajectoire dépend seulement de la graine du générateur fourni.
//! Usage pédagogique uniquement.

use crate::case::{apply_modifiers, DrawSource};
use crate::error::{Error, Issue, LoadError};
use crate::math;
use crate::modules::{DiseaseModule, DrugModule};
use crate::params::Params;
use crate::patient::Patient;
use crate::pkpd::{apply_drug_effects, timecourse, Dose};
use crate::rng::SimRng;
use serde_json::Value as Json;

/// Fluctuation naturelle d'un paramètre (Ornstein-Uhlenbeck sur son logarithme).
#[derive(Debug, Clone, PartialEq)]
pub struct OuTerm {
    /// Paramètre du modèle.
    pub param: &'static str,
    /// Écart-type stationnaire de ln(paramètre) (≈ CV intra-individuel).
    pub cv: f64,
    /// Temps de corrélation (h).
    pub tau_h: f64,
}

/// Configuration de la variabilité intra-individuelle (`modules/variation.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct VariationConfig {
    /// Fluctuation naturelle, dans l'ordre du module.
    pub natural: Vec<OuTerm>,
    /// Écart-type de la fluctuation de sévérité.
    pub severity_sd: f64,
    /// Temps de corrélation de la fluctuation de sévérité (h).
    pub severity_tau_h: f64,
    /// Progression de la sévérité par jour, par maladie (0 si absente).
    pub progression_per_day: Vec<(String, f64)>,
    /// CV de la clairance des médicaments (par trajectoire).
    pub drug_cl_cv: f64,
    /// CV du volume central.
    pub drug_vc_cv: f64,
    /// CV de l'EC50 thérapeutique.
    pub drug_ec50_cv: f64,
    /// Imprécision analytique par sortie (clé plate du contrat `eval`).
    pub measurement: Vec<(String, f64)>,
}

/// Facteurs d'échelle des quatre sources (0 : source coupée, 1 : amplitudes du module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    /// Fluctuation naturelle.
    pub natural: f64,
    /// Fluctuation de sévérité (la progression n'est pas mise à l'échelle).
    pub disease: f64,
    /// Variabilité PK/PD.
    pub drug: f64,
    /// Bruit de mesure.
    pub measurement: f64,
}

impl Default for Scale {
    fn default() -> Self {
        Self {
            natural: 1.0,
            disease: 1.0,
            drug: 1.0,
            measurement: 1.0,
        }
    }
}

fn num(root: &Json, path: &[&str], issues: &mut Vec<Issue>, ok: impl Fn(f64) -> bool) -> f64 {
    let mut v = root;
    for k in path {
        match v.get(*k) {
            Some(x) => v = x,
            None => {
                issues.push(Issue::new(format!("/{}", path.join("/")), "nombre requis"));
                return 0.0;
            }
        }
    }
    match v.as_f64() {
        Some(x) if x.is_finite() && ok(x) => x,
        _ => {
            issues.push(Issue::new(
                format!("/{}", path.join("/")),
                "nombre fini dans les bornes attendu",
            ));
            0.0
        }
    }
}

impl VariationConfig {
    /// Charge et valide la configuration (chemins JSON dans les messages d'erreur).
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root: Json = serde_json::from_str(json).map_err(|source| LoadError::Json {
            origin: origin.into(),
            source,
        })?;
        let mut issues = Vec::new();
        if root.get("schema_version").and_then(Json::as_str) != Some("1.0.0") {
            issues.push(Issue::new("/schema_version", "« 1.0.0 » attendu"));
        }
        if root.get("id").and_then(Json::as_str) != Some("variation") {
            issues.push(Issue::new("/id", "« variation » attendu"));
        }
        for g in ["natural", "disease", "drug", "measurement"] {
            for k in ["source", "evidence"] {
                if root
                    .get(g)
                    .and_then(|x| x.get(k))
                    .and_then(Json::as_str)
                    .is_none()
                {
                    issues.push(Issue::new(
                        format!("/{g}/{k}"),
                        "provenance requise (source, evidence)",
                    ));
                }
            }
        }
        let mut natural = Vec::new();
        match root
            .get("natural")
            .and_then(|n| n.get("params"))
            .and_then(Json::as_object)
        {
            Some(ps) => {
                for (name, _) in ps {
                    let Some(param) = Params::NAMES.iter().copied().find(|n| n == name) else {
                        issues.push(Issue::new(
                            format!("/natural/params/{name}"),
                            "paramètre du modèle inconnu",
                        ));
                        continue;
                    };
                    let cv = num(
                        &root,
                        &["natural", "params", name, "cv"],
                        &mut issues,
                        |x| (0.0..=1.0).contains(&x),
                    );
                    let tau_h = num(
                        &root,
                        &["natural", "params", name, "tau_h"],
                        &mut issues,
                        |x| x > 0.0,
                    );
                    natural.push(OuTerm { param, cv, tau_h });
                }
            }
            None => issues.push(Issue::new("/natural/params", "objet requis")),
        }
        let severity_sd = num(&root, &["disease", "severity_sd"], &mut issues, |x| {
            (0.0..=1.0).contains(&x)
        });
        let severity_tau_h = num(&root, &["disease", "tau_h"], &mut issues, |x| x > 0.0);
        let progression_per_day = rate_map(
            root.get("disease")
                .and_then(|d| d.get("progression_per_day")),
            "/disease/progression_per_day",
            &mut issues,
        );
        let cv = |k: &str, issues: &mut Vec<Issue>| {
            num(&root, &["drug", k], issues, |x| (0.0..=2.0).contains(&x))
        };
        let drug_cl_cv = cv("cl_cv", &mut issues);
        let drug_vc_cv = cv("vc_cv", &mut issues);
        let drug_ec50_cv = cv("ec50_cv", &mut issues);
        let mut measurement = Vec::new();
        match root
            .get("measurement")
            .and_then(|m| m.get("outputs"))
            .and_then(Json::as_object)
        {
            Some(o) => {
                for (k, v) in o {
                    match v.as_f64() {
                        Some(x) if (0.0..=1.0).contains(&x) => measurement.push((k.clone(), x)),
                        _ => issues.push(Issue::new(
                            format!("/measurement/outputs/{k}"),
                            "CV entre 0 et 1 attendu",
                        )),
                    }
                }
            }
            None => issues.push(Issue::new("/measurement/outputs", "objet requis")),
        }
        if !issues.is_empty() {
            return Err(LoadError::Invalid {
                origin: origin.into(),
                issues,
            });
        }
        Ok(Self {
            natural,
            severity_sd,
            severity_tau_h,
            progression_per_day,
            drug_cl_cv,
            drug_vc_cv,
            drug_ec50_cv,
            measurement,
        })
    }

    /// Configuration dont les amplitudes sont multipliées par `scale`.
    pub fn scaled(&self, scale: Scale) -> Self {
        let mut c = self.clone();
        for t in &mut c.natural {
            t.cv *= scale.natural;
        }
        c.severity_sd *= scale.disease;
        c.drug_cl_cv *= scale.drug;
        c.drug_vc_cv *= scale.drug;
        c.drug_ec50_cv *= scale.drug;
        for (_, cv) in &mut c.measurement {
            *cv *= scale.measurement;
        }
        c
    }

    /// Remplace (ou ajoute) des vitesses de progression par maladie.
    pub fn with_progression(mut self, rates: &[(String, f64)]) -> Self {
        for (id, r) in rates {
            match self.progression_per_day.iter_mut().find(|(d, _)| d == id) {
                Some(slot) => slot.1 = *r,
                None => self.progression_per_day.push((id.clone(), *r)),
            }
        }
        self
    }

    fn progression(&self, disease: &str) -> f64 {
        self.progression_per_day
            .iter()
            .find(|(d, _)| d == disease)
            .map_or(0.0, |(_, r)| *r)
    }

    /// Valeur mesurée de la sortie `key` : bruit multiplicatif lognormal de moyenne 1
    /// (inchangée si la sortie n'a pas d'imprécision déclarée).
    pub fn measure(&self, key: &str, value: f64, rng: &mut SimRng) -> f64 {
        match self.measurement.iter().find(|(k, _)| k == key) {
            Some((_, cv)) if *cv > 0.0 => value * rng.lognormal_mean1(*cv),
            _ => value,
        }
    }
}

fn rate_map(v: Option<&Json>, path: &str, issues: &mut Vec<Issue>) -> Vec<(String, f64)> {
    let Some(o) = v.and_then(Json::as_object) else {
        issues.push(Issue::new(path, "objet requis"));
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, x) in o {
        match x.as_f64().filter(|x| x.is_finite()) {
            Some(r) => out.push((k.clone(), r)),
            None => issues.push(Issue::new(format!("{path}/{k}"), "nombre attendu")),
        }
    }
    out
}

/// Processus d'Ornstein-Uhlenbeck réduit (moyenne 0, variance stationnaire 1).
#[derive(Debug, Clone, Copy)]
struct Ou {
    x: f64,
    tau_h: f64,
}

impl Ou {
    /// Départ dans la loi stationnaire.
    fn new(tau_h: f64, rng: &mut SimRng) -> Self {
        Self {
            x: rng.standard_normal(),
            tau_h,
        }
    }

    /// Avance de `dt` heures (discrétisation exacte).
    fn step(&mut self, dt: f64, rng: &mut SimRng) {
        if dt <= 0.0 {
            return;
        }
        let a = math::exp(-dt / self.tau_h);
        self.x = a * self.x + (1.0 - a * a).max(0.0).sqrt() * rng.standard_normal();
    }
}

/// Rejoue les tirages enregistrés d'un cas (même animal, même maladie à une autre sévérité).
struct CaseReplay<'a> {
    values: &'a [(String, f64)],
    pos: usize,
}

impl CaseReplay<'_> {
    fn next(&mut self) -> f64 {
        let v = self.values.get(self.pos).map_or(0.0, |(_, x)| *x);
        self.pos += 1;
        v
    }
}

impl DrawSource for CaseReplay<'_> {
    fn lognormal_mean1(&mut self, _sd: f64) -> f64 {
        self.next()
    }
    fn bernoulli(&mut self, _p: f64) -> bool {
        self.next() != 0.0
    }
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        0.5 * (lo + hi)
    }
    fn weighted_index(&mut self, _weights: &[f64]) -> usize {
        0
    }
}

/// Une réalisation de l'évolution de l'animal.
#[derive(Debug, Clone, PartialEq)]
pub struct Trajectory {
    /// Instants (h).
    pub t: Vec<f64>,
    /// Paramètres d'entrée du moteur à chaque instant (maladies et médicaments appliqués).
    pub params: Vec<Params>,
    /// Paramètres de l'animal à t = 0 avant médicament (référence des marqueurs rénaux).
    pub start: Params,
    /// Sévérité de chaque cas (ordre de `patient.cases`) à chaque instant.
    pub severity: Vec<Vec<f64>>,
    /// Concentrations (mg/L) par médicament.
    pub conc: Vec<(String, Vec<f64>)>,
    /// Multiplicateurs PK/PD tirés (médicament, clairance, volume central, EC50).
    pub drug_factors: Vec<(String, f64, f64, f64)>,
}

/// Tire une trajectoire de `patient` aux instants `t_h` (croissants, ≥ 0).
///
/// `diseases` : modules des cas du patient, dans l'ordre de `patient.cases`.
pub fn sample_trajectory(
    patient: &Patient,
    diseases: &[&DiseaseModule],
    drugs: &[DrugModule],
    doses: &[Dose],
    t_h: &[f64],
    cfg: &VariationConfig,
    rng: &mut SimRng,
) -> Result<Trajectory, Error> {
    if diseases.len() != patient.cases.len() {
        return Err(Error::InvalidInput(
            "un module par cas de maladie attendu".into(),
        ));
    }
    if t_h.is_empty() || t_h.iter().any(|t| !(t.is_finite() && *t >= 0.0)) {
        return Err(Error::InvalidInput("instants ≥ 0 requis".into()));
    }
    if t_h.windows(2).any(|w| w[1] < w[0]) {
        return Err(Error::InvalidInput("instants croissants requis".into()));
    }
    // Ordre des tirages fixe : médicaments, fluctuation naturelle, sévérité, puis pas de temps.
    let mut ids: Vec<&str> = doses.iter().map(|d| d.drug.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut drugs_r: Vec<DrugModule> = drugs.to_vec();
    let mut drug_factors = Vec::new();
    for id in &ids {
        let f_cl = rng.lognormal_mean1(cfg.drug_cl_cv);
        let f_vc = rng.lognormal_mean1(cfg.drug_vc_cv);
        let f_ec = rng.lognormal_mean1(cfg.drug_ec50_cv);
        if let Some(m) = drugs_r.iter_mut().find(|m| m.id == *id) {
            m.pk.cl_ml_min_kg *= f_cl;
            m.pk.vc_l_kg *= f_vc;
            m.pd.ec50_mg_l *= f_ec;
        }
        drug_factors.push((id.to_string(), f_cl, f_vc, f_ec));
    }
    let mut nat: Vec<Ou> = cfg.natural.iter().map(|o| Ou::new(o.tau_h, rng)).collect();
    let mut sev: Vec<Ou> = patient
        .cases
        .iter()
        .map(|_| Ou::new(cfg.severity_tau_h, rng))
        .collect();

    let mut undrugged = Vec::with_capacity(t_h.len());
    let mut severity = vec![Vec::with_capacity(t_h.len()); patient.cases.len()];
    let mut prev_t = t_h[0];
    for &t in t_h {
        let dt = t - prev_t;
        prev_t = t;
        for o in nat.iter_mut().chain(sev.iter_mut()) {
            o.step(dt, rng);
        }
        let mut p = patient.base.clone();
        for (term, o) in cfg.natural.iter().zip(&nat) {
            let f = math::exp(term.cv * o.x - 0.5 * term.cv * term.cv);
            *p.get_mut(term.param).expect("nom validé au chargement") *= f;
        }
        for (i, (case, module)) in patient.cases.iter().zip(diseases).enumerate() {
            let s = (case.severity
                + cfg.progression(&case.disease) * t / 24.0
                + cfg.severity_sd * sev[i].x)
                .clamp(0.0, 1.0);
            severity[i].push(s);
            let mut replay = CaseReplay {
                values: &case.draws,
                pos: 0,
            };
            let c = module.case(s, &mut replay)?;
            apply_modifiers(&mut p, &c.modifiers)?;
        }
        undrugged.push(p);
    }
    let start = undrugged[0].clone();
    let t_end = *t_h.last().expect("non vide");
    let tc = timecourse(&start, &drugs_r, doses, t_end, t_h)?;
    // `timecourse` dédoublonne les instants : on rattache chaque instant à sa ligne.
    let row = |t: f64| {
        tc.t.iter()
            .position(|x| *x == t)
            .expect("instant enregistré")
    };
    let mut params = Vec::with_capacity(t_h.len());
    let mut conc: Vec<(String, Vec<f64>)> = tc
        .conc
        .iter()
        .map(|(d, _)| (d.clone(), Vec::new()))
        .collect();
    for (k, &t) in t_h.iter().enumerate() {
        let i = row(t);
        let ci: Vec<(String, f64)> = tc.conc.iter().map(|(d, v)| (d.clone(), v[i])).collect();
        for ((_, out), (_, c)) in conc.iter_mut().zip(&ci) {
            out.push(*c);
        }
        params.push(if ci.is_empty() && tc.d[i] == 0.0 && tc.u[i] == 0.0 {
            undrugged[k].clone()
        } else {
            apply_drug_effects(&undrugged[k], &drugs_r, &ci, tc.d[i], tc.u[i])?
        });
    }
    Ok(Trajectory {
        t: t_h.to_vec(),
        params,
        start,
        severity,
        conc,
        drug_factors,
    })
}

/// Percentile `q` (0–1) par interpolation linéaire d'un échantillon (NaN si vide).
pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    match sorted.len() {
        0 => f64::NAN,
        1 => sorted[0],
        n => {
            let x = q.clamp(0.0, 1.0) * (n - 1) as f64;
            let i = x.floor() as usize;
            let j = (i + 1).min(n - 1);
            sorted[i] + (x - i as f64) * (sorted[j] - sorted[i])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ou_is_stationary_and_correlated() {
        let mut rng = SimRng::new(3);
        let n = 20_000;
        let (mut s1, mut s2, mut sxy) = (0.0, 0.0, 0.0);
        for _ in 0..n {
            let mut o = Ou::new(10.0, &mut rng);
            let x0 = o.x;
            o.step(10.0, &mut rng);
            s1 += x0 * x0;
            s2 += o.x * o.x;
            sxy += x0 * o.x;
        }
        let (v1, v2, r) = (s1 / n as f64, s2 / n as f64, sxy / n as f64);
        assert!(
            (v1 - 1.0).abs() < 0.03 && (v2 - 1.0).abs() < 0.03,
            "{v1} {v2}"
        );
        // corrélation théorique e^(−Δt/τ) = e^(−1)
        assert!((r - math::exp(-1.0)).abs() < 0.03, "{r}");
    }

    #[test]
    fn percentiles() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&v, 0.5), 3.0);
        assert_eq!(percentile(&v, 0.25), 2.0);
        assert_eq!(percentile(&v, 1.0), 5.0);
        assert!(percentile(&[], 0.5).is_nan());
    }
}
