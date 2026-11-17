//! Interpréteur du DSL de formules (docs/DSL.md, version 1.0.0).
//!
//! Une expression JSON est transformée **au chargement** en un arbre d'opérations fermé
//! ([`Expr`]) : aucune exécution de code issu du JSON n'est possible. Le parseur vérifie
//! l'arité de chaque opérateur et la portée des variables, avec le chemin JSON de chaque
//! problème. L'évaluation reproduit `tests/dsl_ref.py` :
//! - les nombres sont des `f64` ; les booléens valent 1/0 dans un calcul (comme en Python) ;
//! - `if`, `and`, `or` sont paresseux (seule la branche utile est évaluée) ;
//! - une division par zéro, un `log` d'un nombre ≤ 0 ou un résultat non fini est une
//!   [`EvalError`], jamais un `NaN` silencieux. C'est plus strict que Python sur un point :
//!   un dépassement flottant (`inf`) est refusé ici alors que Python le laisse passer.

use crate::error::{EvalError, Issue};
use crate::math;
use serde_json::Value as Json;

/// Valeur d'une expression : nombre ou booléen (résultat d'une comparaison).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// Nombre flottant.
    Num(f64),
    /// Booléen.
    Bool(bool),
}

impl Value {
    /// Valeur numérique (booléen → 1 ou 0, comme en Python).
    pub fn as_f64(self) -> f64 {
        match self {
            Value::Num(x) => x,
            Value::Bool(b) => f64::from(u8::from(b)),
        }
    }

    /// Valeur de vérité (nombre non nul → vrai, comme en Python).
    pub fn truthy(self) -> bool {
        match self {
            Value::Num(x) => x != 0.0,
            Value::Bool(b) => b,
        }
    }
}

/// Opérateurs n-aires (au moins un argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NaryOp {
    /// Somme.
    Add,
    /// Produit.
    Mul,
    /// Minimum.
    Min,
    /// Maximum.
    Max,
    /// Conjonction (paresseuse).
    And,
    /// Disjonction (paresseuse).
    Or,
}

/// Opérateurs binaires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `a − b`.
    Sub,
    /// `a / b`.
    Div,
    /// `a ^ b`.
    Pow,
    /// `a < b`.
    Lt,
    /// `a ≤ b`.
    Le,
    /// `a > b`.
    Gt,
    /// `a ≥ b`.
    Ge,
}

/// Opérateurs unaires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// Exponentielle.
    Exp,
    /// Logarithme népérien.
    Log,
    /// Opposé.
    Neg,
    /// Valeur absolue.
    Abs,
    /// Négation logique.
    Not,
}

/// Opérateurs ternaires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerOp {
    /// `clip [x, lo, hi]` = `min(max(x, lo), hi)`.
    Clip,
    /// `if [cond, a, b]` (paresseux).
    If,
}

/// Arbre d'expression fermé.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Constante numérique.
    Num(f64),
    /// Constante booléenne.
    Bool(bool),
    /// Variable de l'environnement.
    Var(String),
    /// Opération n-aire.
    Nary(NaryOp, Vec<Expr>),
    /// Opération binaire.
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// Opération unaire.
    Unary(UnOp, Box<Expr>),
    /// Opération ternaire.
    Ternary(TerOp, Box<[Expr; 3]>),
}

enum Arity {
    Nary(NaryOp),
    Binary(BinOp),
    Unary(UnOp),
    Ternary(TerOp),
}

fn op_of(name: &str) -> Option<Arity> {
    use Arity::*;
    Some(match name {
        "add" => Nary(NaryOp::Add),
        "mul" => Nary(NaryOp::Mul),
        "min" => Nary(NaryOp::Min),
        "max" => Nary(NaryOp::Max),
        "and" => Nary(NaryOp::And),
        "or" => Nary(NaryOp::Or),
        "sub" => Binary(BinOp::Sub),
        "div" => Binary(BinOp::Div),
        "pow" => Binary(BinOp::Pow),
        "lt" => Binary(BinOp::Lt),
        "le" => Binary(BinOp::Le),
        "gt" => Binary(BinOp::Gt),
        "ge" => Binary(BinOp::Ge),
        "exp" => Unary(UnOp::Exp),
        "log" => Unary(UnOp::Log),
        "neg" => Unary(UnOp::Neg),
        "abs" => Unary(UnOp::Abs),
        "not" => Unary(UnOp::Not),
        "clip" => Ternary(TerOp::Clip),
        "if" => Ternary(TerOp::If),
        _ => return None,
    })
}

/// Noms d'opérateurs acceptés (pour les messages d'erreur).
pub const OPERATORS: &[&str] = &[
    "add", "sub", "mul", "div", "pow", "exp", "log", "neg", "abs", "min", "max", "clip", "if",
    "lt", "le", "gt", "ge", "not", "and", "or",
];

/// Échappe un segment de pointeur JSON (RFC 6901).
pub(crate) fn ptr_seg(seg: &str) -> String {
    seg.replace('~', "~0").replace('/', "~1")
}

impl Expr {
    /// Parse une expression JSON. `path` est le pointeur JSON de `json` (pour les messages),
    /// `scope` la liste des variables visibles. Les problèmes sont ajoutés à `issues`;
    /// renvoie `None` si l'expression est invalide.
    pub fn parse(json: &Json, path: &str, scope: &[&str], issues: &mut Vec<Issue>) -> Option<Expr> {
        match json {
            Json::Bool(b) => Some(Expr::Bool(*b)),
            Json::Number(n) => match n.as_f64() {
                Some(x) if x.is_finite() => Some(Expr::Num(x)),
                _ => {
                    issues.push(Issue::new(path, format!("nombre non représentable : {n}")));
                    None
                }
            },
            Json::String(name) => {
                if scope.contains(&name.as_str()) {
                    Some(Expr::Var(name.clone()))
                } else {
                    let visible = if scope.is_empty() {
                        "aucune".to_string()
                    } else {
                        scope.join(", ")
                    };
                    issues.push(Issue::new(
                        path,
                        format!("variable inconnue « {name} » (visibles ici : {visible})"),
                    ));
                    None
                }
            }
            Json::Object(map) => {
                if map.len() != 1 {
                    issues.push(Issue::new(
                        path,
                        format!(
                            "une opération est un objet à une seule clé ({} clés trouvées)",
                            map.len()
                        ),
                    ));
                    return None;
                }
                let (name, arg) = map.iter().next()?;
                let Some(arity) = op_of(name) else {
                    issues.push(Issue::new(
                        path,
                        format!(
                            "opérateur inconnu « {name} » (attendu : {})",
                            OPERATORS.join(", ")
                        ),
                    ));
                    return None;
                };
                let sub = format!("{path}/{}", ptr_seg(name));
                // Comme dsl_ref.py : un argument non-tableau vaut un tableau à un élément.
                let (items, item_path): (Vec<&Json>, Box<dyn Fn(usize) -> String>) = match arg {
                    Json::Array(a) => {
                        let sub = sub.clone();
                        (a.iter().collect(), Box::new(move |i| format!("{sub}/{i}")))
                    }
                    other => {
                        let sub = sub.clone();
                        (vec![other], Box::new(move |_| sub.clone()))
                    }
                };
                let expected = match arity {
                    Arity::Nary(_) => None,
                    Arity::Unary(_) => Some(1),
                    Arity::Binary(_) => Some(2),
                    Arity::Ternary(_) => Some(3),
                };
                match expected {
                    None if items.is_empty() => {
                        issues.push(Issue::new(
                            &sub,
                            format!("« {name} » attend au moins un argument"),
                        ));
                        return None;
                    }
                    Some(n) if items.len() != n => {
                        issues.push(Issue::new(
                            &sub,
                            format!(
                                "« {name} » attend {n} argument(s), {} fourni(s)",
                                items.len()
                            ),
                        ));
                        return None;
                    }
                    _ => {}
                }
                let before = issues.len();
                let args: Vec<Option<Expr>> = items
                    .iter()
                    .enumerate()
                    .map(|(i, j)| Expr::parse(j, &item_path(i), scope, issues))
                    .collect();
                if issues.len() > before {
                    return None;
                }
                let mut args: Vec<Expr> = args.into_iter().map(Option::unwrap).collect();
                Some(match arity {
                    Arity::Nary(op) => Expr::Nary(op, args),
                    Arity::Unary(op) => Expr::Unary(op, Box::new(args.pop()?)),
                    Arity::Binary(op) => {
                        let b = args.pop()?;
                        let a = args.pop()?;
                        Expr::Binary(op, Box::new(a), Box::new(b))
                    }
                    Arity::Ternary(op) => {
                        let c = args.pop()?;
                        let b = args.pop()?;
                        let a = args.pop()?;
                        Expr::Ternary(op, Box::new([a, b, c]))
                    }
                })
            }
            Json::Null => {
                issues.push(Issue::new(path, "expression nulle"));
                None
            }
            Json::Array(_) => {
                issues.push(Issue::new(
                    path,
                    "un tableau n'est pas une expression (les tableaux sont les arguments d'une opération)",
                ));
                None
            }
        }
    }

    /// Évalue l'expression dans l'environnement `env`.
    pub fn eval(&self, env: &Env) -> Result<Value, EvalError> {
        Ok(match self {
            Expr::Num(x) => Value::Num(*x),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Var(name) => env
                .get(name)
                .ok_or_else(|| EvalError::UnknownVariable(name.clone()))?,
            Expr::Nary(op, args) => match op {
                NaryOp::And => {
                    for a in args {
                        if !a.eval(env)?.truthy() {
                            return Ok(Value::Bool(false));
                        }
                    }
                    Value::Bool(true)
                }
                NaryOp::Or => {
                    for a in args {
                        if a.eval(env)?.truthy() {
                            return Ok(Value::Bool(true));
                        }
                    }
                    Value::Bool(false)
                }
                NaryOp::Add | NaryOp::Mul | NaryOp::Min | NaryOp::Max => {
                    let mut acc: Option<f64> = None;
                    for a in args {
                        let x = a.eval(env)?.as_f64();
                        acc = Some(match (acc, op) {
                            (None, _) => x,
                            (Some(r), NaryOp::Add) => r + x,
                            (Some(r), NaryOp::Mul) => r * x,
                            // Python min/max : garde le premier en cas d'égalité.
                            (Some(r), NaryOp::Min) => {
                                if x < r {
                                    x
                                } else {
                                    r
                                }
                            }
                            (Some(r), _) => {
                                if x > r {
                                    x
                                } else {
                                    r
                                }
                            }
                        });
                    }
                    // L'arité ≥ 1 est garantie par le parseur.
                    let r = acc.unwrap_or(0.0);
                    finite(r, nary_name(*op))?
                }
            },
            Expr::Binary(op, a, b) => {
                let x = a.eval(env)?.as_f64();
                let y = b.eval(env)?.as_f64();
                match op {
                    BinOp::Sub => finite(x - y, "sub")?,
                    BinOp::Div => {
                        if y == 0.0 {
                            return Err(EvalError::DivisionByZero("div"));
                        }
                        finite(x / y, "div")?
                    }
                    BinOp::Pow => {
                        if x == 0.0 && y < 0.0 {
                            return Err(EvalError::DivisionByZero("pow"));
                        }
                        finite(math::pow(x, y), "pow")?
                    }
                    BinOp::Lt => Value::Bool(x < y),
                    BinOp::Le => Value::Bool(x <= y),
                    BinOp::Gt => Value::Bool(x > y),
                    BinOp::Ge => Value::Bool(x >= y),
                }
            }
            Expr::Unary(op, a) => {
                let v = a.eval(env)?;
                match op {
                    UnOp::Not => Value::Bool(!v.truthy()),
                    UnOp::Neg => Value::Num(-v.as_f64()),
                    UnOp::Abs => Value::Num(v.as_f64().abs()),
                    UnOp::Exp => finite(math::exp(v.as_f64()), "exp")?,
                    UnOp::Log => {
                        let x = v.as_f64();
                        if x <= 0.0 {
                            return Err(EvalError::LogDomain(x));
                        }
                        Value::Num(math::ln(x))
                    }
                }
            }
            Expr::Ternary(op, args) => match op {
                TerOp::If => {
                    if args[0].eval(env)?.truthy() {
                        args[1].eval(env)?
                    } else {
                        args[2].eval(env)?
                    }
                }
                TerOp::Clip => {
                    let x = args[0].eval(env)?.as_f64();
                    let lo = args[1].eval(env)?.as_f64();
                    let hi = args[2].eval(env)?.as_f64();
                    // Python : min(max(x, lo), hi)
                    let m = if lo > x { lo } else { x };
                    Value::Num(if hi < m { hi } else { m })
                }
            },
        })
    }

    /// Évalue et convertit en nombre.
    pub fn eval_f64(&self, env: &Env) -> Result<f64, EvalError> {
        self.eval(env).map(Value::as_f64)
    }
}

fn nary_name(op: NaryOp) -> &'static str {
    match op {
        NaryOp::Add => "add",
        NaryOp::Mul => "mul",
        NaryOp::Min => "min",
        NaryOp::Max => "max",
        NaryOp::And => "and",
        NaryOp::Or => "or",
    }
}

fn finite(x: f64, op: &'static str) -> Result<Value, EvalError> {
    if x.is_finite() {
        Ok(Value::Num(x))
    } else {
        Err(EvalError::NonFinite(op))
    }
}

/// Environnement d'évaluation : liste ordonnée de variables (une définition plus récente
/// masque une plus ancienne, comme une affectation dans un dictionnaire Python).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Env {
    vars: Vec<(String, Value)>,
}

impl Env {
    /// Environnement vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Définit (ou redéfinit) une variable.
    pub fn set(&mut self, name: &str, value: Value) {
        if let Some(slot) = self.vars.iter_mut().find(|(n, _)| n == name) {
            slot.1 = value;
        } else {
            self.vars.push((name.to_string(), value));
        }
    }

    /// Définit une variable numérique.
    pub fn set_num(&mut self, name: &str, value: f64) {
        self.set(name, Value::Num(value));
    }

    /// Lit une variable.
    pub fn get(&self, name: &str) -> Option<Value> {
        self.vars.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(j: Json, scope: &[&str]) -> Result<Expr, Vec<Issue>> {
        let mut issues = Vec::new();
        match Expr::parse(&j, "", scope, &mut issues) {
            Some(e) if issues.is_empty() => Ok(e),
            _ => Err(issues),
        }
    }

    fn ev(j: Json) -> Result<Value, EvalError> {
        let mut env = Env::new();
        env.set_num("s", 0.5);
        env.set_num("z", 0.0);
        parse(j, &["s", "z"]).expect("parse").eval(&env)
    }

    #[test]
    fn arithmetic_matches_reference() {
        assert_eq!(ev(json!({"add": [1, 2, "s"]})), Ok(Value::Num(3.5)));
        assert_eq!(ev(json!({"mul": [2, "s", 3]})), Ok(Value::Num(3.0)));
        assert_eq!(ev(json!({"sub": [1, "s"]})), Ok(Value::Num(0.5)));
        assert_eq!(ev(json!({"div": [1, "s"]})), Ok(Value::Num(2.0)));
        assert_eq!(ev(json!({"pow": ["s", 2]})), Ok(Value::Num(0.25)));
        assert_eq!(ev(json!({"neg": "s"})), Ok(Value::Num(-0.5)));
        assert_eq!(ev(json!({"exp": [0]})), Ok(Value::Num(1.0)));
        assert_eq!(ev(json!({"clip": [5, 0, 1]})), Ok(Value::Num(1.0)));
        assert_eq!(
            ev(json!({"max": [0, {"sub": ["s", 0.8]}]})),
            Ok(Value::Num(0.0))
        );
    }

    #[test]
    fn booleans_and_laziness() {
        assert_eq!(ev(json!({"lt": ["s", 1]})), Ok(Value::Bool(true)));
        assert_eq!(ev(json!({"not": "z"})), Ok(Value::Bool(true)));
        // Booléen dans un calcul : 1/0 comme en Python.
        assert_eq!(
            ev(json!({"mul": [{"gt": ["s", 0]}, 3]})),
            Ok(Value::Num(3.0))
        );
        // Branche non prise : pas d'erreur (évaluation paresseuse).
        assert_eq!(
            ev(json!({"if": [false, {"div": [1, 0]}, 2]})),
            Ok(Value::Num(2.0))
        );
        assert_eq!(
            ev(json!({"and": [false, {"div": [1, 0]}]})),
            Ok(Value::Bool(false))
        );
        assert_eq!(ev(json!({"or": [true, {"log": 0}]})), Ok(Value::Bool(true)));
    }

    #[test]
    fn evaluation_errors_are_typed() {
        assert_eq!(
            ev(json!({"div": [1, "z"]})),
            Err(EvalError::DivisionByZero("div"))
        );
        assert_eq!(ev(json!({"log": "z"})), Err(EvalError::LogDomain(0.0)));
        assert_eq!(
            ev(json!({"pow": ["z", -1]})),
            Err(EvalError::DivisionByZero("pow"))
        );
        assert_eq!(
            ev(json!({"pow": [-8, 0.5]})),
            Err(EvalError::NonFinite("pow"))
        );
        assert_eq!(ev(json!({"exp": 1000})), Err(EvalError::NonFinite("exp")));
        assert_eq!(
            ev(json!({"mul": [1e308, 10]})),
            Err(EvalError::NonFinite("mul"))
        );
    }

    #[test]
    fn parse_errors_carry_paths() {
        let e = parse(json!({"add": [1, {"mul": ["s", "q"]}]}), &["s"]).unwrap_err();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].path, "/add/1/mul/1");
        assert!(e[0].message.contains("« q »"));

        let e = parse(json!({"foo": [1]}), &[]).unwrap_err();
        assert!(e[0].message.contains("opérateur inconnu"));
        let e = parse(json!({"sub": [1]}), &[]).unwrap_err();
        assert_eq!(e[0].path, "/sub");
        let e = parse(json!({"add": []}), &[]).unwrap_err();
        assert!(e[0].message.contains("au moins un"));
        let e = parse(json!({"add": 1, "mul": 2}), &[]).unwrap_err();
        assert!(e[0].message.contains("une seule clé"));
        assert!(parse(json!(null), &[]).is_err());
        assert!(parse(json!([1, 2]), &[]).is_err());
        // Plusieurs erreurs sont toutes rapportées.
        let e = parse(json!({"add": ["a", "b"]}), &[]).unwrap_err();
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn env_redefinition_masks() {
        let mut env = Env::new();
        env.set_num("x", 1.0);
        env.set_num("x", 2.0);
        assert_eq!(env.get("x"), Some(Value::Num(2.0)));
        assert_eq!(env.get("y"), None);
    }
}
