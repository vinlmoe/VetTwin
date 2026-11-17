//! Patient : base saine (allométrie) + cas de maladie (miroir de `params.py` / `patient.py`).

use crate::case::{apply_modifiers, Case, DrawSource};
use crate::dsl::Env;
use crate::error::Error;
use crate::modules::{DiseaseModule, PatientModule};
use crate::params::Params;

/// Paramètres du patient sain de poids `bw` (kg).
///
/// Chaque formule du module est évaluée avec `s = bw / reference_bw_kg` et `bw`.
/// Allométrie (définie dans le module, pas ici) : débits et résistances en `s^0.75`,
/// FC et VO2 par kg en `s^-0.25`, élastances en `1/s`, volumes en `s`.
pub fn healthy_params(module: &PatientModule, bw: f64) -> Result<Params, Error> {
    if !(bw.is_finite() && bw > 0.0) {
        return Err(Error::InvalidBodyWeight(bw));
    }
    let mut env = Env::new();
    env.set_num("s", bw / module.reference_bw_kg);
    env.set_num("bw", bw);
    let mut p = Params::zeroed();
    for (name, e) in &module.params {
        let v = e.eval_f64(&env).map_err(|source| Error::Eval {
            module: module.id.clone(),
            location: format!("/params/{name}"),
            source,
        })?;
        *p.get_mut(name).expect("nom validé au chargement") = v;
    }
    Ok(p)
}

/// Patient virtuel : paramètres courants et maladies actives.
#[derive(Debug, Clone, PartialEq)]
pub struct Patient {
    /// Paramètres (sains, puis modifiés par chaque maladie ajoutée).
    pub params: Params,
    /// Cas de maladie, dans l'ordre d'ajout.
    pub cases: Vec<Case>,
}

impl Patient {
    /// Patient sain sans variabilité interindividuelle (mode déterministe).
    pub fn healthy(module: &PatientModule, bw: f64) -> Result<Self, Error> {
        Ok(Self {
            params: healthy_params(module, bw)?,
            cases: Vec::new(),
        })
    }

    /// Patient avec variabilité interindividuelle : chaque paramètre listé dans
    /// `interindividual_variability` est multiplié par un lognormal de moyenne 1 (CV du module),
    /// dans l'ordre du module (comme `Patient(bw, seed, variability=True)`).
    pub fn with_variability(
        module: &PatientModule,
        bw: f64,
        draws: &mut dyn DrawSource,
    ) -> Result<Self, Error> {
        let mut params = healthy_params(module, bw)?;
        for (name, sd) in &module.variability {
            *params.get_mut(name).expect("nom validé au chargement") *= draws.lognormal_mean1(*sd);
        }
        Ok(Self {
            params,
            cases: Vec::new(),
        })
    }

    /// Ajoute une maladie tirée par stade (ou selon `prior` si `stage` est `None`).
    pub fn add_disease_staged(
        &mut self,
        module: &DiseaseModule,
        stage: Option<&str>,
        draws: &mut dyn DrawSource,
    ) -> Result<&Case, Error> {
        let case = module.sample(stage, draws)?;
        self.push_case(module, case)
    }

    fn push_case(&mut self, module: &DiseaseModule, case: Case) -> Result<&Case, Error> {
        let mut p = self.params.clone();
        apply_modifiers(&mut p, &case.modifiers).map_err(|e| match e {
            Error::Eval {
                location, source, ..
            } => Error::Eval {
                module: module.id.clone(),
                location,
                source,
            },
            other => other,
        })?;
        self.params = p;
        self.cases.push(case);
        Ok(self.cases.last().expect("cas ajouté"))
    }

    /// Ajoute une maladie de sévérité imposée ; ses effets s'appliquent aux paramètres courants.
    pub fn add_disease(
        &mut self,
        module: &DiseaseModule,
        severity: f64,
        draws: &mut dyn DrawSource,
    ) -> Result<&Case, Error> {
        let case = module.case(severity, draws)?;
        self.push_case(module, case)
    }
}
