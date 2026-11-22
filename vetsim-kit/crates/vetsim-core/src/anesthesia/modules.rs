//! Modules JSON propres à l'anesthésie : agents halogénés (`inhalant.schema.json`), constantes
//! du modèle (`anesthesia_model.schema.json`) et scénarios chirurgicaux
//! (`procedure.schema.json`). Même validation que les autres modules : tous les problèmes sont
//! rapportés ensemble, avec leur pointeur JSON.

use crate::error::LoadError;
use crate::modules::{child, parse_json, Ctx, Effect, Evidence, Localized};
use serde_json::{Map, Value as Json};

/// Compartiments tissulaires de l'absorption des halogénés (ordre fixe).
pub const TISSUES: [&str; 3] = ["vrg", "muscle", "fat"];

// ------------------------------------------------------------------------------- halogénés

/// Agent anesthésique inhalé.
#[derive(Debug, Clone, PartialEq)]
pub struct InhalantModule {
    /// Identifiant.
    pub id: String,
    /// Version.
    pub version: String,
    /// Nom.
    pub name: Localized,
    /// Niveau de preuve global.
    pub evidence: Evidence,
    /// MAC canine (% d'une atmosphère).
    pub mac_pct: f64,
    /// Coefficient de partage sang/gaz.
    pub blood_gas: f64,
    /// Coefficients de partage tissu/sang, dans l'ordre de [`TISSUES`].
    pub tissue_blood: [f64; 3],
    /// Réglage maximal du vaporisateur (%).
    pub vaporizer_max_pct: f64,
    /// Effets sur les paramètres (variables `M` : pression artérielle / MAC ; `MB` : cérébrale).
    pub effects: Vec<Effect>,
    /// Références.
    pub refs: Vec<String>,
    /// Notes.
    pub notes: Option<String>,
}

impl InhalantModule {
    /// Charge et valide un module halogéné.
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
        let pos = |x: f64| x > 0.0;
        let mac = c.req_num_in(m, "", "mac_pct", pos, "> 0");
        let bg = c.req_num_in(m, "", "blood_gas", pos, "> 0");
        let vmax = c.req_num_in(m, "", "vaporizer_max_pct", pos, "> 0");
        let mut tb = [0.0; 3];
        let mut tb_ok = true;
        match c
            .req(m, "", "tissue_blood")
            .and_then(|v| c.as_obj(v, "/tissue_blood"))
        {
            Some(o) => {
                for (i, t) in TISSUES.iter().enumerate() {
                    match c.req_num_in(o, "/tissue_blood", t, pos, "> 0") {
                        Some(x) => tb[i] = x,
                        None => tb_ok = false,
                    }
                }
            }
            None => tb_ok = false,
        }
        let effects = c.effects(m.get("effects"), "/effects", &["M", "MB"], false);
        let refs = c.strings(m, "refs");
        let notes = m.get("notes").and_then(Json::as_str).map(str::to_string);
        match (mac, bg, vmax, tb_ok) {
            (Some(mac_pct), Some(blood_gas), Some(vaporizer_max_pct), true) => c.finish(
                origin,
                Self {
                    id,
                    version,
                    name,
                    evidence,
                    mac_pct,
                    blood_gas,
                    tissue_blood: tb,
                    vaporizer_max_pct,
                    effects,
                    refs,
                    notes,
                },
            ),
            _ => Err(c.fail(origin)),
        }
    }
}

// ------------------------------------------------------------------------- constantes du modèle

/// Compartiment tissulaire : fraction du poids et fraction du débit cardiaque.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tissue {
    /// Volume (L/kg, densité 1).
    pub vol_l_kg: f64,
    /// Fraction du débit cardiaque.
    pub co_frac: f64,
}

/// Constantes du modèle d'anesthésie (toutes lues dans `anesthesia_model.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnesthesiaModel {
    /// Version.
    pub version: String,
    /// Capacité de stockage du CO2 (mL CO2 / mmHg / kg).
    pub co2_store_ml_mmhg_kg: f64,
    /// Capacité résiduelle fonctionnelle (mL/kg).
    pub frc_ml_kg: f64,
    /// Espace mort pour la ventilation contrôlée (mL/kg).
    pub vd_ml_kg: f64,
    /// Volume du circuit filtre (L).
    pub circuit_l: f64,
    /// Écart PaCO2 − EtCO2 (mmHg).
    pub etco2_gap_mmhg: f64,
    /// FiO2 au masque ou en flow-by d'oxygène.
    pub flowby_fio2: f64,
    /// Compartiments tissulaires (ordre de [`TISSUES`]).
    pub tissues: [Tissue; 3],
    /// Indice d'hypnose à la perte de conscience (équivalents MAC).
    pub loc_mac: f64,
    /// Retour de la déglutition (équivalents MAC) : critère d'extubation.
    pub swallow_mac: f64,
    /// Perte du réflexe palpébral (équivalents MAC).
    pub palpebral_mac: f64,
    /// Perte du tonus de la mâchoire (équivalents MAC).
    pub jaw_mac: f64,
    /// Début du plan chirurgical (équivalents MAC).
    pub surgical_mac: f64,
    /// Début du plan profond (équivalents MAC).
    pub deep_mac: f64,
    /// Plan excessif (équivalents MAC).
    pub excessive_mac: f64,
    /// MAC-BAR : équivalents MAC qui suppriment la moitié de la réponse autonome.
    pub mac_bar: f64,
    /// Pente de Hill de la réponse autonome en fonction de la profondeur.
    pub bar_hill: f64,
    /// Pente de la probabilité de mouvement (`1/(1 + (Δ/N)^γ)`).
    pub move_hill: f64,
    /// Constante de temps de la nociception nette (s).
    pub tau_nu_s: f64,
    /// Constante de temps de l'état de stress (min).
    pub tau_stress_min: f64,
    /// Vitesse de sensibilisation centrale (1/min par unité de nociception).
    pub sens_rate_per_min: f64,
    /// Constante de retour de la sensibilisation (h).
    pub sens_tau_h: f64,
    /// Amplification de la nociception par la sensibilisation (`1 + κ·S`).
    pub sens_gain: f64,
    /// Effets de la nociception (variables `NU`, `STRESS`, `S`, `BW` : poids en kg).
    pub nociception_effects: Vec<Effect>,
    /// Température de référence (°C).
    pub t0_c: f64,
    /// Température ambiante (°C).
    pub t_amb_c: f64,
    /// Constante de temps de refroidissement vigile (h).
    pub tau_cool_h: f64,
    /// Multiplicateur des pertes thermiques sous anesthésie (au plan chirurgical).
    pub loss_gain_anesth: f64,
    /// Apport du réchauffement actif (°C/h).
    pub warming_c_per_h: f64,
    /// Réduction de MAC par °C d'hypothermie (fraction).
    pub mac_per_c: f64,
    /// Effets de la température (variable `DT` = T − T0).
    pub temperature_effects: Vec<Effect>,
    /// Volume sanguin (mL/kg).
    pub blood_volume_ml_kg: f64,
    /// Constante de sortie vasculaire des cristalloïdes (min).
    pub crystalloid_tau_min: f64,
    /// Durée par défaut d'une injection IV (s).
    pub iv_push_s: f64,
    /// Dette d'O2 tolérée avant le décès (mL/kg).
    pub o2_debt_tol_ml_kg: f64,
    /// Constante de remboursement de la dette d'O2 (min).
    pub o2_debt_tau_min: f64,
    /// Durée cumulée tolérée sous `map_arrest` avant le choc irréversible (min).
    pub shock_tol_min: f64,
    /// Pente de la conscience sous la MAC-awake : `1 − (indice/loc_mac)^n`.
    pub awareness_hill: f64,
    /// Gradient A-a ajouté sous anesthésie en O2 pur (atélectasie, mmHg).
    pub atelectasis_aa_mmhg: f64,
    /// Température où l'excès de pertes thermiques de l'anesthésie s'éteint (°C).
    pub vasoconstriction_c: f64,
    /// Fraction du volume perdu restituée par remplissage transcapillaire.
    pub refill_frac: f64,
    /// Constante du remplissage transcapillaire (h).
    pub refill_tau_h: f64,
    /// Sang mobilisable par la rate (mL/kg).
    pub spleen_ml_kg: f64,
    /// Constante de la contraction splénique (min).
    pub spleen_tau_min: f64,
    /// Hausse de tonus sympathique qui vide entièrement la rate.
    pub spleen_s_span: f64,
    /// Hématocrite du sang splénique rapporté à l'hématocrite circulant.
    pub spleen_hct_ratio: f64,
    /// Hausse maximale des RVS par vasopressine et angiotensine II en hémorragie.
    pub pressor_gain: f64,
    /// Fraction du volume sanguin perdue à mi-effet presseur.
    pub pressor_lost_frac: f64,
    /// Pente de l'effet presseur.
    pub pressor_hill: f64,
    /// Fraction du volume sanguin dont le contenu en O2 suit l'alvéole (réserve sanguine).
    pub blood_o2_store_frac: f64,
    /// Apport d'O2 critique (mL/kg/min) sous lequel la consommation devient dépendante de l'apport.
    pub do2_crit_ml_kg_min: f64,
    /// Effets de la dette d'O2 (variable `DEBT`, fraction 0–1 de la dette tolérée).
    pub hypoxia_effects: Vec<Effect>,
    /// Niveau de preuve global du module.
    pub evidence: Evidence,
    /// Références.
    pub refs: Vec<String>,
}

impl AnesthesiaModel {
    /// Charge et valide le module de constantes.
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root = parse_json(json, origin)?;
        let mut c = Ctx::new();
        let Some(m) = c.as_obj(&root, "") else {
            return Err(c.fail(origin));
        };
        c.schema_version(m);
        let version = c.semver(m);
        let evidence = c.evidence(m);
        let refs = c.strings(m, "refs");
        let Some(v) = c.req(m, "", "values").and_then(|v| c.as_obj(v, "/values")) else {
            return Err(c.fail(origin));
        };
        let num = |c: &mut Ctx, k: &str| value(c, v, k);
        let co2 = num(&mut c, "co2_store_ml_mmhg_kg");
        let frc = num(&mut c, "frc_ml_kg");
        let vd = num(&mut c, "vd_ml_kg");
        let circ = num(&mut c, "circuit_l");
        let gap = num(&mut c, "etco2_gap_mmhg");
        let flowby = num(&mut c, "flowby_fio2");
        let loc = num(&mut c, "loc_mac");
        let swallow = num(&mut c, "swallow_mac");
        let palp = num(&mut c, "palpebral_mac");
        let jaw = num(&mut c, "jaw_mac");
        let surg = num(&mut c, "surgical_mac");
        let deep = num(&mut c, "deep_mac");
        let exc = num(&mut c, "excessive_mac");
        let bar = num(&mut c, "mac_bar");
        let barh = num(&mut c, "bar_hill");
        let mh = num(&mut c, "move_hill");
        let tnu = num(&mut c, "tau_nu_s");
        let tst = num(&mut c, "tau_stress_min");
        let sr = num(&mut c, "sens_rate_per_min");
        let st = num(&mut c, "sens_tau_h");
        let sg = num(&mut c, "sens_gain");
        let t0 = num(&mut c, "t0_c");
        let tamb = num(&mut c, "t_amb_c");
        let tcool = num(&mut c, "tau_cool_h");
        let lga = num(&mut c, "loss_gain_anesth");
        let warm = num(&mut c, "warming_c_per_h");
        let mpc = num(&mut c, "mac_per_c");
        let bv = num(&mut c, "blood_volume_ml_kg");
        let ctau = num(&mut c, "crystalloid_tau_min");
        let push = num(&mut c, "iv_push_s");
        let debt = num(&mut c, "o2_debt_tol_ml_kg");
        let dtau = num(&mut c, "o2_debt_tau_min");
        let stol = num(&mut c, "shock_tol_min");
        let ahill = num(&mut c, "awareness_hill");
        let atel = num(&mut c, "atelectasis_aa_mmhg");
        let vcon = num(&mut c, "vasoconstriction_c");
        let refill_frac = num(&mut c, "refill_frac");
        let refill_tau_h = num(&mut c, "refill_tau_h");
        let spleen_ml_kg = num(&mut c, "spleen_ml_kg");
        let spleen_tau_min = num(&mut c, "spleen_tau_min");
        let spleen_s_span = num(&mut c, "spleen_s_span");
        let spleen_hct_ratio = num(&mut c, "spleen_hct_ratio");
        let pressor_gain = num(&mut c, "pressor_gain");
        let pressor_lost_frac = num(&mut c, "pressor_lost_frac");
        let pressor_hill = num(&mut c, "pressor_hill");
        let dcrit = num(&mut c, "do2_crit_ml_kg_min");
        let bfrac = num(&mut c, "blood_o2_store_frac");
        let mut tissues = [Tissue {
            vol_l_kg: 0.0,
            co_frac: 0.0,
        }; 3];
        let mut t_ok = true;
        match c
            .req(m, "", "tissues")
            .and_then(|t| c.as_obj(t, "/tissues"))
        {
            Some(o) => {
                for (i, name) in TISSUES.iter().enumerate() {
                    let p = child("/tissues", name);
                    let Some(t) = c.req(o, "/tissues", name).and_then(|t| c.as_obj(t, &p)) else {
                        t_ok = false;
                        continue;
                    };
                    let vol = c.req_num_in(t, &p, "vol_L_kg", |x| x > 0.0, "> 0");
                    let cf = c.req_num_in(t, &p, "co_frac", |x| (0.0..=1.0).contains(&x), "0–1");
                    match (vol, cf) {
                        (Some(vol_l_kg), Some(co_frac)) => {
                            tissues[i] = Tissue { vol_l_kg, co_frac }
                        }
                        _ => t_ok = false,
                    }
                }
                let s: f64 = tissues.iter().map(|t| t.co_frac).sum();
                if t_ok && (s - 1.0).abs() > 1e-9 {
                    c.err(
                        "/tissues",
                        format!("la somme des co_frac vaut {s}, 1 attendu"),
                    );
                }
            }
            None => t_ok = false,
        }
        let ne = c.effects(
            m.get("nociception_effects"),
            "/nociception_effects",
            &["NU", "STRESS", "S", "BW"],
            false,
        );
        let te = c.effects(
            m.get("temperature_effects"),
            "/temperature_effects",
            &["DT"],
            false,
        );
        let he = c.effects(
            m.get("hypoxia_effects"),
            "/hypoxia_effects",
            &["DEBT"],
            false,
        );
        let all = [
            co2, frc, vd, circ, gap, flowby, loc, swallow, palp, jaw, surg, deep, exc, bar, barh,
            mh, tnu, tst, sr, st, sg, t0, tamb, tcool, lga, warm, mpc, bv, ctau, push, debt, dtau,
            dcrit, bfrac,
        ];
        if all.iter().any(Option::is_none) || !t_ok {
            return Err(c.fail(origin));
        }
        let g = |x: Option<f64>| x.expect("contrôlé");
        let out = Self {
            version,
            co2_store_ml_mmhg_kg: g(co2),
            frc_ml_kg: g(frc),
            vd_ml_kg: g(vd),
            circuit_l: g(circ),
            etco2_gap_mmhg: g(gap),
            flowby_fio2: g(flowby),
            tissues,
            loc_mac: g(loc),
            swallow_mac: g(swallow),
            palpebral_mac: g(palp),
            jaw_mac: g(jaw),
            surgical_mac: g(surg),
            deep_mac: g(deep),
            excessive_mac: g(exc),
            mac_bar: g(bar),
            bar_hill: g(barh),
            move_hill: g(mh),
            tau_nu_s: g(tnu),
            tau_stress_min: g(tst),
            sens_rate_per_min: g(sr),
            sens_tau_h: g(st),
            sens_gain: g(sg),
            nociception_effects: ne,
            t0_c: g(t0),
            t_amb_c: g(tamb),
            tau_cool_h: g(tcool),
            loss_gain_anesth: g(lga),
            warming_c_per_h: g(warm),
            mac_per_c: g(mpc),
            temperature_effects: te,
            blood_volume_ml_kg: g(bv),
            crystalloid_tau_min: g(ctau),
            iv_push_s: g(push),
            o2_debt_tol_ml_kg: g(debt),
            o2_debt_tau_min: g(dtau),
            shock_tol_min: g(stol),
            awareness_hill: g(ahill),
            atelectasis_aa_mmhg: g(atel),
            vasoconstriction_c: g(vcon),
            refill_frac: g(refill_frac),
            refill_tau_h: g(refill_tau_h),
            spleen_ml_kg: g(spleen_ml_kg),
            spleen_tau_min: g(spleen_tau_min),
            spleen_s_span: g(spleen_s_span),
            spleen_hct_ratio: g(spleen_hct_ratio),
            pressor_gain: g(pressor_gain),
            pressor_lost_frac: g(pressor_lost_frac),
            pressor_hill: g(pressor_hill),
            do2_crit_ml_kg_min: g(dcrit),
            blood_o2_store_frac: g(bfrac),
            hypoxia_effects: he,
            evidence,
            refs,
        };
        if !(out.swallow_mac < out.loc_mac
            && out.loc_mac <= out.palpebral_mac
            && out.surgical_mac < out.deep_mac
            && out.deep_mac < out.excessive_mac)
        {
            c.err(
                "/values",
                "seuils non ordonnés (swallow < loc ≤ palpebral ; surgical < deep < excessive)",
            );
        }
        c.finish(origin, out)
    }
}

/// Valeur `values.<key>` : objet `{"value": x, "evidence": ..., "source": ...}` ou nombre nu.
fn value(c: &mut Ctx, v: &Map<String, Json>, key: &str) -> Option<f64> {
    let p = child("/values", key);
    match c.req(v, "/values", key)? {
        Json::Object(o) => {
            if !o.contains_key("evidence") {
                c.err(
                    child(&p, "evidence"),
                    "niveau de preuve requis pour chaque valeur",
                );
            }
            c.req_num(o, &p, "value")
        }
        other => c.as_num(other, &p),
    }
}

// ------------------------------------------------------------------------------ procédures

/// Temps opératoire : stimulation nociceptive d'intensité donnée.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// Identifiant.
    pub id: String,
    /// Libellé.
    pub label: Localized,
    /// Intensité (1 = stimulus supramaximal de la définition de la MAC).
    pub intensity: f64,
    /// Durée (s).
    pub duration_s: f64,
    /// Territoire (pour les blocs locorégionaux).
    pub territory: String,
}

/// Scénario chirurgical.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcedureModule {
    /// Identifiant.
    pub id: String,
    /// Nom.
    pub name: Localized,
    /// Niveau de preuve.
    pub evidence: Evidence,
    /// Territoires.
    pub territories: Vec<String>,
    /// Temps opératoires, dans l'ordre.
    pub steps: Vec<Step>,
    /// Douleur postopératoire : intensité initiale, constante de décroissance (h), territoire.
    pub postop: (f64, f64, String),
    /// Notes.
    pub notes: Option<String>,
}

impl ProcedureModule {
    /// Charge et valide un scénario.
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root = parse_json(json, origin)?;
        let mut c = Ctx::new();
        let Some(m) = c.as_obj(&root, "") else {
            return Err(c.fail(origin));
        };
        c.schema_version(m);
        let id = c.req_str(m, "", "id").unwrap_or_default().to_string();
        c.semver(m);
        let name = c.text(m, "name", true);
        let evidence = c.evidence(m);
        let territories = c.strings(m, "territories");
        if territories.is_empty() {
            c.err("/territories", "au moins un territoire");
        }
        let mut steps = Vec::new();
        match m.get("steps") {
            Some(Json::Array(a)) => {
                for (i, s) in a.iter().enumerate() {
                    let p = format!("/steps/{i}");
                    let Some(o) = c.as_obj(s, &p) else { continue };
                    let sid = c.req_str(o, &p, "id").map(str::to_string);
                    let label = match o.get("label").and_then(Json::as_object) {
                        Some(l) => Localized(
                            l.iter()
                                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                .collect(),
                        ),
                        None => {
                            c.err(child(&p, "label"), "libellé requis");
                            Localized::default()
                        }
                    };
                    let it = c.req_num_in(o, &p, "intensity", |x| (0.0..=3.0).contains(&x), "0–3");
                    let du = c.req_num_in(o, &p, "duration_s", |x| x > 0.0, "> 0");
                    let te = c.req_str(o, &p, "territory").map(str::to_string);
                    if let Some(t) = &te {
                        if !territories.contains(t) {
                            c.err(
                                child(&p, "territory"),
                                format!("territoire « {t} » non déclaré"),
                            );
                        }
                    }
                    if let (Some(id), Some(intensity), Some(duration_s), Some(territory)) =
                        (sid, it, du, te)
                    {
                        steps.push(Step {
                            id,
                            label,
                            intensity,
                            duration_s,
                            territory,
                        });
                    }
                }
            }
            _ => c.err("/steps", "tableau requis"),
        }
        let postop = c
            .req(m, "", "postop")
            .and_then(|v| c.as_obj(v, "/postop"))
            .and_then(|o| {
                let i = c.req_num_in(
                    o,
                    "/postop",
                    "intensity",
                    |x| (0.0..=3.0).contains(&x),
                    "0–3",
                );
                let t = c.req_num_in(o, "/postop", "tau_h", |x| x > 0.0, "> 0");
                let te = c.req_str(o, "/postop", "territory").map(str::to_string);
                Some((i?, t?, te?))
            });
        let notes = m.get("notes").and_then(Json::as_str).map(str::to_string);
        match postop {
            Some(postop) => c.finish(
                origin,
                Self {
                    id,
                    name,
                    evidence,
                    territories,
                    steps,
                    postop,
                    notes,
                },
            ),
            None => Err(c.fail(origin)),
        }
    }
}
