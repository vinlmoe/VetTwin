//! Tirage d'un cas de maladie et application des effets aux paramètres
//! (miroir de `Disease.sample` / `diseases.apply` et de `tests/dsl_ref.py::disease_mods`).

use crate::dsl::{Env, Value};
use crate::error::{Error, EvalError};
use crate::modules::{DiseaseModule, Draw, Effect, EffectOp};
use crate::params::Params;

/// Source des tirages aléatoires d'un cas.
///
/// Le mode déterministe ([`NoNoise`]) est celui du CLI et du golden. Le mode stochastique
/// (générateur à graine explicite `rand_chacha`) viendra avec la population virtuelle (M5).
pub trait DrawSource {
    /// Multiplicateur lognormal de moyenne 1 et d'écart-type `sd`.
    fn lognormal_mean1(&mut self, sd: f64) -> f64;
    /// Vrai avec probabilité `p`.
    fn bernoulli(&mut self, p: f64) -> bool;
    /// Uniforme sur [lo, hi).
    fn uniform(&mut self, lo: f64, hi: f64) -> f64;
    /// Indice tiré selon des poids positifs.
    fn weighted_index(&mut self, weights: &[f64]) -> usize;
}

/// Tirages sans bruit : lognormal → 1, Bernoulli → faux (docs/DSL.md, mode du golden).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoNoise;

impl DrawSource for NoNoise {
    fn lognormal_mean1(&mut self, _sd: f64) -> f64 {
        1.0
    }
    fn bernoulli(&mut self, _p: f64) -> bool {
        false
    }
    /// Milieu de l'intervalle.
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        0.5 * (lo + hi)
    }
    /// Poids le plus élevé (le premier en cas d'égalité).
    fn weighted_index(&mut self, weights: &[f64]) -> usize {
        let mut best = 0;
        for (i, w) in weights.iter().enumerate() {
            if *w > weights[best] {
                best = i;
            }
        }
        best
    }
}

/// Modificateur concret d'un paramètre (effet évalué).
#[derive(Debug, Clone, PartialEq)]
pub struct Modifier {
    /// Paramètre ciblé.
    pub param: &'static str,
    /// Opération.
    pub op: EffectOp,
    /// Valeur évaluée.
    pub value: f64,
    /// Bornes de la valeur finale (optionnelles).
    pub clamp: Option<(f64, f64)>,
}

/// Cas de maladie tiré : sévérité, stade, valeurs des tirages et modificateurs.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// Identifiant du module maladie.
    pub disease: String,
    /// Sévérité `s` (0–1).
    pub severity: f64,
    /// Stade correspondant (aucun si `s` est hors de tous les stades).
    pub stage: Option<String>,
    /// Valeurs des tirages, dans l'ordre du module.
    pub draws: Vec<(String, f64)>,
    /// Modificateurs, dans l'ordre des effets (effets à `when` faux omis).
    pub modifiers: Vec<Modifier>,
    /// Drapeaux descriptifs (`meta_flags`).
    pub flags: Vec<(String, bool)>,
}

/// Évalue une liste d'effets dans `env` (les effets dont `when` est faux sont omis).
pub fn evaluate_effects(
    effects: &[Effect],
    env: &Env,
    module: &str,
) -> Result<Vec<Modifier>, Error> {
    let wrap = |location: &str, source: EvalError| Error::Eval {
        module: module.to_string(),
        location: location.to_string(),
        source,
    };
    let mut out = Vec::with_capacity(effects.len());
    for e in effects {
        if let Some(w) = &e.when {
            let cond = w
                .eval(env)
                .map_err(|err| wrap(&format!("{}/when", e.location), err))?;
            if !cond.truthy() {
                continue;
            }
        }
        let value = e
            .expr
            .eval_f64(env)
            .map_err(|err| wrap(&format!("{}/expr", e.location), err))?;
        out.push(Modifier {
            param: e.param,
            op: e.op,
            value,
            clamp: e.clamp,
        });
    }
    Ok(out)
}

/// Applique des modificateurs **dans l'ordre** : `mul` → `p × v`, `add` → `p + v`, puis `clamp`.
pub fn apply_modifiers(p: &mut Params, mods: &[Modifier]) -> Result<(), Error> {
    for m in mods {
        let slot = p
            .get_mut(m.param)
            .expect("nom de paramètre validé au chargement");
        let mut x = match m.op {
            EffectOp::Mul => *slot * m.value,
            EffectOp::Add => *slot + m.value,
        };
        if let Some((lo, hi)) = m.clamp {
            x = x.max(lo).min(hi);
        }
        if !x.is_finite() {
            return Err(Error::Eval {
                module: String::new(),
                location: format!("paramètre {}", m.param),
                source: EvalError::NonFinite(m.op.as_str()),
            });
        }
        *slot = x;
    }
    Ok(())
}

impl DiseaseModule {
    /// Tire un cas : stade imposé (sévérité uniforme dans ses bornes) ou tiré selon `prior`,
    /// puis tirages du module (comme `Disease.draw_severity` puis `sample`).
    /// Avec [`NoNoise`] : stade de plus forte probabilité a priori, milieu de ses bornes.
    pub fn sample(&self, stage: Option<&str>, draws: &mut dyn DrawSource) -> Result<Case, Error> {
        let st =
            match stage {
                Some(name) => self.stages.iter().find(|s| s.name == name).ok_or_else(|| {
                    Error::UnknownStage {
                        disease: self.id.clone(),
                        stage: name.to_string(),
                    }
                })?,
                None => {
                    let w: Vec<f64> = self.stages.iter().map(|s| s.prior).collect();
                    &self.stages[draws.weighted_index(&w)]
                }
            };
        let s = draws.uniform(st.lo, st.hi);
        let mut case = self.case(s, draws)?;
        // Le stade tiré fait foi (une sévérité sur une borne commune appartiendrait aux deux).
        case.stage = Some(st.name.clone());
        Ok(case)
    }

    /// Tire un cas de sévérité imposée `severity` (le stade se déduit des bornes).
    pub fn case(&self, severity: f64, draws: &mut dyn DrawSource) -> Result<Case, Error> {
        if !(0.0..=1.0).contains(&severity) {
            return Err(Error::InvalidSeverity(severity));
        }
        let wrap = |location: String, source: EvalError| Error::Eval {
            module: self.id.clone(),
            location,
            source,
        };
        let mut env = Env::new();
        env.set_num("s", severity);
        let mut drawn = Vec::with_capacity(self.draws.len());
        // Chaque tirage est fait une fois par cas, dans l'ordre du module.
        for (name, d) in &self.draws {
            let x = match d {
                Draw::LognormalMean1 { sd } => draws.lognormal_mean1(*sd),
                Draw::Bernoulli { p } => {
                    let pv = p
                        .eval_f64(&env)
                        .map_err(|e| wrap(format!("/draws/{name}/p"), e))?;
                    if !(0.0..=1.0).contains(&pv) {
                        return Err(Error::InvalidProbability {
                            module: self.id.clone(),
                            draw: name.clone(),
                            p: pv,
                        });
                    }
                    f64::from(u8::from(draws.bernoulli(pv)))
                }
            };
            env.set_num(name, x);
            drawn.push((name.clone(), x));
        }
        for (name, e) in &self.lets {
            let v = e
                .eval(&env)
                .map_err(|err| wrap(format!("/let/{name}"), err))?;
            env.set(name, v);
        }
        let modifiers = evaluate_effects(&self.effects, &env, &self.id)?;
        let mut flags = Vec::with_capacity(self.meta_flags.len());
        for (name, e) in &self.meta_flags {
            let v: Value = e
                .eval(&env)
                .map_err(|err| wrap(format!("/meta_flags/{name}"), err))?;
            flags.push((name.clone(), v.truthy()));
        }
        Ok(Case {
            disease: self.id.clone(),
            severity,
            stage: self.stage_of(severity).map(|s| s.name.clone()),
            draws: drawn,
            modifiers,
            flags,
        })
    }
}
