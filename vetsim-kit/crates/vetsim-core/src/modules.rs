//! Chargement et validation des modules JSON (patient sain, maladies, médicaments).
//!
//! La validation reprend les contraintes de `schema/*.json` (types, champs requis, énumérations,
//! motifs) et ajoute des règles sémantiques que le schéma ne peut pas exprimer :
//! - les paramètres cités existent ([`Params::NAMES`]) ;
//! - les variables des formules sont définies à cet endroit (portée du DSL) ;
//! - les stades ont des bornes ordonnées dans [0, 1] et le `prior` couvre exactement les stades ;
//! - les constantes PK sont dans leur domaine (volumes > 0, 0 ≤ f ≤ 1…).
//!
//! Tous les problèmes sont rapportés ensemble, chacun avec son pointeur JSON.
//! Le noyau ne lit aucun fichier : l'appelant fournit le texte et une étiquette d'origine.

use crate::dsl::{ptr_seg, Expr};
use crate::error::{Issue, LoadError};
use crate::params::Params;
use serde_json::{Map, Value as Json};

/// Version de schéma acceptée.
pub const SCHEMA_VERSION: &str = "1.0.0";

// ---------------------------------------------------------------------------------------------
// Outils de validation
// ---------------------------------------------------------------------------------------------

pub(crate) struct Ctx {
    pub(crate) issues: Vec<Issue>,
}

pub(crate) fn kind_of(v: &Json) -> &'static str {
    match v {
        Json::Null => "null",
        Json::Bool(_) => "un booléen",
        Json::Number(_) => "un nombre",
        Json::String(_) => "une chaîne",
        Json::Array(_) => "un tableau",
        Json::Object(_) => "un objet",
    }
}

pub(crate) fn child(path: &str, key: &str) -> String {
    format!("{path}/{}", ptr_seg(key))
}

impl Ctx {
    pub(crate) fn new() -> Self {
        Self { issues: Vec::new() }
    }

    pub(crate) fn err(&mut self, path: impl Into<String>, msg: impl Into<String>) {
        self.issues.push(Issue::new(path, msg));
    }

    pub(crate) fn finish<T>(self, origin: &str, value: T) -> Result<T, LoadError> {
        if self.issues.is_empty() {
            Ok(value)
        } else {
            Err(LoadError::Invalid {
                origin: origin.to_string(),
                issues: self.issues,
            })
        }
    }

    /// Erreur de chargement (garantit au moins un problème).
    pub(crate) fn fail(mut self, origin: &str) -> LoadError {
        if self.issues.is_empty() {
            self.err("", "module incomplet");
        }
        LoadError::Invalid {
            origin: origin.to_string(),
            issues: self.issues,
        }
    }

    pub(crate) fn as_obj<'a>(&mut self, v: &'a Json, path: &str) -> Option<&'a Map<String, Json>> {
        match v {
            Json::Object(m) => Some(m),
            other => {
                self.err(path, format!("objet attendu, {} trouvé", kind_of(other)));
                None
            }
        }
    }

    pub(crate) fn as_num(&mut self, v: &Json, path: &str) -> Option<f64> {
        match v.as_f64() {
            Some(x) if v.is_number() && x.is_finite() => Some(x),
            _ => {
                self.err(path, format!("nombre attendu, {} trouvé", kind_of(v)));
                None
            }
        }
    }

    pub(crate) fn as_str<'a>(&mut self, v: &'a Json, path: &str) -> Option<&'a str> {
        match v {
            Json::String(s) => Some(s),
            other => {
                self.err(path, format!("chaîne attendue, {} trouvé", kind_of(other)));
                None
            }
        }
    }

    pub(crate) fn req<'a>(
        &mut self,
        m: &'a Map<String, Json>,
        path: &str,
        key: &str,
    ) -> Option<&'a Json> {
        let v = m.get(key);
        if v.is_none() {
            self.err(path, format!("champ requis « {key} » manquant"));
        }
        v
    }

    pub(crate) fn req_str<'a>(
        &mut self,
        m: &'a Map<String, Json>,
        path: &str,
        key: &str,
    ) -> Option<&'a str> {
        let v = self.req(m, path, key)?;
        self.as_str(v, &child(path, key))
    }

    pub(crate) fn req_num(&mut self, m: &Map<String, Json>, path: &str, key: &str) -> Option<f64> {
        let v = self.req(m, path, key)?;
        self.as_num(v, &child(path, key))
    }

    pub(crate) fn opt_num(
        &mut self,
        m: &Map<String, Json>,
        path: &str,
        key: &str,
    ) -> Option<Option<f64>> {
        match m.get(key) {
            None => Some(None),
            Some(v) => self.as_num(v, &child(path, key)).map(Some),
        }
    }

    /// Nombre requis vérifiant un prédicat de domaine.
    pub(crate) fn req_num_in(
        &mut self,
        m: &Map<String, Json>,
        path: &str,
        key: &str,
        ok: impl Fn(f64) -> bool,
        domain: &str,
    ) -> Option<f64> {
        let x = self.req_num(m, path, key)?;
        if ok(x) {
            Some(x)
        } else {
            self.err(child(path, key), format!("{x} hors du domaine ({domain})"));
            None
        }
    }

    pub(crate) fn expr(&mut self, v: &Json, path: &str, scope: &[&str]) -> Option<Expr> {
        Expr::parse(v, path, scope, &mut self.issues)
    }

    pub(crate) fn pair(&mut self, v: &Json, path: &str) -> Option<(f64, f64)> {
        match v {
            Json::Array(a) if a.len() == 2 => {
                let lo = self.as_num(&a[0], &format!("{path}/0"))?;
                let hi = self.as_num(&a[1], &format!("{path}/1"))?;
                Some((lo, hi))
            }
            other => {
                self.err(
                    path,
                    format!("tableau de 2 nombres attendu, {} trouvé", kind_of(other)),
                );
                None
            }
        }
    }

    pub(crate) fn schema_version(&mut self, m: &Map<String, Json>) {
        if let Some(v) = self.req_str(m, "", "schema_version") {
            if v != SCHEMA_VERSION {
                self.err(
                    "/schema_version",
                    format!(
                        "version de schéma « {v} » non prise en charge (attendu {SCHEMA_VERSION})"
                    ),
                );
            }
        }
    }

    pub(crate) fn semver(&mut self, m: &Map<String, Json>) -> String {
        let v = self.req_str(m, "", "version").unwrap_or_default();
        let ok = v.split('.').count() == 3
            && v.split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if !v.is_empty() && !ok {
            self.err("/version", format!("« {v} » n'est pas de la forme X.Y.Z"));
        }
        v.to_string()
    }

    pub(crate) fn evidence(&mut self, m: &Map<String, Json>) -> Evidence {
        let e = self.req_str(m, "", "evidence");
        match e {
            Some("A") => Evidence::A,
            Some("B") => Evidence::B,
            Some("C") => Evidence::C,
            Some("D") | None => Evidence::D,
            Some(other) => {
                self.err(
                    "/evidence",
                    format!("niveau de preuve « {other} » invalide (attendu A, B, C ou D)"),
                );
                Evidence::D
            }
        }
    }

    pub(crate) fn text(&mut self, m: &Map<String, Json>, key: &str, required: bool) -> Localized {
        let mut out = Localized::default();
        let Some(v) = m.get(key) else {
            if required {
                self.err("", format!("champ requis « {key} » manquant"));
            }
            return out;
        };
        let path = child("", key);
        if let Some(obj) = self.as_obj(v, &path) {
            for (lang, txt) in obj {
                if let Some(s) = self.as_str(txt, &child(&path, lang)) {
                    out.0.push((lang.clone(), s.to_string()));
                }
            }
        }
        out
    }

    pub(crate) fn species(&mut self, m: &Map<String, Json>) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(v) = m.get("species") {
            match v {
                Json::Array(a) => {
                    for (i, s) in a.iter().enumerate() {
                        let p = format!("/species/{i}");
                        match self.as_str(s, &p) {
                            Some(x @ ("dog" | "cat")) => out.push(x.to_string()),
                            Some(x) => self.err(p, format!("espèce « {x} » inconnue (dog, cat)")),
                            None => {}
                        }
                    }
                }
                other => self.err(
                    "/species",
                    format!("tableau attendu, {} trouvé", kind_of(other)),
                ),
            }
        }
        out
    }

    pub(crate) fn strings(&mut self, m: &Map<String, Json>, key: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(v) = m.get(key) {
            let path = child("", key);
            match v {
                Json::Array(a) => {
                    for (i, s) in a.iter().enumerate() {
                        if let Some(x) = self.as_str(s, &format!("{path}/{i}")) {
                            out.push(x.to_string());
                        }
                    }
                }
                other => self.err(path, format!("tableau attendu, {} trouvé", kind_of(other))),
            }
        }
        out
    }

    /// Un identifiant de variable (draw, let) : `[A-Za-z_][A-Za-z0-9_]*`.
    pub(crate) fn ident(&mut self, name: &str, path: &str) -> bool {
        let mut chars = name.chars();
        let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            self.err(path, format!("nom de variable « {name} » invalide"));
        }
        ok
    }

    pub(crate) fn param(&mut self, name: &str, path: &str) -> Option<&'static str> {
        let found = Params::NAMES.iter().copied().find(|n| *n == name);
        if found.is_none() {
            self.err(path, format!("paramètre inconnu « {name} »"));
        }
        found
    }

    pub(crate) fn effects(
        &mut self,
        v: Option<&Json>,
        path: &str,
        scope: &[&str],
        required: bool,
    ) -> Vec<Effect> {
        let mut out = Vec::new();
        let Some(v) = v else {
            return out;
        };
        let Json::Array(items) = v else {
            self.err(path, format!("tableau attendu, {} trouvé", kind_of(v)));
            return out;
        };
        if required && items.is_empty() {
            self.err(path, "au moins un effet est requis");
        }
        for (i, item) in items.iter().enumerate() {
            let p = format!("{path}/{i}");
            if let Some(e) = self.effect(item, &p, scope) {
                out.push(e);
            }
        }
        out
    }

    pub(crate) fn effect(&mut self, v: &Json, path: &str, scope: &[&str]) -> Option<Effect> {
        let m = self.as_obj(v, path)?;
        let before = self.issues.len();
        for k in m.keys() {
            if !matches!(k.as_str(), "param" | "op" | "expr" | "when" | "clamp") {
                self.err(
                    child(path, k),
                    format!(
                        "champ « {k} » non autorisé dans un effet (param, op, expr, when, clamp)"
                    ),
                );
            }
        }
        let param = self
            .req_str(m, path, "param")
            .and_then(|n| self.param(n, &child(path, "param")));
        let op = match self.req_str(m, path, "op") {
            Some("mul") => Some(EffectOp::Mul),
            Some("add") => Some(EffectOp::Add),
            Some(other) => {
                self.err(
                    child(path, "op"),
                    format!("opération « {other} » invalide (attendu mul ou add)"),
                );
                None
            }
            None => None,
        };
        let expr = self
            .req(m, path, "expr")
            .and_then(|e| self.expr(e, &child(path, "expr"), scope));
        let when = m
            .get("when")
            .and_then(|w| self.expr(w, &child(path, "when"), scope));
        let clamp = m.get("clamp").and_then(|c| {
            let p = child(path, "clamp");
            let (lo, hi) = self.pair(c, &p)?;
            if lo > hi {
                self.err(p, format!("bornes inversées [{lo}, {hi}]"));
                return None;
            }
            Some((lo, hi))
        });
        if self.issues.len() > before {
            return None;
        }
        Some(Effect {
            param: param?,
            op: op?,
            expr: expr?,
            when,
            clamp,
            location: path.to_string(),
        })
    }
}

pub(crate) fn parse_json(json: &str, origin: &str) -> Result<Json, LoadError> {
    serde_json::from_str(json).map_err(|source| LoadError::Json {
        origin: origin.to_string(),
        source,
    })
}

// ---------------------------------------------------------------------------------------------
// Types communs
// ---------------------------------------------------------------------------------------------

/// Niveau de preuve (docs/PROVENANCE.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// Calé sur mesures publiées.
    A,
    /// Bornes ou critères publiés.
    B,
    /// Plausibilité à partir d'intervalles publiés.
    C,
    /// Placeholder pédagogique.
    D,
}

impl Evidence {
    /// Lettre du niveau.
    pub fn as_str(self) -> &'static str {
        match self {
            Evidence::A => "A",
            Evidence::B => "B",
            Evidence::C => "C",
            Evidence::D => "D",
        }
    }
}

/// Texte multilingue (`{"fr": ..., "en": ...}`), ordre du fichier conservé.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Localized(pub Vec<(String, String)>);

impl Localized {
    /// Texte dans la langue demandée, sinon en français, sinon le premier disponible.
    pub fn get(&self, lang: &str) -> Option<&str> {
        let find = |l: &str| self.0.iter().find(|(k, _)| k == l).map(|(_, v)| v.as_str());
        find(lang)
            .or_else(|| find("fr"))
            .or_else(|| self.0.first().map(|(_, v)| v.as_str()))
    }
}

/// Opération d'un effet sur un paramètre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectOp {
    /// `p ← p × valeur`.
    Mul,
    /// `p ← p + valeur`.
    Add,
}

impl EffectOp {
    /// Nom JSON de l'opération.
    pub fn as_str(self) -> &'static str {
        match self {
            EffectOp::Mul => "mul",
            EffectOp::Add => "add",
        }
    }
}

/// Effet déclaratif d'un module sur un paramètre.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    /// Paramètre ciblé (nom validé contre [`Params::NAMES`]).
    pub param: &'static str,
    /// Opération.
    pub op: EffectOp,
    /// Valeur de l'effet.
    pub expr: Expr,
    /// Condition d'application (optionnelle).
    pub when: Option<Expr>,
    /// Bornes de la valeur finale du paramètre (optionnelles).
    pub clamp: Option<(f64, f64)>,
    /// Pointeur JSON de l'effet dans son module (pour les messages d'erreur).
    pub location: String,
}

// ---------------------------------------------------------------------------------------------
// Patient sain
// ---------------------------------------------------------------------------------------------

/// Module du patient sain de référence (`modules/healthy_dog.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct PatientModule {
    /// Identifiant.
    pub id: String,
    /// Version du module.
    pub version: String,
    /// Poids de référence de l'allométrie (`s = bw / reference_bw_kg`).
    pub reference_bw_kg: f64,
    /// Formule de chaque paramètre, dans l'ordre de [`Params::NAMES`].
    pub params: Vec<(&'static str, Expr)>,
    /// Écart-type (CV) du multiplicateur lognormal de moyenne 1 par paramètre.
    pub variability: Vec<(&'static str, f64)>,
}

impl PatientModule {
    /// Charge et valide un module patient.
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root = parse_json(json, origin)?;
        let mut c = Ctx::new();
        let Some(m) = c.as_obj(&root, "") else {
            return Err(c.fail(origin));
        };
        c.schema_version(m);
        let id = c.req_str(m, "", "id").unwrap_or_default().to_string();
        let version = c.semver(m);
        let reference_bw_kg = c.req_num(m, "", "reference_bw_kg").unwrap_or(f64::NAN);
        if reference_bw_kg.is_finite() && reference_bw_kg != 20.0 {
            c.err(
                "/reference_bw_kg",
                format!("doit valoir 20.0 (schéma patient), {reference_bw_kg} trouvé"),
            );
        }
        const SCOPE: &[&str] = &["s", "bw"];
        let mut found: Vec<(&'static str, Expr)> = Vec::new();
        if let Some(obj) = c.req(m, "", "params").and_then(|v| c.as_obj(v, "/params")) {
            for (name, ex) in obj {
                let p = child("/params", name);
                let Some(param) = c.param(name, &p) else {
                    continue;
                };
                if let Some(e) = c.expr(ex, &p, SCOPE) {
                    found.push((param, e));
                }
            }
            for name in Params::NAMES {
                if !obj.contains_key(*name) {
                    c.err("/params", format!("paramètre « {name} » non défini"));
                }
            }
        }
        // Ordre canonique de Params::NAMES (indépendant de l'ordre du fichier).
        let params = Params::NAMES
            .iter()
            .filter_map(|n| found.iter().find(|(k, _)| k == n).cloned())
            .collect();
        let mut variability = Vec::new();
        if let Some(v) = m.get("interindividual_variability") {
            if let Some(obj) = c.as_obj(v, "/interindividual_variability") {
                for (name, d) in obj {
                    let p = child("/interindividual_variability", name);
                    let param = c.param(name, &p);
                    let Some(dm) = c.as_obj(d, &p) else { continue };
                    match c.req_str(dm, &p, "dist") {
                        Some("lognormal_mean1") | None => {}
                        Some(other) => c.err(
                            child(&p, "dist"),
                            format!("distribution « {other} » invalide (attendu lognormal_mean1)"),
                        ),
                    }
                    let sd = c.req_num_in(dm, &p, "sd", |x| x >= 0.0, "≥ 0");
                    if let (Some(param), Some(sd)) = (param, sd) {
                        variability.push((param, sd));
                    }
                }
            }
        }
        c.finish(
            origin,
            Self {
                id,
                version,
                reference_bw_kg,
                params,
                variability,
            },
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Maladies
// ---------------------------------------------------------------------------------------------

/// Stade de sévérité : bornes `[lo, hi]` de `s` et probabilité a priori.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    /// Nom du stade (ex. `B2`, `IRIS3`).
    pub name: String,
    /// Borne basse de `s`.
    pub lo: f64,
    /// Borne haute de `s`.
    pub hi: f64,
    /// Probabilité a priori (non normalisée).
    pub prior: f64,
}

/// Loi d'un tirage aléatoire par cas.
#[derive(Debug, Clone, PartialEq)]
pub enum Draw {
    /// `exp(N(−σ²/2, σ²))`, de moyenne 1.
    LognormalMean1 {
        /// Écart-type σ.
        sd: f64,
    },
    /// 1 avec probabilité `p`, sinon 0.
    Bernoulli {
        /// Probabilité (expression de `s` et des tirages précédents).
        p: Expr,
    },
}

/// Cible de validation publiée ou de plausibilité.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Stade concerné.
    pub stage: String,
    /// Variable de sortie.
    pub var: String,
    /// Intervalle attendu.
    pub range: (f64, f64),
    /// Unité (optionnelle).
    pub unit: Option<String>,
    /// Source.
    pub source: String,
    /// `published_threshold`, `plausibility` ou `measured`.
    pub kind: String,
}

/// Module maladie.
#[derive(Debug, Clone, PartialEq)]
pub struct DiseaseModule {
    /// Identifiant (`[a-z0-9_]+`).
    pub id: String,
    /// Version du module.
    pub version: String,
    /// Nom affiché.
    pub name: Localized,
    /// Description.
    pub description: Localized,
    /// Niveau de preuve.
    pub evidence: Evidence,
    /// Espèces.
    pub species: Vec<String>,
    /// Stades, dans l'ordre du fichier.
    pub stages: Vec<Stage>,
    /// Tirages par cas, dans l'ordre du fichier.
    pub draws: Vec<(String, Draw)>,
    /// Variables intermédiaires, évaluées dans l'ordre.
    pub lets: Vec<(String, Expr)>,
    /// Effets sur les paramètres, appliqués dans l'ordre.
    pub effects: Vec<Effect>,
    /// Drapeaux descriptifs du cas (ex. `hypertension`).
    pub meta_flags: Vec<(String, Expr)>,
    /// Cibles de validation.
    pub targets: Vec<Target>,
    /// Références bibliographiques.
    pub refs: Vec<String>,
    /// Présentation pédagogique (structure libre, transmise telle quelle).
    pub presentation: Option<Json>,
}

impl DiseaseModule {
    /// Charge et valide un module maladie.
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root = parse_json(json, origin)?;
        let mut c = Ctx::new();
        let Some(m) = c.as_obj(&root, "") else {
            return Err(c.fail(origin));
        };
        c.schema_version(m);
        let id = c.req_str(m, "", "id").unwrap_or_default().to_string();
        if !id.is_empty()
            && !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            c.err(
                "/id",
                format!("identifiant « {id} » invalide (attendu [a-z0-9_]+)"),
            );
        }
        let version = c.semver(m);
        let name = c.text(m, "name", true);
        let description = c.text(m, "description", false);
        let evidence = c.evidence(m);
        let species = c.species(m);
        let stages = Self::stages(&mut c, m);

        // Portée : s, puis les tirages, puis les let dans l'ordre.
        let mut scope: Vec<String> = vec!["s".to_string()];
        let mut draws = Vec::new();
        if let Some(v) = m.get("draws") {
            if let Some(obj) = c.as_obj(v, "/draws") {
                for (dname, d) in obj {
                    let p = child("/draws", dname);
                    if !c.ident(dname, &p) {
                        continue;
                    }
                    if scope.contains(dname) {
                        c.err(&p, format!("nom « {dname} » déjà utilisé"));
                        continue;
                    }
                    let refs: Vec<&str> = scope.iter().map(String::as_str).collect();
                    if let Some(draw) = Self::draw(&mut c, d, &p, &refs) {
                        draws.push((dname.clone(), draw));
                    }
                    scope.push(dname.clone());
                }
            }
        }
        let mut lets = Vec::new();
        if let Some(v) = m.get("let") {
            if let Some(obj) = c.as_obj(v, "/let") {
                for (lname, ex) in obj {
                    let p = child("/let", lname);
                    if !c.ident(lname, &p) {
                        continue;
                    }
                    if scope.contains(lname) {
                        c.err(&p, format!("nom « {lname} » déjà utilisé"));
                        continue;
                    }
                    let refs: Vec<&str> = scope.iter().map(String::as_str).collect();
                    if let Some(e) = c.expr(ex, &p, &refs) {
                        lets.push((lname.clone(), e));
                    }
                    scope.push(lname.clone());
                }
            }
        }
        let refs: Vec<&str> = scope.iter().map(String::as_str).collect();
        let effects = match c.req(m, "", "effects") {
            Some(v) => c.effects(Some(v), "/effects", &refs, true),
            None => Vec::new(),
        };
        let mut meta_flags = Vec::new();
        if let Some(v) = m.get("meta_flags") {
            if let Some(obj) = c.as_obj(v, "/meta_flags") {
                for (fname, ex) in obj {
                    if let Some(e) = c.expr(ex, &child("/meta_flags", fname), &refs) {
                        meta_flags.push((fname.clone(), e));
                    }
                }
            }
        }
        let targets = Self::targets(&mut c, m, &stages);
        let refs_list = c.strings(m, "refs");
        let presentation = m.get("presentation").cloned();
        if let Some(p) = &presentation {
            c.as_obj(p, "/presentation");
        }
        if let Some(l) = m.get("license") {
            c.as_str(l, "/license");
        }
        c.finish(
            origin,
            Self {
                id,
                version,
                name,
                description,
                evidence,
                species,
                stages,
                draws,
                lets,
                effects,
                meta_flags,
                targets,
                refs: refs_list,
                presentation,
            },
        )
    }

    fn stages(c: &mut Ctx, m: &Map<String, Json>) -> Vec<Stage> {
        let mut out = Vec::new();
        let Some(sev) = c
            .req(m, "", "severity")
            .and_then(|v| c.as_obj(v, "/severity"))
        else {
            return out;
        };
        let stages = c
            .req(sev, "/severity", "stages")
            .and_then(|v| c.as_obj(v, "/severity/stages"));
        let prior = c
            .req(sev, "/severity", "prior")
            .and_then(|v| c.as_obj(v, "/severity/prior"));
        let (Some(stages), Some(prior)) = (stages, prior) else {
            return out;
        };
        if stages.is_empty() {
            c.err("/severity/stages", "au moins un stade est requis");
        }
        let mut total = 0.0;
        for (name, b) in stages {
            let p = child("/severity/stages", name);
            let Some((lo, hi)) = c.pair(b, &p) else {
                continue;
            };
            if !(0.0..=1.0).contains(&lo) || !(0.0..=1.0).contains(&hi) || lo > hi {
                c.err(
                    &p,
                    format!("bornes [{lo}, {hi}] invalides (0 ≤ lo ≤ hi ≤ 1)"),
                );
                continue;
            }
            let pp = child("/severity/prior", name);
            let pr = match prior.get(name) {
                Some(v) => match c.as_num(v, &pp) {
                    Some(x) if x >= 0.0 => x,
                    Some(x) => {
                        c.err(&pp, format!("probabilité {x} négative"));
                        continue;
                    }
                    None => continue,
                },
                None => {
                    c.err(
                        "/severity/prior",
                        format!("stade « {name} » sans probabilité a priori"),
                    );
                    continue;
                }
            };
            total += pr;
            out.push(Stage {
                name: name.clone(),
                lo,
                hi,
                prior: pr,
            });
        }
        for name in prior.keys() {
            if !stages.contains_key(name) {
                c.err(
                    child("/severity/prior", name),
                    format!("stade « {name} » inconnu"),
                );
            }
        }
        if !stages.is_empty() && total <= 0.0 {
            c.err(
                "/severity/prior",
                "la somme des probabilités a priori doit être > 0",
            );
        }
        out
    }

    fn draw(c: &mut Ctx, v: &Json, path: &str, scope: &[&str]) -> Option<Draw> {
        let m = c.as_obj(v, path)?;
        match c.req_str(m, path, "dist")? {
            "lognormal_mean1" => {
                let sd = c.req_num_in(m, path, "sd", |x| x >= 0.0, "≥ 0")?;
                Some(Draw::LognormalMean1 { sd })
            }
            "bernoulli" => {
                let p = c.req(m, path, "p")?;
                let p = c.expr(p, &child(path, "p"), scope)?;
                Some(Draw::Bernoulli { p })
            }
            other => {
                c.err(
                    child(path, "dist"),
                    format!("distribution « {other} » invalide (lognormal_mean1, bernoulli)"),
                );
                None
            }
        }
    }

    fn targets(c: &mut Ctx, m: &Map<String, Json>, stages: &[Stage]) -> Vec<Target> {
        let mut out = Vec::new();
        let Some(v) = m.get("targets") else {
            return out;
        };
        let Json::Array(items) = v else {
            c.err(
                "/targets",
                format!("tableau attendu, {} trouvé", kind_of(v)),
            );
            return out;
        };
        for (i, t) in items.iter().enumerate() {
            let p = format!("/targets/{i}");
            let Some(tm) = c.as_obj(t, &p) else { continue };
            let stage = c.req_str(tm, &p, "stage");
            if let Some(st) = stage {
                if !stages.is_empty() && !stages.iter().any(|s| s.name == st) {
                    c.err(child(&p, "stage"), format!("stade « {st} » inconnu"));
                }
            }
            let var = c.req_str(tm, &p, "var");
            let range = c
                .req(tm, &p, "range")
                .and_then(|r| c.pair(r, &child(&p, "range")));
            let unit = tm.get("unit").and_then(|u| c.as_str(u, &child(&p, "unit")));
            let source = c.req_str(tm, &p, "source");
            let kind = c.req_str(tm, &p, "kind");
            if let Some(k) = kind {
                if !matches!(k, "published_threshold" | "plausibility" | "measured") {
                    c.err(
                        child(&p, "kind"),
                        format!(
                            "type « {k} » invalide (published_threshold, plausibility, measured)"
                        ),
                    );
                }
            }
            if let (Some(stage), Some(var), Some(range), Some(source), Some(kind)) =
                (stage, var, range, source, kind)
            {
                out.push(Target {
                    stage: stage.to_string(),
                    var: var.to_string(),
                    range,
                    unit: unit.map(str::to_string),
                    source: source.to_string(),
                    kind: kind.to_string(),
                });
            }
        }
        out
    }

    /// Premier stade (ordre du fichier) dont les bornes contiennent `s`, comme `_stage_of`.
    pub fn stage_of(&self, s: f64) -> Option<&Stage> {
        self.stages.iter().find(|st| st.lo <= s && s <= st.hi)
    }
}

// ---------------------------------------------------------------------------------------------
// Médicaments
// ---------------------------------------------------------------------------------------------

/// Constantes PK (2 compartiments + dépôt oral), valeurs pour un chien de 20 kg.
#[derive(Debug, Clone, PartialEq)]
pub struct Pk {
    /// Volume central (L/kg).
    pub vc_l_kg: f64,
    /// Volume périphérique (L/kg).
    pub vp_l_kg: f64,
    /// Clairance (mL/min/kg).
    pub cl_ml_min_kg: f64,
    /// Clairance intercompartimentale (mL/min/kg).
    pub q_ml_min_kg: f64,
    /// Constante d'absorption orale (1/h).
    pub ka_per_h: f64,
    /// Biodisponibilité orale.
    pub f: f64,
    /// Exposant de la masse néphronique sur la clairance.
    pub renal_exp: f64,
    /// Exposant de la fonction hépatique sur la clairance.
    pub hep_exp: f64,
}

/// Type de pharmacodynamie.
#[derive(Debug, Clone, PartialEq)]
pub enum PdKind {
    /// Effet Emax appliqué aux paramètres via la variable `E`.
    Emax,
    /// Diurétique : diurèse, déficit de volume `D` et diurèse effective `U` (MODEL_SPEC §6).
    Diuretic {
        /// Débit urinaire à effet maximal (mL/kg/min).
        urine_max_ml_kg_min: f64,
        /// Fraction du volume urinaire prélevée sur le volume sanguin.
        f_plasma: f64,
        /// Constante de retour du volume (h).
        tau_refill_h: f64,
        /// Constante de récupération des électrolytes (h).
        tau_elect_h: f64,
        /// Effet piloté par l'excrétion urinaire du médicament (`drive: urinary_excretion`) :
        /// concentration × masse néphronique au lieu de la concentration plasmatique.
        urinary_drive: bool,
        /// Adaptation électrolytique (compensation rénale et alimentaire lente) : constante (h)
        /// et fraction compensée φ ; `U` des effets vaut `U − φ·W`, `dW/dt = (U − W)/τ`.
        /// `None` : pas d'adaptation.
        adapt: Option<(f64, f64)>,
    },
}

/// Pharmacodynamie.
#[derive(Debug, Clone, PartialEq)]
pub struct Pd {
    /// Type.
    pub kind: PdKind,
    /// Effet maximal.
    pub emax: f64,
    /// Concentration à demi-effet (mg/L).
    pub ec50_mg_l: f64,
    /// Coefficient de Hill.
    pub hill: f64,
    /// Effets sur les paramètres (variables `E`, et `D`, `U` pour un diurétique).
    pub effects: Vec<Effect>,
    /// Termes toxiques : mécanismes distincts, à concentrations plus hautes (variable `T`).
    pub toxicity: Vec<Toxicity>,
}

/// Terme toxique d'un médicament : `T = emax·C^h/(EC50^h + C^h)`, appliqué après l'effet
/// thérapeutique. L'effet thérapeutique reste plafonné (occupation des récepteurs) ; la toxicité
/// prolonge la relation dose-réponse au-delà de sa saturation.
#[derive(Debug, Clone, PartialEq)]
pub struct Toxicity {
    /// Identifiant du mécanisme (`respiratory_arrest`, `cardiodepression`…).
    pub id: String,
    /// Effet toxique maximal.
    pub emax: f64,
    /// Concentration à demi-effet toxique (mg/L).
    pub ec50_mg_l: f64,
    /// Coefficient de Hill.
    pub hill: f64,
    /// Niveau de preuve.
    pub evidence: Evidence,
    /// Provenance déclarée.
    pub source: Option<String>,
    /// Effets sur les paramètres (variable `T`).
    pub effects: Vec<Effect>,
}

/// Module médicament.
#[derive(Debug, Clone, PartialEq)]
pub struct DrugModule {
    /// Identifiant.
    pub id: String,
    /// Version du module.
    pub version: String,
    /// Nom affiché.
    pub name: Localized,
    /// Niveau de preuve.
    pub evidence: Evidence,
    /// PK.
    pub pk: Pk,
    /// PD.
    pub pd: Pd,
    /// Références.
    pub refs: Vec<String>,
    /// Notes libres.
    pub notes: Option<String>,
    /// Pharmacodynamie anesthésique (optionnelle, module d'anesthésie).
    pub anesthesia: Option<crate::anesthesia::agent::AnesthesiaPd>,
}

impl DrugModule {
    /// Charge et valide un module médicament.
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root = parse_json(json, origin)?;
        let mut c = Ctx::new();
        let Some(m) = c.as_obj(&root, "") else {
            return Err(c.fail(origin));
        };
        c.schema_version(m);
        let id = c.req_str(m, "", "id").unwrap_or_default().to_string();
        if !id.is_empty() {
            c.ident(&id, "/id");
        }
        let version = c.semver(m);
        let name = c.text(m, "name", true);
        let evidence = c.evidence(m);
        let pk = c
            .req(m, "", "pk")
            .and_then(|v| c.as_obj(v, "/pk"))
            .and_then(|pk| Self::pk(&mut c, pk));
        let anesthetic = m.contains_key("anesthesia");
        let pd = c
            .req(m, "", "pd")
            .and_then(|v| c.as_obj(v, "/pd"))
            .and_then(|pd| Self::pd(&mut c, pd, anesthetic));
        let anesthesia = m
            .get("anesthesia")
            .and_then(|a| crate::anesthesia::agent::parse(&mut c, a, "/anesthesia"));
        let refs = c.strings(m, "refs");
        let notes = m
            .get("notes")
            .and_then(|n| c.as_str(n, "/notes"))
            .map(str::to_string);
        match (pk, pd) {
            (Some(pk), Some(pd)) => c.finish(
                origin,
                Self {
                    id,
                    version,
                    name,
                    evidence,
                    pk,
                    pd,
                    refs,
                    notes,
                    anesthesia,
                },
            ),
            _ => Err(c.fail(origin)),
        }
    }

    fn pk(c: &mut Ctx, m: &Map<String, Json>) -> Option<Pk> {
        let p = "/pk";
        let pos = |x: f64| x > 0.0;
        let nonneg = |x: f64| x >= 0.0;
        let vc = c.req_num_in(m, p, "vc_L_kg", pos, "> 0");
        let vp = c.req_num_in(m, p, "vp_L_kg", pos, "> 0");
        let cl = c.req_num_in(m, p, "cl_mL_min_kg", nonneg, "≥ 0");
        let q = c.req_num_in(m, p, "q_mL_min_kg", nonneg, "≥ 0");
        let ka = c.req_num_in(m, p, "ka_per_h", pos, "> 0");
        let f = c.req_num_in(m, p, "f", |x| (0.0..=1.0).contains(&x), "0 ≤ f ≤ 1");
        let renal = c.opt_num(m, p, "renal_exp");
        let hep = c.opt_num(m, p, "hep_exp");
        Some(Pk {
            vc_l_kg: vc?,
            vp_l_kg: vp?,
            cl_ml_min_kg: cl?,
            q_ml_min_kg: q?,
            ka_per_h: ka?,
            f: f?,
            renal_exp: renal?.unwrap_or(0.0),
            hep_exp: hep?.unwrap_or(0.0),
        })
    }

    fn pd(c: &mut Ctx, m: &Map<String, Json>, anesthetic: bool) -> Option<Pd> {
        let p = "/pd";
        let kind = match c.req_str(m, p, "type") {
            Some("emax") => Some(PdKind::Emax),
            Some("diuretic") => {
                let pos = |x: f64| x > 0.0;
                let u = c.req_num_in(m, p, "urine_max_mL_kg_min", |x| x >= 0.0, "≥ 0");
                let fp = c.req_num_in(m, p, "f_plasma", |x| (0.0..=1.0).contains(&x), "0 ≤ f ≤ 1");
                let tr = c.req_num_in(m, p, "tau_refill_h", pos, "> 0");
                let te = c.req_num_in(m, p, "tau_elect_h", pos, "> 0");
                let urinary_drive = match m.get("drive") {
                    None => false,
                    Some(Json::String(s)) if s == "plasma" => false,
                    Some(Json::String(s)) if s == "urinary_excretion" => true,
                    Some(_) => {
                        c.err("/pd/drive", "« plasma » ou « urinary_excretion » attendu");
                        false
                    }
                };
                let adapt = match (m.get("adapt_tau_h"), m.get("adapt_frac")) {
                    (None, None) => None,
                    _ => {
                        let at = c.req_num_in(m, p, "adapt_tau_h", pos, "> 0");
                        let af = c.req_num_in(
                            m,
                            p,
                            "adapt_frac",
                            |x| (0.0..=1.0).contains(&x),
                            "0 ≤ φ ≤ 1",
                        );
                        at.zip(af)
                    }
                };
                match (u, fp, tr, te) {
                    (Some(u), Some(fp), Some(tr), Some(te)) => Some(PdKind::Diuretic {
                        urine_max_ml_kg_min: u,
                        f_plasma: fp,
                        tau_refill_h: tr,
                        tau_elect_h: te,
                        urinary_drive,
                        adapt,
                    }),
                    _ => None,
                }
            }
            Some(other) => {
                c.err(
                    "/pd/type",
                    format!("type « {other} » invalide (emax, diuretic)"),
                );
                None
            }
            None => None,
        };
        let emax = c.req_num_in(m, p, "emax", |x| x >= 0.0, "≥ 0");
        let ec50 = c.req_num_in(m, p, "ec50_mg_L", |x| x > 0.0, "> 0");
        let hill = c.req_num_in(m, p, "hill", |x| x > 0.0, "> 0");
        let scope: &[&str] = match kind {
            Some(PdKind::Diuretic { .. }) => &["E", "D", "U"],
            // agent d'anesthésie : `E` au site effet, `EP` sur la concentration plasmatique
            _ if anesthetic => &["E", "EP"],
            _ => &["E"],
        };
        let effects = c.req(m, p, "effects");
        let effects = c.effects(effects, "/pd/effects", scope, false);
        let toxicity = Self::toxicity(c, m);
        Some(Pd {
            kind: kind?,
            emax: emax?,
            ec50_mg_l: ec50?,
            hill: hill?,
            effects,
            toxicity,
        })
    }

    fn toxicity(c: &mut Ctx, m: &Map<String, Json>) -> Vec<Toxicity> {
        let mut out = Vec::new();
        let Some(v) = m.get("toxicity") else {
            return out;
        };
        let Json::Array(items) = v else {
            c.err(
                "/pd/toxicity",
                format!("tableau attendu, {} trouvé", kind_of(v)),
            );
            return out;
        };
        for (i, item) in items.iter().enumerate() {
            let p = format!("/pd/toxicity/{i}");
            let Some(t) = c.as_obj(item, &p) else {
                continue;
            };
            for k in t.keys() {
                if !matches!(
                    k.as_str(),
                    "id" | "emax" | "ec50_mg_L" | "hill" | "evidence" | "source" | "effects"
                ) {
                    c.err(
                        child(&p, k),
                        format!("champ « {k} » non autorisé dans un terme toxique"),
                    );
                }
            }
            let id = c.req_str(t, &p, "id").map(str::to_string);
            if let Some(id) = &id {
                c.ident(id, &child(&p, "id"));
            }
            let emax = c.req_num_in(t, &p, "emax", |x| x >= 0.0, "≥ 0");
            let ec50 = c.req_num_in(t, &p, "ec50_mg_L", |x| x > 0.0, "> 0");
            let hill = c.req_num_in(t, &p, "hill", |x| x > 0.0, "> 0");
            let evidence = match t.get("evidence").map(Json::as_str) {
                None | Some(Some("D")) => Evidence::D,
                Some(Some("A")) => Evidence::A,
                Some(Some("B")) => Evidence::B,
                Some(Some("C")) => Evidence::C,
                Some(_) => {
                    c.err(
                        child(&p, "evidence"),
                        "niveau de preuve invalide (attendu A, B, C ou D)",
                    );
                    Evidence::D
                }
            };
            let source = t.get("source").and_then(Json::as_str).map(str::to_string);
            let eff = c.req(t, &p, "effects");
            let effects = c.effects(eff, &child(&p, "effects"), &["T"], true);
            if let (Some(id), Some(emax), Some(ec50), Some(hill)) = (id, emax, ec50, hill) {
                out.push(Toxicity {
                    id,
                    emax,
                    ec50_mg_l: ec50,
                    hill,
                    evidence,
                    source,
                    effects,
                });
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Catalogue
// ---------------------------------------------------------------------------------------------

/// Ensemble validé des modules chargés.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    /// Patient sain de référence.
    pub patient: PatientModule,
    /// Maladies, triées par identifiant.
    pub diseases: Vec<DiseaseModule>,
    /// Médicaments, triés par identifiant.
    pub drugs: Vec<DrugModule>,
}

impl Catalog {
    /// Assemble un catalogue (refuse les identifiants dupliqués).
    pub fn new(
        patient: PatientModule,
        mut diseases: Vec<DiseaseModule>,
        mut drugs: Vec<DrugModule>,
    ) -> Result<Self, LoadError> {
        diseases.sort_by(|a, b| a.id.cmp(&b.id));
        drugs.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(w) = diseases.windows(2).find(|w| w[0].id == w[1].id) {
            return Err(LoadError::Duplicate {
                kind: "disease",
                id: w[0].id.clone(),
            });
        }
        if let Some(w) = drugs.windows(2).find(|w| w[0].id == w[1].id) {
            return Err(LoadError::Duplicate {
                kind: "drug",
                id: w[0].id.clone(),
            });
        }
        Ok(Self {
            patient,
            diseases,
            drugs,
        })
    }

    /// Module maladie par identifiant.
    pub fn disease(&self, id: &str) -> Option<&DiseaseModule> {
        self.diseases.iter().find(|d| d.id == id)
    }

    /// Module médicament par identifiant.
    pub fn drug(&self, id: &str) -> Option<&DrugModule> {
        self.drugs.iter().find(|d| d.id == id)
    }
}
