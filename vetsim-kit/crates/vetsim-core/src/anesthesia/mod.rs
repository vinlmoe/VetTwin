//! Anesthésie générale en temps réel : de la prémédication au réveil (docs/ANESTHESIE.tex).
//!
//! Le moteur cardiovasculaire et métabolique ([`crate::evaluate`]) est quasi stationnaire et
//! reste la référence. Une [`Session`] porte les états dynamiques qui lui manquent et avance par
//! pas de temps ; à chaque pas :
//! 1. les états (PK et sites effet, halogénés, stocks de CO2 et d'O2, nociception, température,
//!    remplissage) donnent des paramètres modifiés (effets JSON par le DSL) ;
//! 2. l'évaluation existante calcule baroréflexe, hémodynamique, gaz et décompensation ;
//! 3. le débit cardiaque et la ventilation alvéolaire de cette évaluation font avancer les
//!    états jusqu'au pas suivant (RK4 à pas fixe, déterministe).
//!
//! **Stocks de gaz.** `C·dPaCO2/dt = VCO2 − VA·PaCO2/0,863` et
//! `V_CRF·dPAO2/dt = VA·(PIO2 − PAO2) − 0,863·VCO2/QR` ont pour équilibre exact les relations
//! algébriques de `gas.rs`. L'évaluation reçoit une commande ventilatoire et une FiO2
//! *équivalentes* qui lui font retrouver les PaCO2 et PAO2 courantes ; à l'équilibre, ce sont
//! les valeurs du patient : une session sans médicament reproduit `eval`.
//!
//! **Profondeur.** Indice hypnotique `H = Σ P_cerveau/MAC + Σ Ce/c_imm` (équivalents MAC) ;
//! profondeur `Δ = H/((1 − R)·f_T)` avec `R` l'épargne de MAC combinée et `f_T` l'effet de
//! l'hypothermie ; conscience perdue quand `H/((1 − Sd)·f_T)` dépasse `loc_mac`, `Sd` étant la
//! sédation combinée (les sédatifs réduisent la dose d'hypnotique nécessaire à l'induction).
//!
//! **Douleur.** Stimulus `N` (scénario chirurgical) atténué par les blocs locorégionaux et
//! amplifié par la sensibilisation `S` ; nociception nette `ν = N·(1 − A_méd)·(1 − A_prof)` ;
//! ses effets (sympathique, VO2, ventilation, glycémie) sont des effets JSON (`NU`, `STRESS`,
//! `S`). Mouvement si `u < 1/(1 + (Δ/N)^γ)` (tirage `u` à chaque stimulus, graine explicite).
//!
//! Usage pédagogique uniquement ; aucun résultat ne sert à décider d'une anesthésie réelle.

pub mod agent;
pub mod modules;

pub use modules::{AnesthesiaModel, InhalantModule, ProcedureModule, Step, TISSUES};

use crate::case::{apply_modifiers, evaluate_effects};
use crate::dsl::Env;
use crate::engine::{evaluate, Evaluation};
use crate::error::Error;
use crate::gas::stewart_ph;
use crate::math;
use crate::modules::{DrugModule, Effect};
use crate::params::Params;
use crate::pkpd::{pk_constants, PkConstants};
use crate::rng::SimRng;

/// Voie d'administration d'un bolus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminRoute {
    /// Intraveineuse.
    Iv,
    /// Intramusculaire.
    Im,
    /// Sous-cutanée.
    Sc,
}

/// Circuit anesthésique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Circuit {
    /// Circuit filtre (réinhalation) : la concentration inspirée suit le débit de gaz frais.
    Circle,
    /// Circuit sans réinhalation : concentration inspirée ≈ réglage du vaporisateur.
    NonRebreathing,
}

/// Mode ventilatoire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ventilation {
    /// Ventilation spontanée.
    Spontaneous,
    /// Ventilation contrôlée (intubé) : fréquence (/min) et volume courant (mL/kg).
    Controlled {
        /// Fréquence (/min).
        rr: f64,
        /// Volume courant (mL/kg).
        vt_ml_kg: f64,
    },
}

/// Action de l'utilisateur ou du scénario.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Bolus (mg/kg).
    Bolus {
        /// Agent.
        agent: String,
        /// Dose (mg/kg).
        mg_per_kg: f64,
        /// Voie.
        route: AdminRoute,
        /// Durée d'injection IV (s) ; `None` : `iv_push_s` du modèle. Ignorée en IM/SC.
        duration_s: Option<f64>,
    },
    /// Perfusion continue (mg/kg/h ; 0 l'arrête).
    Cri {
        /// Agent.
        agent: String,
        /// Débit (mg/kg/h).
        mg_per_kg_h: f64,
    },
    /// Vaporisateur (un seul agent ouvert à la fois ; 0 le ferme).
    Vaporizer {
        /// Agent inhalé.
        agent: String,
        /// Réglage (%).
        pct: f64,
    },
    /// Débit de gaz frais (L/min).
    FreshGas(f64),
    /// Type de circuit.
    SetCircuit(Circuit),
    /// Intubation et branchement au circuit.
    Intubate,
    /// Extubation.
    Extubate,
    /// Mode ventilatoire.
    SetVentilation(Ventilation),
    /// Oxygène au masque / flow-by (patient non intubé).
    Oxygen(bool),
    /// Stimulus nociceptif.
    Stimulus {
        /// Intensité (1 = stimulus supramaximal de la MAC).
        intensity: f64,
        /// Durée (s).
        duration_s: f64,
        /// Territoire.
        territory: String,
    },
    /// Temps opératoire du scénario chargé (indice).
    ProcedureStep(usize),
    /// Fin de chirurgie : début de la douleur postopératoire du scénario.
    EndSurgery,
    /// Bloc locorégional (agent à bloc `local`) sur un territoire.
    LocalBlock {
        /// Agent.
        agent: String,
        /// Territoire.
        territory: String,
    },
    /// Perfusion de cristalloïde (mL/kg/h).
    Fluids(f64),
    /// Bolus de cristalloïde (mL/kg).
    FluidBolus(f64),
    /// Hémorragie (mL/kg).
    Hemorrhage(f64),
    /// Réchauffement actif.
    Warming(bool),
}

/// Plan d'anesthésie (classification du modèle, seuils de `anesthesia_model.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plane {
    /// Vigile.
    Awake,
    /// Sédation (conscient).
    Sedated,
    /// Plan léger (inconscient, sous le plan chirurgical).
    Light,
    /// Plan chirurgical.
    Surgical,
    /// Plan profond.
    Deep,
    /// Surdosage.
    Excessive,
}

impl Plane {
    /// Identifiant.
    pub fn as_str(self) -> &'static str {
        match self {
            Plane::Awake => "awake",
            Plane::Sedated => "sedated",
            Plane::Light => "light",
            Plane::Surgical => "surgical",
            Plane::Deep => "deep",
            Plane::Excessive => "excessive",
        }
    }
}

/// Signes cliniques de profondeur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reflexes {
    /// Réflexe palpébral présent.
    pub palpebral: bool,
    /// Globe en rotation ventromédiale.
    pub eye_ventromedial: bool,
    /// Tonus de la mâchoire (0 absent, 1 diminué, 2 présent).
    pub jaw_tone: u8,
    /// Déglutition présente (critère d'extubation).
    pub swallowing: bool,
    /// Mydriase.
    pub mydriasis: bool,
    /// Mouvement en réponse au stimulus en cours.
    pub movement: bool,
}

/// Concentrations d'un agent IV/IM.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentLevel {
    /// Identifiant.
    pub id: String,
    /// Concentration plasmatique (mg/L).
    pub cp: f64,
    /// Concentration au site effet (mg/L).
    pub ce: f64,
    /// Débit de perfusion en cours (mg/kg/h).
    pub cri_mg_kg_h: f64,
}

/// Concentrations d'un halogéné.
#[derive(Debug, Clone, PartialEq)]
pub struct InhalantLevel {
    /// Identifiant.
    pub id: String,
    /// Fraction inspirée (%).
    pub fi: f64,
    /// Fraction télé-expiratoire ≈ alvéolaire (%).
    pub fe: f64,
    /// Pression cérébrale / MAC.
    pub brain_mac: f64,
    /// Réglage du vaporisateur (%).
    pub vaporizer: f64,
}

/// Grandeurs de profondeur et de nociception.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Depth {
    /// Indice hypnotique (équivalents MAC).
    pub hypnotic: f64,
    /// Épargne de MAC combinée (fraction).
    pub mac_reduction: f64,
    /// Profondeur Δ (équivalents MAC).
    pub delta: f64,
    /// Indice de perte de conscience `H/((1 − Sd)·f_T)`.
    pub loc_index: f64,
    /// Sédation (0–1).
    pub sedation: f64,
    /// Niveau de conscience (0 inconscient – 1 vigile).
    pub awareness: f64,
    /// Plan.
    pub plane: Plane,
    /// Fraction dissociative de l'hypnose (kétamine).
    pub dissociative: f64,
}

/// Nociception.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nociception {
    /// Stimulus effectif (après blocs et sensibilisation).
    pub stimulus: f64,
    /// Antinociception médicamenteuse (0–1).
    pub antinociception: f64,
    /// Nociception nette filtrée ν.
    pub nu: f64,
    /// État de stress (filtre lent de ν).
    pub stress: f64,
    /// Sensibilisation centrale S (0–1).
    pub sensitization: f64,
    /// Indice de douleur du modèle (0–10) si le patient est conscient.
    pub pain_score: Option<f64>,
}

/// État affiché à un instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Temps (s).
    pub t_s: f64,
    /// Évaluation du moteur (paramètres effectifs, hémodynamique, gaz) ; `None` après décès.
    pub eval: Option<Evaluation>,
    /// PaCO2 dynamique (mmHg).
    pub paco2: f64,
    /// PAO2 dynamique (mmHg).
    pub pao2_alv: f64,
    /// EtCO2 (mmHg ; 0 en apnée).
    pub etco2: f64,
    /// Ventilation alvéolaire réelle (L/min).
    pub va: f64,
    /// Fréquence respiratoire (/min).
    pub rr: f64,
    /// FiO2.
    pub fio2: f64,
    /// Température (°C).
    pub temp_c: f64,
    /// Agents IV/IM.
    pub agents: Vec<AgentLevel>,
    /// Halogénés.
    pub inhalants: Vec<InhalantLevel>,
    /// Profondeur.
    pub depth: Depth,
    /// Réflexes.
    pub reflexes: Reflexes,
    /// Nociception.
    pub nociception: Nociception,
    /// Intubé.
    pub intubated: bool,
    /// Ventilation contrôlée.
    pub controlled: bool,
    /// Apnée (ventilation alvéolaire < 10 % de la valeur de repos).
    pub apnea: bool,
    /// Alarmes.
    pub alarms: Vec<&'static str>,
    /// Agitation au réveil (douleur probable).
    pub agitation: bool,
    /// Cause du décès.
    pub dead: Option<&'static str>,
}

/// Ligne de la feuille d'anesthésie.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordRow {
    /// Temps (s).
    pub t_s: f64,
    /// FC.
    pub hr: f64,
    /// PAS, PAM, PAD.
    pub sap: f64,
    /// PAM.
    pub map: f64,
    /// PAD.
    pub dap: f64,
    /// SpO2.
    pub spo2: f64,
    /// EtCO2.
    pub etco2: f64,
    /// FR.
    pub rr: f64,
    /// Température.
    pub temp_c: f64,
    /// Fe de l'halogéné ouvert (%).
    pub fe_agent: f64,
    /// Profondeur.
    pub delta: f64,
    /// Plan.
    pub plane: Plane,
}

/// Jalons du réveil (s depuis le début de la session).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Milestones {
    /// Perte de conscience.
    pub loc: Option<f64>,
    /// Intubation.
    pub intubation: Option<f64>,
    /// Fermeture du vaporisateur (dernier halogéné).
    pub vaporizer_off: Option<f64>,
    /// Retour de la déglutition.
    pub swallowing: Option<f64>,
    /// Extubation.
    pub extubation: Option<f64>,
    /// Relevé de la tête (conscience > 0,5).
    pub head_lift: Option<f64>,
    /// Décubitus sternal (conscience > 0,8).
    pub sternal: Option<f64>,
}

#[derive(Debug, Clone)]
struct ActiveStimulus {
    intensity: f64,
    until_s: f64,
    territory: String,
    u: f64,
}

#[derive(Debug, Clone)]
struct ActiveBlock {
    territory: String,
    start_s: f64,
    block: agent::LocalBlock,
}

/// Agents chargés : agents IV/IM (modules médicaments) et halogénés.
#[derive(Debug, Clone)]
struct Pharm {
    agents: Vec<DrugModule>,
    pk: Vec<PkConstants>,
    inhalants: Vec<InhalantModule>,
}

const NA: usize = 5; // états par agent : IM, SC, central, périphérique, Ce
const NI: usize = 5; // états par halogéné : PI, PA, VRG, muscle, graisse

/// Résumé pharmacodynamique à un instant.
#[derive(Debug, Clone, Copy)]
struct PdSummary {
    hypnotic: f64,
    r: f64,
    sedation: f64,
    antinoc: f64,
    nmda: f64,
    dissociative: f64,
}

/// Session d'anesthésie (déterministe pour une graine donnée).
#[derive(Debug, Clone)]
pub struct Session {
    base: Params,
    ph: Pharm,
    model: AnesthesiaModel,
    procedure: Option<ProcedureModule>,
    rng: SimRng,
    t_s: f64,
    y: Vec<f64>,
    cri: Vec<f64>,
    vaporizer: Option<(usize, f64)>,
    fgf_l_min: f64,
    circuit: Circuit,
    intubated: bool,
    oxygen: bool,
    ventilation: Ventilation,
    stimuli: Vec<ActiveStimulus>,
    blocks: Vec<ActiveBlock>,
    postop_from_s: Option<f64>,
    fluids_ml_kg_h: f64,
    /// Dette d'O2 cumulée (mL).
    o2_debt_ml: f64,
    /// Injections IV en cours : (agent, débit mg/min, fin s).
    pushes: Vec<(usize, f64, f64)>,
    /// Débit d'injection IV par agent pendant le sous-pas courant (mg/min).
    push_now: Vec<f64>,
    blood_lost_ml: f64,
    warming: bool,
    va_rest: f64,
    last: Option<Evaluation>,
    dead: Option<&'static str>,
    record: Vec<RecordRow>,
    next_record_s: f64,
    milestones: Milestones,
    log: Vec<(f64, String)>,
}

fn idx_agent(j: usize) -> usize {
    NA * j
}

impl Session {
    /// Nouvelle session sur le patient `base` (après maladies). `agents` : modules médicaments
    /// utilisables (avec ou sans bloc `anesthesia`) ; `inhalants` : halogénés.
    pub fn new(
        base: Params,
        agents: Vec<DrugModule>,
        inhalants: Vec<InhalantModule>,
        model: AnesthesiaModel,
        procedure: Option<ProcedureModule>,
        seed: u64,
    ) -> Result<Self, Error> {
        let pk = agents.iter().map(|a| pk_constants(a, &base)).collect();
        let n = NA * agents.len() + NI * inhalants.len() + 7;
        let e0 = evaluate(&base)?;
        let mut y = vec![0.0; n];
        let o = n - 7;
        let paco2 = e0.metabolic.art.pco2;
        y[o] = paco2;
        y[o + 1] = base.fio2 * (base.pb - 47.0) - paco2 / base.rq;
        y[o + 5] = model.t0_c;
        let cri = vec![0.0; agents.len()];
        let n_agents = agents.len();
        let va_rest = e0.metabolic.art.va;
        Ok(Self {
            base,
            ph: Pharm {
                agents,
                pk,
                inhalants,
            },
            model,
            procedure,
            rng: SimRng::for_stream(seed, 0xA7E5),
            t_s: 0.0,
            y,
            cri,
            vaporizer: None,
            fgf_l_min: 0.0,
            circuit: Circuit::Circle,
            intubated: false,
            oxygen: false,
            ventilation: Ventilation::Spontaneous,
            stimuli: Vec::new(),
            blocks: Vec::new(),
            postop_from_s: None,
            fluids_ml_kg_h: 0.0,
            pushes: Vec::new(),
            o2_debt_ml: 0.0,
            push_now: vec![0.0; n_agents],
            blood_lost_ml: 0.0,
            warming: false,
            va_rest,
            last: Some(e0),
            dead: None,
            record: Vec::new(),
            next_record_s: 0.0,
            milestones: Milestones::default(),
            log: Vec::new(),
        })
    }

    /// Temps courant (s).
    pub fn time_s(&self) -> f64 {
        self.t_s
    }

    /// Feuille d'anesthésie (une ligne toutes les 5 min).
    pub fn record(&self) -> &[RecordRow] {
        &self.record
    }

    /// Journal des événements (temps, libellé).
    pub fn log(&self) -> &[(f64, String)] {
        &self.log
    }

    /// Jalons de l'anesthésie et du réveil.
    pub fn milestones(&self) -> Milestones {
        self.milestones
    }

    /// Scénario chargé.
    pub fn procedure(&self) -> Option<&ProcedureModule> {
        self.procedure.as_ref()
    }

    /// Cause du décès, le cas échéant.
    pub fn dead(&self) -> Option<&'static str> {
        self.dead
    }

    fn o(&self) -> usize {
        self.y.len() - 7
    }

    fn agent_index(&self, id: &str) -> Result<usize, Error> {
        self.ph
            .agents
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| Error::UnknownDrug(id.to_string()))
    }

    /// Applique une action.
    pub fn apply(&mut self, ev: Event) -> Result<(), Error> {
        let bw = self.base.bw;
        let pos = |x: f64, what: &str| {
            if x.is_finite() && x >= 0.0 {
                Ok(())
            } else {
                Err(Error::InvalidInput(format!("{what} invalide : {x}")))
            }
        };
        let label = match &ev {
            Event::Bolus {
                agent,
                mg_per_kg,
                route,
                duration_s,
            } => {
                pos(*mg_per_kg, "dose")?;
                let j = self.agent_index(agent)?;
                let a = self.ph.agents[j].anesthesia.as_ref();
                let i = idx_agent(j);
                let mg = mg_per_kg * bw;
                match route {
                    AdminRoute::Iv => {
                        let d = duration_s.unwrap_or(self.model.iv_push_s);
                        if !(d.is_finite() && d >= 0.0) {
                            return Err(Error::InvalidInput(format!("durée d'injection {d} s")));
                        }
                        if d == 0.0 {
                            self.y[i + 2] += mg;
                        } else {
                            // injection à débit constant (mg/min) jusqu'à t + d
                            self.pushes.push((j, mg / (d / 60.0), self.t_s + d));
                        }
                    }
                    AdminRoute::Im => {
                        let f = a.and_then(|a| a.im).ok_or_else(|| {
                            Error::InvalidInput(format!("voie IM non décrite pour {agent}"))
                        })?;
                        self.y[i] += mg * f.f;
                    }
                    AdminRoute::Sc => {
                        let f = a.and_then(|a| a.sc).ok_or_else(|| {
                            Error::InvalidInput(format!("voie SC non décrite pour {agent}"))
                        })?;
                        self.y[i + 1] += mg * f.f;
                    }
                }
                let r = match route {
                    AdminRoute::Iv => "IV",
                    AdminRoute::Im => "IM",
                    AdminRoute::Sc => "SC",
                };
                format!("{agent} {mg_per_kg} mg/kg {r}")
            }
            Event::Cri { agent, mg_per_kg_h } => {
                pos(*mg_per_kg_h, "débit")?;
                let j = self.agent_index(agent)?;
                self.cri[j] = *mg_per_kg_h;
                format!("{agent} perfusion {mg_per_kg_h} mg/kg/h")
            }
            Event::Vaporizer { agent, pct } => {
                pos(*pct, "réglage")?;
                let k = self
                    .ph
                    .inhalants
                    .iter()
                    .position(|m| &m.id == agent)
                    .ok_or_else(|| Error::UnknownDrug(agent.clone()))?;
                let max = self.ph.inhalants[k].vaporizer_max_pct;
                let pct = pct.min(max);
                if pct == 0.0 {
                    if self.vaporizer.is_some() {
                        self.milestones.vaporizer_off = Some(self.t_s);
                    }
                    self.vaporizer = None;
                } else {
                    self.vaporizer = Some((k, pct));
                    self.milestones.vaporizer_off = None;
                }
                format!("vaporisateur {agent} {pct} %")
            }
            Event::FreshGas(l) => {
                pos(*l, "débit de gaz frais")?;
                self.fgf_l_min = *l;
                format!("gaz frais {l} L/min")
            }
            Event::SetCircuit(c) => {
                self.circuit = *c;
                match c {
                    Circuit::Circle => "circuit filtre".into(),
                    Circuit::NonRebreathing => "circuit sans réinhalation".into(),
                }
            }
            Event::Intubate => {
                // intubation impossible tant que la déglutition est présente (plan trop léger)
                let d = self.depth(&self.pd_summary());
                if !self.intubated && d.loc_index < self.model.swallow_mac {
                    return Err(Error::InvalidInput(
                        "intubation impossible : déglutition présente (plan trop léger)".into(),
                    ));
                }
                self.intubated = true;
                self.milestones.intubation.get_or_insert(self.t_s);
                "intubation".into()
            }
            Event::Extubate => {
                self.intubated = false;
                self.ventilation = Ventilation::Spontaneous;
                self.milestones.extubation = Some(self.t_s);
                "extubation".into()
            }
            Event::SetVentilation(v) => {
                if let Ventilation::Controlled { rr, vt_ml_kg } = v {
                    if !self.intubated {
                        return Err(Error::InvalidInput(
                            "ventilation contrôlée impossible sans intubation".into(),
                        ));
                    }
                    pos(*rr, "fréquence")?;
                    pos(*vt_ml_kg, "volume courant")?;
                }
                self.ventilation = *v;
                match v {
                    Ventilation::Spontaneous => "ventilation spontanée".into(),
                    Ventilation::Controlled { rr, vt_ml_kg } => {
                        format!("ventilation contrôlée {rr}/min, {vt_ml_kg} mL/kg")
                    }
                }
            }
            Event::Oxygen(on) => {
                self.oxygen = *on;
                if *on {
                    "oxygène au masque"
                } else {
                    "arrêt de l'oxygène au masque"
                }
                .into()
            }
            Event::Stimulus {
                intensity,
                duration_s,
                territory,
            } => {
                pos(*intensity, "intensité")?;
                pos(*duration_s, "durée")?;
                let u = self.rng.uniform01();
                self.stimuli.push(ActiveStimulus {
                    intensity: *intensity,
                    until_s: self.t_s + duration_s,
                    territory: territory.clone(),
                    u,
                });
                format!("stimulus {intensity} ({territory}, {duration_s} s)")
            }
            Event::ProcedureStep(i) => {
                let s = self
                    .procedure
                    .as_ref()
                    .and_then(|p| p.steps.get(*i))
                    .cloned()
                    .ok_or_else(|| Error::InvalidInput(format!("temps opératoire {i} inconnu")))?;
                let u = self.rng.uniform01();
                self.stimuli.push(ActiveStimulus {
                    intensity: s.intensity,
                    until_s: self.t_s + s.duration_s,
                    territory: s.territory.clone(),
                    u,
                });
                format!("chirurgie : {}", s.label.get("fr").unwrap_or(&s.id))
            }
            Event::EndSurgery => {
                if self.procedure.is_none() {
                    return Err(Error::InvalidInput("aucun scénario chargé".into()));
                }
                self.postop_from_s = Some(self.t_s);
                self.stimuli.clear();
                "fin de chirurgie".into()
            }
            Event::LocalBlock { agent, territory } => {
                let j = self.agent_index(agent)?;
                let block = self.ph.agents[j]
                    .anesthesia
                    .as_ref()
                    .and_then(|a| a.local)
                    .ok_or_else(|| {
                        Error::InvalidInput(format!("{agent} : pas d'usage locorégional décrit"))
                    })?;
                self.blocks.push(ActiveBlock {
                    territory: territory.clone(),
                    start_s: self.t_s,
                    block,
                });
                format!("bloc locorégional {agent} ({territory})")
            }
            Event::Fluids(r) => {
                pos(*r, "débit")?;
                self.fluids_ml_kg_h = *r;
                format!("cristalloïde {r} mL/kg/h")
            }
            Event::FluidBolus(v) => {
                pos(*v, "volume")?;
                let o = self.o();
                self.y[o + 6] += v * bw;
                format!("bolus de cristalloïde {v} mL/kg")
            }
            Event::Hemorrhage(v) => {
                pos(*v, "volume")?;
                self.blood_lost_ml += v * bw;
                format!("hémorragie {v} mL/kg")
            }
            Event::Warming(on) => {
                self.warming = *on;
                if *on {
                    "réchauffement actif"
                } else {
                    "arrêt du réchauffement"
                }
                .into()
            }
        };
        self.log.push((self.t_s, label));
        Ok(())
    }

    // ------------------------------------------------------------------ pharmacodynamie

    fn cp(&self, j: usize) -> f64 {
        self.y[idx_agent(j) + 2] / self.ph.pk[j].vc
    }

    fn ce(&self, j: usize) -> f64 {
        match self.ph.agents[j]
            .anesthesia
            .as_ref()
            .and_then(|a| a.ke0_per_min)
        {
            Some(_) => self.y[idx_agent(j) + 4],
            None => self.cp(j),
        }
    }

    /// Multiplicateur d'EC50 de l'agent `j` par les antagonistes présents.
    fn shift(&self, j: usize) -> f64 {
        let id = &self.ph.agents[j].id;
        let mut s = 1.0;
        for (k, a) in self.ph.agents.iter().enumerate() {
            if let Some(ant) = a.anesthesia.as_ref().and_then(|x| x.antagonist.as_ref()) {
                if ant.targets.iter().any(|t| t == id) {
                    s += self.ce(k).max(0.0) / ant.ki_mg_l;
                }
            }
        }
        s
    }

    fn temp_factor(&self) -> f64 {
        let dt = self.model.t0_c - self.y[self.o() + 5];
        f64::max(0.3, 1.0 - self.model.mac_per_c * dt.max(0.0))
    }

    fn pd_summary(&self) -> PdSummary {
        let mut hyp_iv = 0.0;
        let mut diss = 0.0;
        let (mut keep_r, mut keep_sed, mut keep_a, mut keep_n) = (1.0, 1.0, 1.0, 1.0);
        for (j, a) in self.ph.agents.iter().enumerate() {
            let Some(an) = a.anesthesia.as_ref() else {
                continue;
            };
            let ce = self.ce(j);
            if ce <= 0.0 {
                continue;
            }
            let sh = self.shift(j);
            if let Some(c) = an.c_imm_mg_l {
                let h = ce / (c * sh);
                hyp_iv += h;
                if an.is("dissociative") {
                    diss += h;
                }
            }
            if let Some(s) = an.mac_reduction {
                keep_r *= 1.0 - s.at(ce, sh);
            }
            if let Some(s) = an.sedation {
                keep_sed *= 1.0 - s.at(ce, sh);
            }
            if let Some(s) = an.antinociception {
                keep_a *= 1.0 - s.at(ce, sh);
            }
            if let Some(s) = an.antihyperalgesia {
                keep_n *= 1.0 - s.at(ce, sh);
            }
        }
        let off = NA * self.ph.agents.len();
        let hyp_gas: f64 = self
            .ph
            .inhalants
            .iter()
            .enumerate()
            .map(|(k, m)| self.y[off + NI * k + 2].max(0.0) / m.mac_pct)
            .sum();
        let hypnotic = hyp_iv + hyp_gas;
        PdSummary {
            hypnotic,
            r: 1.0 - keep_r,
            sedation: 1.0 - keep_sed,
            antinoc: 1.0 - keep_a,
            nmda: 1.0 - keep_n,
            dissociative: if hypnotic > 0.0 { diss / hypnotic } else { 0.0 },
        }
    }

    fn depth(&self, s: &PdSummary) -> Depth {
        let m = &self.model;
        let ft = self.temp_factor();
        let delta = s.hypnotic / ((1.0 - s.r).max(1e-3) * ft);
        let loc_index = s.hypnotic / ((1.0 - s.sedation).max(1e-3) * ft);
        let awareness = (1.0 - s.sedation) * (1.0 - loc_index / m.loc_mac).clamp(0.0, 1.0);
        let plane = if loc_index < m.loc_mac {
            if s.sedation > 0.3 {
                Plane::Sedated
            } else {
                Plane::Awake
            }
        } else if delta < m.surgical_mac {
            Plane::Light
        } else if delta < m.deep_mac {
            Plane::Surgical
        } else if delta < m.excessive_mac {
            Plane::Deep
        } else {
            Plane::Excessive
        };
        Depth {
            hypnotic: s.hypnotic,
            mac_reduction: s.r,
            delta,
            loc_index,
            sedation: s.sedation,
            awareness,
            plane,
            dissociative: s.dissociative,
        }
    }

    /// Fraction bloquée sur un territoire (blocs combinés `1 − Π(1 − b)`).
    fn block_on(&self, territory: &str) -> f64 {
        let t = self.t_s / 60.0;
        let mut keep = 1.0;
        for b in self.blocks.iter().filter(|b| b.territory == territory) {
            let x = t - b.start_s / 60.0;
            let on = b.block.onset_min.max(1e-6);
            let d = b.block.duration_min;
            let f = if x < 0.0 {
                0.0
            } else if x < on {
                x / on
            } else if x < on + d {
                1.0
            } else {
                (1.0 - (x - on - d) / on).max(0.0)
            };
            keep *= 1.0 - b.block.efficacy * f;
        }
        1.0 - keep
    }

    /// Stimulus effectif et mouvement.
    fn stimulus(&self, depth: &Depth) -> (f64, bool) {
        let amp = 1.0 + self.model.sens_gain * self.y[self.o() + 4];
        let mut n: f64 = 0.0;
        let mut moving = false;
        for s in &self.stimuli {
            let ne = s.intensity * (1.0 - self.block_on(&s.territory)) * amp;
            n = n.max(ne);
            if ne > 0.0 && depth.loc_index >= self.model.loc_mac {
                let p = 1.0 / (1.0 + math::pow(depth.delta / ne, self.model.move_hill));
                moving |= s.u < p;
            }
        }
        if let (Some(t0), Some(p)) = (self.postop_from_s, self.procedure.as_ref()) {
            let (i, tau_h, terr) = &p.postop;
            let x = i * math::exp(-(self.t_s - t0) / 3600.0 / tau_h);
            n = n.max(x * (1.0 - self.block_on(terr)) * amp);
        }
        (n, moving)
    }

    // ------------------------------------------------------------------ paramètres

    fn agent_effects(&self, q: &mut Params) -> Result<(), Error> {
        for (j, a) in self.ph.agents.iter().enumerate() {
            let i = idx_agent(j);
            if self.y[i..i + NA].iter().all(|x| *x == 0.0) {
                continue;
            }
            let sh = self.shift(j);
            let pd = &a.pd;
            let emax = |c: f64| {
                let x = math::pow(c.max(0.0), pd.hill);
                pd.emax * x / (math::pow(pd.ec50_mg_l * sh, pd.hill) + x)
            };
            let cp = self.cp(j);
            let mut env = Env::new();
            env.set_num("E", emax(self.ce(j)));
            env.set_num("EP", emax(cp));
            apply(q, &a.pd.effects, &env, &a.id)?;
            for t in &a.pd.toxicity {
                let mut env = Env::new();
                env.set_num("T", crate::pkpd::toxic_effect(t, cp));
                apply(q, &t.effects, &env, &a.id)?;
            }
        }
        let off = NA * self.ph.agents.len();
        for (k, m) in self.ph.inhalants.iter().enumerate() {
            let i = off + NI * k;
            if self.y[i + 1] == 0.0 && self.y[i + 2] == 0.0 {
                continue;
            }
            let mut env = Env::new();
            env.set_num("M", self.y[i + 1].max(0.0) / m.mac_pct);
            env.set_num("MB", self.y[i + 2].max(0.0) / m.mac_pct);
            apply(q, &m.effects, &env, &m.id)?;
        }
        Ok(())
    }

    /// Paramètres du patient à l'état courant, avant la substitution ventilatoire.
    fn build_params(&self) -> Result<Params, Error> {
        let b = &self.base;
        let o = self.o();
        let mut q = b.clone();
        // remplissage et pertes sanguines
        let vf = self.y[o + 6];
        let bv = self.model.blood_volume_ml_kg * b.bw;
        if vf != 0.0 || self.blood_lost_ml != 0.0 {
            q.vol_shift += vf - self.blood_lost_ml;
            let rbc = (bv - self.blood_lost_ml).max(0.05 * bv);
            q.hb *= rbc / (rbc + vf);
        }
        self.agent_effects(&mut q)?;
        let mut env = Env::new();
        env.set_num("NU", self.y[o + 2]);
        env.set_num("STRESS", self.y[o + 3]);
        env.set_num("S", self.y[o + 4]);
        env.set_num("BW", b.bw);
        apply(
            &mut q,
            &self.model.nociception_effects,
            &env,
            "anesthesia_model",
        )?;
        let mut env = Env::new();
        env.set_num("DT", self.y[o + 5] - self.model.t0_c);
        apply(
            &mut q,
            &self.model.temperature_effects,
            &env,
            "anesthesia_model",
        )?;
        // dette d'O2 : dépression myocardique et bradycardie hypoxiques (variable DEBT 0–1)
        let mut env = Env::new();
        env.set_num("DEBT", self.debt_frac());
        apply(
            &mut q,
            &self.model.hypoxia_effects,
            &env,
            "anesthesia_model",
        )?;
        Ok(q)
    }

    /// Fraction de la dette d'O2 tolérée (0–1).
    fn debt_frac(&self) -> f64 {
        (self.o2_debt_ml / (self.model.o2_debt_tol_ml_kg * self.base.bw)).clamp(0.0, 1.0)
    }

    /// Dette d'O2 : elle s'accroît quand l'apport passe sous l'apport critique (consommation
    /// dépendante de l'apport) ou sous la consommation, et se rembourse sinon. Vrai si la dette
    /// tolérée est dépassée (décès).
    fn account_o2(&mut self, e: &Evaluation, dt_s: f64) -> bool {
        let bw = self.base.bw;
        let vo2 = e.params.vo2 * bw;
        let do2 = e.metabolic.do2;
        let crit = self.model.do2_crit_ml_kg_min * bw;
        let deficit = f64::max(vo2 - do2, vo2 * (1.0 - do2 / crit)).max(0.0);
        if deficit > 0.0 {
            self.o2_debt_ml += deficit * dt_s / 60.0;
        } else {
            self.o2_debt_ml *= math::exp(-dt_s / 60.0 / self.model.o2_debt_tau_min);
        }
        self.o2_debt_ml > self.model.o2_debt_tol_ml_kg * bw
    }

    fn fio2_true(&self) -> f64 {
        if self.intubated {
            let off = NA * self.ph.agents.len();
            let agent: f64 = (0..self.ph.inhalants.len())
                .map(|k| self.y[off + NI * k])
                .sum();
            (1.0 - agent / 100.0).max(0.0)
        } else if self.oxygen {
            self.model.flowby_fio2
        } else {
            self.base.fio2
        }
    }

    /// Ventilation alvéolaire réelle (L/min) pour les paramètres `q` à PaCO2 `p`.
    fn va_true(&self, q: &Params, p: f64, lactate: f64, pao2: f64) -> Result<f64, Error> {
        match self.ventilation {
            Ventilation::Controlled { rr, vt_ml_kg } => {
                Ok(rr * (vt_ml_kg - self.model.vd_ml_kg).max(0.0) * q.bw / 1000.0)
            }
            Ventilation::Spontaneous => {
                let (ph, _) = stewart_ph(q, p, lactate)?;
                Ok(q.va0 * q.bw * q.vent_drive * f_vent(q) * chemo(p, ph, pao2))
            }
        }
    }

    // ------------------------------------------------------------------ pas de temps

    /// Avance de `dt_s` secondes et renvoie l'état au **début** du pas (évaluation à l'instant
    /// courant), puis intègre les états jusqu'à `t + dt`.
    pub fn step(&mut self, dt_s: f64) -> Result<Snapshot, Error> {
        if !(dt_s.is_finite() && dt_s > 0.0 && dt_s <= 60.0) {
            return Err(Error::InvalidInput(format!(
                "pas {dt_s} s invalide (0–60 s)"
            )));
        }
        let o = self.o();
        let pd = self.pd_summary();
        let depth = self.depth(&pd);
        let (n_eff, moving) = self.stimulus(&depth);
        let a_depth = {
            let x = math::pow(depth.delta.max(0.0), self.model.bar_hill);
            x / (math::pow(self.model.mac_bar, self.model.bar_hill) + x)
        };
        let nu_target = n_eff * (1.0 - pd.antinoc) * (1.0 - a_depth);
        let paco2 = self.y[o];
        let pao2_alv = self.y[o + 1].max(0.0);
        let fio2 = self.fio2_true();
        let mut snap_eval = None;
        let mut va = 0.0;
        let mut q_used = None;
        if self.dead.is_none() {
            let mut q = self.build_params()?;
            let (lact, pao2_last) = self
                .last
                .as_ref()
                .map(|e| (e.metabolic.lactate, e.metabolic.art.pao2))
                .unwrap_or((q.lact0, 90.0));
            va = self.va_true(&q, paco2, lact, pao2_last)?;
            // commande ventilatoire et FiO2 équivalentes : l'évaluation retrouve les PaCO2 et
            // PAO2 dynamiques
            let (ph, _) = stewart_ph(&q, paco2, lact)?;
            let va_needed = 0.863 * q.vco2 * q.bw / paco2;
            q.vent_drive = va_needed / (q.va0 * q.bw * f_vent(&q) * chemo(paco2, ph, pao2_last));
            q.fio2 = (pao2_alv + paco2 / q.rq) / (q.pb - 47.0);
            match evaluate(&q) {
                Ok(e) => {
                    if self.account_o2(&e, dt_s) {
                        self.dead = Some("hypoxic_arrest");
                        self.log.push((self.t_s, "décès : arrêt hypoxique".into()));
                    } else {
                        snap_eval = Some(e.clone());
                        self.last = Some(e);
                        q_used = Some(q);
                    }
                }
                // Hypoxie transitoire (VO2 > DO2) : le moteur stationnaire conclut au décès, mais
                // l'organisme tolère une dette d'O2 limitée (réserves, anaérobiose) ; le décès
                // n'est retenu qu'au-delà de `o2_debt_tol_ml_kg`.
                Err(Error::NonViable {
                    cause: "oxygen_debt",
                    last: Some(e),
                }) => {
                    if self.account_o2(&e, dt_s) {
                        self.dead = Some("hypoxic_arrest");
                        self.log.push((self.t_s, "décès : arrêt hypoxique".into()));
                    } else {
                        snap_eval = Some((*e).clone());
                        self.last = Some(*e);
                        q_used = Some(q);
                    }
                }
                Err(Error::NonViable { cause, .. }) => {
                    self.dead = Some(cause);
                    self.log.push((self.t_s, format!("décès : {cause}")));
                }
                Err(e) => return Err(e),
            }
        }
        let apnea = va < 0.1 * self.va_rest;
        let reflexes = self.reflexes(&depth, moving);
        let sens = self.y[o + 4];
        let nociception = Nociception {
            stimulus: n_eff,
            antinociception: pd.antinoc,
            nu: self.y[o + 2],
            stress: self.y[o + 3],
            sensitization: sens,
            pain_score: (depth.awareness > 0.3).then(|| (10.0 * self.y[o + 2]).min(10.0)),
        };
        let agitation = depth.awareness > 0.2 && depth.awareness < 0.9 && self.y[o + 2] > 0.4;
        self.track_milestones(&depth, &reflexes);
        let snap = self.snapshot(
            snap_eval,
            paco2,
            pao2_alv,
            va,
            fio2,
            depth,
            reflexes,
            nociception,
            apnea,
            agitation,
        );
        if self.t_s + 1e-9 >= self.next_record_s {
            self.push_record(&snap);
            self.next_record_s += 300.0;
        }
        // --- intégration des états jusqu'à t + dt
        if let (Some(q), Some(e)) = (q_used, self.last.clone()) {
            let co = e.hemo.summary.co;
            let inputs = Inputs {
                co,
                va,
                vco2: q.vco2 * q.bw,
                rq: q.rq,
                pio2: fio2 * (q.pb - 47.0),
                nu_target,
                nmda: pd.nmda,
                anesth: (depth.loc_index / self.model.loc_mac).clamp(0.0, 1.0),
                vo2_ratio: q.vo2 / self.base.vo2,
                pvap: self.vaporizer,
            };
            self.integrate(dt_s, &inputs);
        }
        self.t_s += dt_s;
        self.stimuli.retain(|s| s.until_s > self.t_s + 1e-9);
        self.pushes.retain(|p| p.2 > self.t_s + 1e-9);
        Ok(snap)
    }

    fn reflexes(&self, d: &Depth, moving: bool) -> Reflexes {
        let m = &self.model;
        let diss = d.dissociative > 0.5;
        let unconscious = d.loc_index >= m.loc_mac;
        Reflexes {
            palpebral: diss || d.delta < m.palpebral_mac,
            eye_ventromedial: !diss && unconscious && d.delta < m.deep_mac,
            jaw_tone: if d.delta < m.loc_mac {
                2
            } else if d.delta < m.jaw_mac {
                1
            } else {
                0
            },
            swallowing: d.loc_index < m.swallow_mac,
            mydriasis: d.delta >= m.excessive_mac,
            movement: moving,
        }
    }

    fn track_milestones(&mut self, d: &Depth, r: &Reflexes) {
        let t = self.t_s;
        let ms = &mut self.milestones;
        if d.loc_index >= self.model.loc_mac {
            ms.loc.get_or_insert(t);
            ms.swallowing = None;
            ms.head_lift = None;
            ms.sternal = None;
        } else if ms.loc.is_some() {
            if r.swallowing {
                ms.swallowing.get_or_insert(t);
            }
            if d.awareness > 0.5 {
                ms.head_lift.get_or_insert(t);
            }
            if d.awareness > 0.8 {
                ms.sternal.get_or_insert(t);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn snapshot(
        &self,
        eval: Option<Evaluation>,
        paco2: f64,
        pao2_alv: f64,
        va: f64,
        fio2: f64,
        depth: Depth,
        reflexes: Reflexes,
        nociception: Nociception,
        apnea: bool,
        agitation: bool,
    ) -> Snapshot {
        let o = self.o();
        let bw = self.base.bw;
        let controlled = matches!(self.ventilation, Ventilation::Controlled { .. });
        let rr = match self.ventilation {
            Ventilation::Controlled { rr, .. } => rr,
            Ventilation::Spontaneous if apnea => 0.0,
            Ventilation::Spontaneous => 20.0 * (va / (self.base.va0 * bw)).max(0.0).sqrt(),
        };
        let etco2 = if apnea {
            0.0
        } else {
            (paco2 - self.model.etco2_gap_mmhg).max(0.0)
        };
        let agents = self
            .ph
            .agents
            .iter()
            .enumerate()
            .map(|(j, a)| AgentLevel {
                id: a.id.clone(),
                cp: self.cp(j),
                ce: self.ce(j),
                cri_mg_kg_h: self.cri[j],
            })
            .collect();
        let off = NA * self.ph.agents.len();
        let inhalants = self
            .ph
            .inhalants
            .iter()
            .enumerate()
            .map(|(k, m)| {
                let i = off + NI * k;
                InhalantLevel {
                    id: m.id.clone(),
                    fi: if self.intubated { self.y[i] } else { 0.0 },
                    fe: self.y[i + 1],
                    brain_mac: self.y[i + 2] / m.mac_pct,
                    vaporizer: match self.vaporizer {
                        Some((kk, p)) if kk == k => p,
                        _ => 0.0,
                    },
                }
            })
            .collect();
        let temp_c = self.y[o + 5];
        let mut alarms = Vec::new();
        if let Some(e) = &eval {
            let h = &e.hemo.summary;
            if h.map < 60.0 {
                alarms.push("hypotension");
            }
            if h.sap > 180.0 {
                alarms.push("hypertension");
            }
            if e.params.hr < 0.5 * self.base.hr {
                alarms.push("bradycardia");
            }
            if e.params.hr > 1.6 * self.base.hr {
                alarms.push("tachycardia");
            }
            if e.metabolic.art.sao2 < 90.0 {
                alarms.push("desaturation");
            }
        }
        if apnea {
            alarms.push("apnea");
        }
        if paco2 > 60.0 {
            alarms.push("hypercapnia");
        }
        if temp_c < 36.0 {
            alarms.push("hypothermia");
        }
        if self.o2_debt_ml > 0.05 * self.model.o2_debt_tol_ml_kg * self.base.bw {
            alarms.push("tissue_hypoxia");
        }
        if reflexes.movement {
            alarms.push("movement");
        }
        if self.dead.is_some() {
            alarms.push("cardiac_arrest");
        }
        Snapshot {
            t_s: self.t_s,
            eval,
            paco2,
            pao2_alv,
            etco2,
            va,
            rr,
            fio2,
            temp_c,
            agents,
            inhalants,
            depth,
            reflexes,
            nociception,
            intubated: self.intubated,
            controlled,
            apnea,
            alarms,
            agitation,
            dead: self.dead,
        }
    }

    fn push_record(&mut self, s: &Snapshot) {
        let (hr, sap, map, dap, spo2) = match &s.eval {
            Some(e) => {
                let h = &e.hemo.summary;
                (e.params.hr, h.sap, h.map, h.dap, e.metabolic.art.sao2)
            }
            None => (0.0, 0.0, 0.0, 0.0, 0.0),
        };
        let fe_agent = s.inhalants.iter().map(|i| i.fe).fold(0.0, f64::max);
        self.record.push(RecordRow {
            t_s: s.t_s,
            hr,
            sap,
            map,
            dap,
            spo2,
            etco2: s.etco2,
            rr: s.rr,
            temp_c: s.temp_c,
            fe_agent,
            delta: s.depth.delta,
            plane: s.depth.plane,
        });
    }

    fn integrate(&mut self, dt_s: f64, u: &Inputs) {
        let n_sub = (dt_s / 0.5).ceil().max(1.0) as usize;
        let h = dt_s / 60.0 / n_sub as f64; // min
        for k in 0..n_sub {
            // injections IV : débit au milieu du sous-pas (constant pendant le sous-pas)
            let t_mid = self.t_s + (k as f64 + 0.5) * h * 60.0;
            self.push_now.iter_mut().for_each(|x| *x = 0.0);
            for &(j, rate, end) in &self.pushes {
                if t_mid < end {
                    self.push_now[j] += rate;
                }
            }
            let y = &self.y;
            let k1 = self.rhs(y, u);
            let y2: Vec<f64> = y.iter().zip(&k1).map(|(a, b)| a + 0.5 * h * b).collect();
            let k2 = self.rhs(&y2, u);
            let y3: Vec<f64> = y.iter().zip(&k2).map(|(a, b)| a + 0.5 * h * b).collect();
            let k3 = self.rhs(&y3, u);
            let y4: Vec<f64> = y.iter().zip(&k3).map(|(a, b)| a + h * b).collect();
            let k4 = self.rhs(&y4, u);
            let next: Vec<f64> = (0..y.len())
                .map(|i| y[i] + h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]))
                .collect();
            self.y = next;
            // quantités et pressions partielles non négatives
            let o = self.o();
            for (i, v) in self.y.iter_mut().enumerate() {
                if i < o + 2 && *v < 0.0 {
                    *v = 0.0;
                }
            }
        }
    }

    /// Dérivées (par minute).
    fn rhs(&self, y: &[f64], u: &Inputs) -> Vec<f64> {
        let bw = self.base.bw;
        let m = &self.model;
        let mut dy = vec![0.0; y.len()];
        for (j, a) in self.ph.agents.iter().enumerate() {
            let i = idx_agent(j);
            let k = &self.ph.pk[j];
            let an = a.anesthesia.as_ref();
            let ka_im = an.and_then(|x| x.im).map_or(0.0, |r| r.ka_per_h / 60.0);
            let ka_sc = an.and_then(|x| x.sc).map_or(0.0, |r| r.ka_per_h / 60.0);
            let (cl, q) = (k.cl / 60.0, k.q / 60.0); // L/min
            let input = self.cri[j] * bw / 60.0 + self.push_now[j]; // mg/min
            dy[i] = -ka_im * y[i];
            dy[i + 1] = -ka_sc * y[i + 1];
            dy[i + 2] = ka_im * y[i] + ka_sc * y[i + 1] + input - (cl + q) / k.vc * y[i + 2]
                + q / k.vp * y[i + 3];
            dy[i + 3] = q / k.vc * y[i + 2] - q / k.vp * y[i + 3];
            if let Some(ke0) = an.and_then(|x| x.ke0_per_min) {
                dy[i + 4] = ke0 * (y[i + 2] / k.vc - y[i + 4]);
            }
        }
        let off = NA * self.ph.agents.len();
        let vfrc = m.frc_ml_kg * bw / 1000.0; // L
        let tissues: Vec<(f64, f64)> = m
            .tissues
            .iter()
            .map(|t| (t.co_frac * u.co, t.vol_l_kg * bw))
            .collect();
        for (k, g) in self.ph.inhalants.iter().enumerate() {
            let i = off + NI * k;
            let pvap = match u.pvap {
                Some((kk, p)) if kk == k => p,
                _ => 0.0,
            };
            let (pi, pa) = (y[i], y[i + 1]);
            let p_insp = if self.intubated { pi } else { 0.0 };
            let pv: f64 = (0..3).map(|t| m.tissues[t].co_frac * y[i + 2 + t]).sum();
            let uptake = g.blood_gas * u.co * (pa - pv); // %·L/min
            let exchange = u.va * (p_insp - pa);
            dy[i] = match self.circuit {
                Circuit::Circle => {
                    let rebreath = if self.intubated { exchange } else { 0.0 };
                    (self.fgf_l_min * (pvap - pi) - rebreath) / m.circuit_l
                }
                // circuit sans réinhalation : volume interne négligeable (constante 0,1 min)
                Circuit::NonRebreathing => (pvap - pi) / 0.1,
            };
            dy[i + 1] = (exchange - uptake) / vfrc;
            for (t, (qt, vt)) in tissues.iter().enumerate() {
                dy[i + 2 + t] = qt / (vt * g.tissue_blood[t]) * (pa - y[i + 2 + t]);
            }
        }
        let o = y.len() - 7;
        let (paco2, pao2) = (y[o], y[o + 1]);
        dy[o] = (u.vco2 - u.va * paco2 / 0.863) / (m.co2_store_ml_mmhg_kg * bw);
        dy[o + 1] = (u.va * (u.pio2 - pao2) - 0.863 * u.vco2 / u.rq) / vfrc;
        dy[o + 2] = (u.nu_target - y[o + 2]) / (m.tau_nu_s / 60.0);
        dy[o + 3] = (y[o + 2] - y[o + 3]) / m.tau_stress_min;
        dy[o + 4] = m.sens_rate_per_min * y[o + 2] * (1.0 - y[o + 4]) * (1.0 - u.nmda)
            - y[o + 4] / (m.sens_tau_h * 60.0);
        let loss = 1.0 + (m.loss_gain_anesth - 1.0) * u.anesth;
        let warm = if self.warming {
            m.warming_c_per_h / 60.0
        } else {
            0.0
        };
        dy[o + 5] = ((m.t0_c - m.t_amb_c) * u.vo2_ratio - loss * (y[o + 5] - m.t_amb_c))
            / (m.tau_cool_h * 60.0)
            + warm;
        dy[o + 6] = self.fluids_ml_kg_h * bw / 60.0 - y[o + 6] / m.crystalloid_tau_min;
        dy
    }
}

/// Entrées tenues constantes pendant un pas.
struct Inputs {
    co: f64,
    va: f64,
    vco2: f64,
    rq: f64,
    pio2: f64,
    nu_target: f64,
    nmda: f64,
    anesth: f64,
    vo2_ratio: f64,
    pvap: Option<(usize, f64)>,
}

fn apply(q: &mut Params, effects: &[Effect], env: &Env, module: &str) -> Result<(), Error> {
    if effects.is_empty() {
        return Ok(());
    }
    let mods = evaluate_effects(effects, env, module)?;
    apply_modifiers(q, &mods).map_err(|e| match e {
        Error::Eval {
            location, source, ..
        } => Error::Eval {
            module: module.to_string(),
            location,
            source,
        },
        other => other,
    })
}

/// Faiblesse ventilatoire hypokaliémique (même formule que `engine::evaluate`).
fn f_vent(q: &Params) -> f64 {
    f64::max(0.1, 1.0 - q.k_weak_vent * f64::max(0.0, q.k_weak - q.k))
}

/// Stimulation chimique de la ventilation (même formule que `gas::gas_exchange`).
fn chemo(pco2: f64, ph: f64, pao2: f64) -> f64 {
    f64::max(0.12, 1.0 + 0.08 * (pco2 - 37.0))
        * math::exp(-2.5 * (ph - 7.4))
        * (1.0 + 0.03 * f64::max(0.0, 70.0 - pao2))
}
