//! Pharmacodynamie anesthésique d'un module médicament (bloc optionnel `anesthesia`).
//!
//! Le bloc complète la PK/PD générique (`pk`, `pd.effects`, `pd.toxicity`) par ce dont la
//! session d'anesthésie a besoin : compartiment effet (`ke0`), voies IM et SC, contribution à
//! l'hypnose (en équivalents MAC), sédation, épargne de MAC, antinociception, effet
//! anti-hyperalgésique (NMDA), antagonisme compétitif et anesthésie locorégionale.
//!
//! Toutes les grandeurs « effet » sont des fonctions de Hill de la concentration au site effet
//! `Ce` (mg/L) : `max·Ce^h/(EC50^h + Ce^h)`. Les valeurs viennent des modules JSON ; celles qui
//! ne sont pas sourcées portent `evidence: "D"`.

use crate::math;
use crate::modules::{child, kind_of, Ctx, Evidence};
use serde_json::{Map, Value as Json};

/// Classe pharmacologique (affichage et reconnaissance de l'anesthésie dissociative).
pub const CLASSES: &[&str] = &[
    "alpha2_agonist",
    "phenothiazine",
    "benzodiazepine",
    "opioid",
    "hypnotic",
    "dissociative",
    "local_anesthetic",
    "antagonist",
];

/// Courbe de Hill saturante : `max·C^h/(EC50^h + C^h)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sat {
    /// Effet maximal (0–1).
    pub max: f64,
    /// Concentration à demi-effet (mg/L).
    pub ec50_mg_l: f64,
    /// Coefficient de Hill.
    pub hill: f64,
}

impl Sat {
    /// Effet à la concentration `c` ; `shift` multiplie l'EC50 (antagonisme compétitif).
    pub fn at(&self, c: f64, shift: f64) -> f64 {
        let x = math::pow(c.max(0.0), self.hill);
        let k = math::pow(self.ec50_mg_l * shift, self.hill);
        self.max * x / (k + x)
    }
}

/// Voie extravasculaire : biodisponibilité et constante d'absorption.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Absorption {
    /// Biodisponibilité (0–1).
    pub f: f64,
    /// Constante d'absorption (1/h).
    pub ka_per_h: f64,
}

/// Antagonisme compétitif : l'EC50 des cibles est multipliée par `1 + C_ant/Ki`.
#[derive(Debug, Clone, PartialEq)]
pub struct Antagonism {
    /// Identifiants des médicaments antagonisés.
    pub targets: Vec<String>,
    /// Constante d'inhibition apparente (mg/L).
    pub ki_mg_l: f64,
}

/// Anesthésie locorégionale : délai d'installation et durée du bloc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalBlock {
    /// Délai d'installation (min).
    pub onset_min: f64,
    /// Durée du bloc complet (min).
    pub duration_min: f64,
    /// Fraction de la nociception bloquée sur le territoire (0–1).
    pub efficacy: f64,
}

/// Bloc `anesthesia` d'un module médicament.
#[derive(Debug, Clone, PartialEq)]
pub struct AnesthesiaPd {
    /// Classes pharmacologiques.
    pub classes: Vec<String>,
    /// Constante d'équilibration plasma → site effet (1/min) ; `None` : site effet = plasma.
    pub ke0_per_min: Option<f64>,
    /// Voie intramusculaire.
    pub im: Option<Absorption>,
    /// Voie sous-cutanée.
    pub sc: Option<Absorption>,
    /// Concentration au site effet équivalente à 1 MAC (immobilité au stimulus supramaximal) :
    /// contribution hypnotique `Ce/c_imm` en équivalents MAC.
    pub c_imm_mg_l: Option<f64>,
    /// Sédation (0–1) ; potentialise aussi l'hypnose (`H/(1 − Sd)` pour la perte de conscience).
    pub sedation: Option<Sat>,
    /// Épargne de MAC (fraction de réduction).
    pub mac_reduction: Option<Sat>,
    /// Antinociception (fraction de la nociception supprimée, réponse autonome).
    pub antinociception: Option<Sat>,
    /// Effet anti-hyperalgésique (frein de la sensibilisation centrale, NMDA).
    pub antihyperalgesia: Option<Sat>,
    /// Antagonisme compétitif.
    pub antagonist: Option<Antagonism>,
    /// Anesthésie locorégionale.
    pub local: Option<LocalBlock>,
    /// Niveau de preuve du bloc.
    pub evidence: Evidence,
    /// Provenance déclarée.
    pub source: Option<String>,
}

impl AnesthesiaPd {
    /// Vrai si la classe `c` est déclarée.
    pub fn is(&self, c: &str) -> bool {
        self.classes.iter().any(|x| x == c)
    }
}

fn sat(c: &mut Ctx, m: &Map<String, Json>, path: &str, key: &str) -> Option<Option<Sat>> {
    let Some(v) = m.get(key) else {
        return Some(None);
    };
    let p = child(path, key);
    let o = c.as_obj(v, &p)?;
    for k in o.keys() {
        if !matches!(k.as_str(), "max" | "ec50_mg_L" | "hill") {
            c.err(
                child(&p, k),
                format!("champ « {k} » non autorisé (max, ec50_mg_L, hill)"),
            );
        }
    }
    let max = c.req_num_in(o, &p, "max", |x| (0.0..=1.0).contains(&x), "0 ≤ max ≤ 1");
    let ec50 = c.req_num_in(o, &p, "ec50_mg_L", |x| x > 0.0, "> 0");
    let hill = c.req_num_in(o, &p, "hill", |x| x > 0.0, "> 0");
    Some(Some(Sat {
        max: max?,
        ec50_mg_l: ec50?,
        hill: hill?,
    }))
}

fn absorption(
    c: &mut Ctx,
    m: &Map<String, Json>,
    path: &str,
    key: &str,
) -> Option<Option<Absorption>> {
    let Some(v) = m.get(key) else {
        return Some(None);
    };
    let p = child(path, key);
    let o = c.as_obj(v, &p)?;
    let f = c.req_num_in(o, &p, "f", |x| (0.0..=1.0).contains(&x), "0 ≤ f ≤ 1");
    let ka = c.req_num_in(o, &p, "ka_per_h", |x| x > 0.0, "> 0");
    Some(Some(Absorption {
        f: f?,
        ka_per_h: ka?,
    }))
}

const KEYS: &[&str] = &[
    "classes",
    "ke0_per_min",
    "im",
    "sc",
    "c_imm_mg_L",
    "sedation",
    "mac_reduction",
    "antinociception",
    "antihyperalgesia",
    "antagonist",
    "local",
    "evidence",
    "source",
];

/// Lit et valide le bloc `anesthesia` (problèmes ajoutés à `c`).
pub(crate) fn parse(c: &mut Ctx, v: &Json, path: &str) -> Option<AnesthesiaPd> {
    let m = c.as_obj(v, path)?;
    let before = c.issues.len();
    for k in m.keys() {
        if !KEYS.contains(&k.as_str()) {
            c.err(
                child(path, k),
                format!("champ « {k} » non autorisé dans le bloc anesthesia"),
            );
        }
    }
    let mut classes = Vec::new();
    match m.get("classes") {
        Some(Json::Array(a)) => {
            for (i, x) in a.iter().enumerate() {
                let p = format!("{path}/classes/{i}");
                if let Some(s) = c.as_str(x, &p) {
                    if CLASSES.contains(&s) {
                        classes.push(s.to_string());
                    } else {
                        c.err(
                            p,
                            format!("classe « {s} » inconnue ({})", CLASSES.join(", ")),
                        );
                    }
                }
            }
        }
        Some(other) => c.err(
            child(path, "classes"),
            format!("tableau attendu, {} trouvé", kind_of(other)),
        ),
        None => c.err(child(path, "classes"), "champ requis"),
    }
    let ke0 = c.opt_num(m, path, "ke0_per_min");
    if let Some(Some(k)) = ke0 {
        if k <= 0.0 {
            c.err(
                child(path, "ke0_per_min"),
                format!("{k} hors du domaine (> 0)"),
            );
        }
    }
    let im = absorption(c, m, path, "im");
    let sc = absorption(c, m, path, "sc");
    let c_imm = c.opt_num(m, path, "c_imm_mg_L");
    if let Some(Some(x)) = c_imm {
        if x <= 0.0 {
            c.err(
                child(path, "c_imm_mg_L"),
                format!("{x} hors du domaine (> 0)"),
            );
        }
    }
    let sedation = sat(c, m, path, "sedation");
    let mac_reduction = sat(c, m, path, "mac_reduction");
    let antinociception = sat(c, m, path, "antinociception");
    let antihyperalgesia = sat(c, m, path, "antihyperalgesia");
    let antagonist = match m.get("antagonist") {
        None => Some(None),
        Some(v) => {
            let p = child(path, "antagonist");
            c.as_obj(v, &p).and_then(|o| {
                let targets = c.strings(o, "targets");
                if targets.is_empty() {
                    c.err(child(&p, "targets"), "au moins une cible");
                }
                let ki = c.req_num_in(o, &p, "ki_mg_L", |x| x > 0.0, "> 0")?;
                Some(Some(Antagonism {
                    targets,
                    ki_mg_l: ki,
                }))
            })
        }
    };
    let local = match m.get("local") {
        None => Some(None),
        Some(v) => {
            let p = child(path, "local");
            c.as_obj(v, &p).and_then(|o| {
                let on = c.req_num_in(o, &p, "onset_min", |x| x >= 0.0, "≥ 0");
                let du = c.req_num_in(o, &p, "duration_min", |x| x > 0.0, "> 0");
                let ef = c.req_num_in(o, &p, "efficacy", |x| (0.0..=1.0).contains(&x), "0–1");
                Some(Some(LocalBlock {
                    onset_min: on?,
                    duration_min: du?,
                    efficacy: ef?,
                }))
            })
        }
    };
    let evidence = c.evidence(m);
    let source = m.get("source").and_then(Json::as_str).map(str::to_string);
    if c.issues.len() > before {
        return None;
    }
    Some(AnesthesiaPd {
        classes,
        ke0_per_min: ke0?,
        im: im?,
        sc: sc?,
        c_imm_mg_l: c_imm?,
        sedation: sedation?,
        mac_reduction: mac_reduction?,
        antinociception: antinociception?,
        antihyperalgesia: antihyperalgesia?,
        antagonist: antagonist?,
        local: local?,
        evidence,
        source,
    })
}
