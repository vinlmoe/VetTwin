//! Messages de l'anesthésie en temps réel (docs/ANESTHESIE.tex, § API).
//!
//! - `anesthesia_list` : agents, halogénés et scénarios chargés ;
//! - `anesthesia_start` `{patient, options: {seed, procedure}}` : nouvelle session sur le patient ;
//! - `anesthesia_event` `{options: {session, event: {type, ...}}}` : action ;
//! - `anesthesia_step` `{options: {session, dt_s, n, all, waves_s, pcg}}` : avance de `n` pas de
//!   `dt_s` s ; avec `waves_s`, courbes du moniteur sur l'état final ; avec `pcg` (`true` ou
//!   `{"site": "base"|"apex", "fs": 4000}`), auscultation en temps réel sur les mêmes battements ;
//! - `anesthesia_record` `{options: {session}}` : feuille d'anesthésie, journal et jalons.

use super::{obj, opt_bool, opt_f64, opt_u64, ApiError, ApiResult, Engine};
use serde_json::{json, Map, Value as Json};
use vetsim_core::anesthesia::{
    AdminRoute, AnesthesiaModel, Circuit, Event, InhalantModule, ProcedureModule, Session,
    Snapshot, Ventilation,
};
use vetsim_core::DrugModule;

/// Modules de l'anesthésie.
#[derive(Debug, Clone)]
pub(crate) struct AnesthesiaCatalog {
    model: AnesthesiaModel,
    agents: Vec<DrugModule>,
    inhalants: Vec<InhalantModule>,
    procedures: Vec<ProcedureModule>,
}

/// Lit le bloc `anesthesia` de `loadModules` :
/// `{"model": {...}, "agents": [...], "inhalants": [...], "procedures": [...]}`.
pub(crate) fn load(v: &Json, errors: &mut Vec<(String, String)>) -> Option<AnesthesiaCatalog> {
    let Some(m) = v.as_object() else {
        errors.push(("/anesthesia".into(), "objet attendu".into()));
        return None;
    };
    let model = match m.get("model") {
        Some(x) => AnesthesiaModel::from_json(&x.to_string(), "/anesthesia/model")
            .map_err(|e| errors.extend(super::load_issues("/anesthesia/model", e)))
            .ok(),
        None => {
            errors.push((
                "/anesthesia/model".into(),
                "module de constantes requis".into(),
            ));
            None
        }
    };
    let items = |key: &str| -> Vec<(String, String)> {
        m.get(key)
            .and_then(Json::as_array)
            .map(|a| {
                a.iter()
                    .enumerate()
                    .map(|(i, x)| (format!("/anesthesia/{key}/{i}"), x.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut agents = Vec::new();
    for (p, s) in items("agents") {
        match DrugModule::from_json(&s, &p) {
            Ok(d) => agents.push(d),
            Err(e) => errors.extend(super::load_issues(&p, e)),
        }
    }
    let mut inhalants = Vec::new();
    for (p, s) in items("inhalants") {
        match InhalantModule::from_json(&s, &p) {
            Ok(d) => inhalants.push(d),
            Err(e) => errors.extend(super::load_issues(&p, e)),
        }
    }
    let mut procedures = Vec::new();
    for (p, s) in items("procedures") {
        match ProcedureModule::from_json(&s, &p) {
            Ok(d) => procedures.push(d),
            Err(e) => errors.extend(super::load_issues(&p, e)),
        }
    }
    agents.sort_by(|a, b| a.id.cmp(&b.id));
    inhalants.sort_by(|a, b| a.id.cmp(&b.id));
    Some(AnesthesiaCatalog {
        model: model?,
        agents,
        inhalants,
        procedures,
    })
}

fn req_str<'a>(m: &'a Map<String, Json>, key: &str) -> Result<&'a str, ApiError> {
    m.get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| ApiError::one(format!("/{key}"), "chaîne requise"))
}

fn req_f64(m: &Map<String, Json>, key: &str) -> Result<f64, ApiError> {
    m.get(key)
        .and_then(Json::as_f64)
        .filter(|x| x.is_finite())
        .ok_or_else(|| ApiError::one(format!("/{key}"), "nombre requis"))
}

/// Événement JSON → [`Event`].
pub fn parse_event(v: &Json) -> Result<Event, ApiError> {
    let m = obj(v, "/event")?;
    let on = || opt_bool(m, "on", true);
    Ok(match req_str(m, "type")? {
        "bolus" => Event::Bolus {
            agent: req_str(m, "agent")?.into(),
            mg_per_kg: req_f64(m, "mg_per_kg")?,
            route: match m.get("route").and_then(Json::as_str).unwrap_or("iv") {
                "iv" => AdminRoute::Iv,
                "im" => AdminRoute::Im,
                "sc" => AdminRoute::Sc,
                r => {
                    return Err(ApiError::one(
                        "/route",
                        format!("voie « {r} » (iv, im, sc)"),
                    ))
                }
            },
            duration_s: m.get("duration_s").and_then(Json::as_f64),
        },
        "cri" => Event::Cri {
            agent: req_str(m, "agent")?.into(),
            mg_per_kg_h: req_f64(m, "mg_per_kg_h")?,
        },
        "vaporizer" => Event::Vaporizer {
            agent: req_str(m, "agent")?.into(),
            pct: req_f64(m, "pct")?,
        },
        "fresh_gas" => Event::FreshGas(req_f64(m, "l_min")?),
        "circuit" => Event::SetCircuit(match req_str(m, "kind")? {
            "circle" => Circuit::Circle,
            "non_rebreathing" => Circuit::NonRebreathing,
            k => return Err(ApiError::one("/kind", format!("circuit « {k} »"))),
        }),
        "intubate" => Event::Intubate,
        "extubate" => Event::Extubate,
        "ventilation" => Event::SetVentilation(match req_str(m, "mode")? {
            "spontaneous" => Ventilation::Spontaneous,
            "controlled" => Ventilation::Controlled {
                rr: req_f64(m, "rr")?,
                vt_ml_kg: req_f64(m, "vt_ml_kg")?,
            },
            k => return Err(ApiError::one("/mode", format!("mode « {k} »"))),
        }),
        "oxygen" => Event::Oxygen(on()?),
        "stimulus" => Event::Stimulus {
            intensity: req_f64(m, "intensity")?,
            duration_s: req_f64(m, "duration_s")?,
            territory: m
                .get("territory")
                .and_then(Json::as_str)
                .unwrap_or("skin")
                .into(),
        },
        "procedure_step" => Event::ProcedureStep(
            m.get("index")
                .and_then(Json::as_u64)
                .ok_or_else(|| ApiError::one("/index", "entier requis"))? as usize,
        ),
        "end_surgery" => Event::EndSurgery,
        "local_block" => Event::LocalBlock {
            agent: req_str(m, "agent")?.into(),
            territory: req_str(m, "territory")?.into(),
        },
        "fluids" => Event::Fluids(req_f64(m, "ml_kg_h")?),
        "fluid_bolus" => Event::FluidBolus(req_f64(m, "ml_kg")?),
        "hemorrhage" => Event::Hemorrhage(req_f64(m, "ml_kg")?),
        "warming" => Event::Warming(on()?),
        t => return Err(ApiError::one("/type", format!("événement inconnu « {t} »"))),
    })
}

/// Instantané → JSON (unités : mmHg, L/min, %, °C, mg/L).
pub fn snapshot_json(s: &Snapshot) -> Json {
    let vitals = s.eval.as_ref().map(|e| {
        let h = &e.hemo.summary;
        let m = &e.metabolic;
        json!({
            "hr": e.params.hr, "sap": h.sap, "map": h.map, "dap": h.dap, "cvp": h.cvp,
            "co": h.co, "sv": h.sv, "spo2": m.art.sao2, "pao2": m.art.pao2, "paco2": m.art.pco2,
            "ph": m.art.ph, "hco3": m.art.hco3, "lactate": m.lactate, "glucose": m.chem.glucose,
            "hct": m.chem.hct, "pvo2": m.ven.po2, "er": m.er,
            "sympathetic": e.autonomic.s, "baro_d": e.autonomic.d,
            "mechanisms": e.outcome.mechanisms.iter().map(|x| x.id).collect::<Vec<_>>(),
            "conduction_block": e.outcome.mechanisms.iter().any(|x| x.id == "conduction_block"),
        })
    });
    let d = &s.depth;
    let r = &s.reflexes;
    let n = &s.nociception;
    json!({
        "t_s": s.t_s,
        "vitals": vitals,
        "etco2": s.etco2, "paco2_dyn": s.paco2, "pao2_alv": s.pao2_alv, "va": s.va, "rr": s.rr,
        "fio2": s.fio2, "temp_c": s.temp_c,
        "agents": s.agents.iter().map(|a| json!({"id": a.id, "cp": a.cp, "ce": a.ce, "cri_mg_kg_h": a.cri_mg_kg_h})).collect::<Vec<_>>(),
        "inhalants": s.inhalants.iter().map(|i| json!({"id": i.id, "fi": i.fi, "fe": i.fe, "brain_mac": i.brain_mac, "vaporizer": i.vaporizer})).collect::<Vec<_>>(),
        "depth": {
            "hypnotic": d.hypnotic, "mac_reduction": d.mac_reduction, "delta": d.delta,
            "loc_index": d.loc_index, "sedation": d.sedation, "awareness": d.awareness,
            "plane": d.plane.as_str(), "dissociative": d.dissociative,
        },
        "reflexes": {
            "palpebral": r.palpebral, "eye_ventromedial": r.eye_ventromedial, "jaw_tone": r.jaw_tone,
            "swallowing": r.swallowing, "mydriasis": r.mydriasis, "movement": r.movement,
        },
        "nociception": {
            "stimulus": n.stimulus, "antinociception": n.antinociception, "nu": n.nu,
            "stress": n.stress, "sensitization": n.sensitization, "pain_score": n.pain_score,
        },
        "intubated": s.intubated, "controlled": s.controlled, "apnea": s.apnea,
        "alarms": s.alarms, "agitation": s.agitation, "dead": s.dead,
    })
}

impl Engine {
    fn anesthesia_catalog(&self) -> Result<&AnesthesiaCatalog, ApiError> {
        self.anesthesia
            .as_ref()
            .ok_or_else(|| ApiError::one("/anesthesia", "modules d'anesthésie non chargés"))
    }

    fn anesthesia_session(&mut self, m: &Map<String, Json>) -> Result<&mut Session, ApiError> {
        let id = req_str(m, "session")?.to_string();
        self.anesthesia_sessions
            .get_mut(&id)
            .ok_or_else(|| ApiError::one("/session", format!("session inconnue « {id} »")))
    }

    pub(crate) fn anesthesia_list(&self) -> ApiResult {
        let c = self.anesthesia_catalog()?;
        let name =
            |n: &vetsim_core::modules::Localized, id: &str| n.get("fr").unwrap_or(id).to_string();
        Ok(json!({
            "ok": true,
            "agents": c.agents.iter().map(|a| {
                let an = a.anesthesia.as_ref();
                json!({
                    "id": a.id, "name": name(&a.name, &a.id), "evidence": a.evidence.as_str(),
                    "classes": an.map(|x| x.classes.clone()).unwrap_or_default(),
                    "routes": {
                        "iv": true, "im": an.is_some_and(|x| x.im.is_some()),
                        "sc": an.is_some_and(|x| x.sc.is_some()),
                        "local": an.is_some_and(|x| x.local.is_some()),
                    },
                    "notes": a.notes,
                })
            }).collect::<Vec<_>>(),
            "inhalants": c.inhalants.iter().map(|i| json!({
                "id": i.id, "name": name(&i.name, &i.id), "mac_pct": i.mac_pct,
                "vaporizer_max_pct": i.vaporizer_max_pct, "evidence": i.evidence.as_str(),
            })).collect::<Vec<_>>(),
            "procedures": c.procedures.iter().map(|p| json!({
                "id": p.id, "name": name(&p.name, &p.id), "territories": p.territories,
                "steps": p.steps.iter().map(|s| json!({"id": s.id, "label": name(&s.label, &s.id),
                    "intensity": s.intensity, "duration_s": s.duration_s, "territory": s.territory})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }))
    }

    pub(crate) fn anesthesia_start(&mut self, pid: &str, opts: &Json) -> ApiResult {
        let m = obj(opts, "")?;
        let seed = opt_u64(m, "seed", 0)?;
        let base = self
            .patients
            .get(pid)
            .ok_or_else(|| ApiError::one("/patient", format!("patient inconnu « {pid} »")))?
            .patient
            .params
            .clone();
        let c = self.anesthesia_catalog()?;
        let procedure = match m.get("procedure").and_then(Json::as_str) {
            None => None,
            Some(id) => Some(
                c.procedures
                    .iter()
                    .find(|p| p.id == id)
                    .cloned()
                    .ok_or_else(|| {
                        ApiError::one("/procedure", format!("scénario inconnu « {id} »"))
                    })?,
            ),
        };
        let mut s = Session::new(
            base,
            c.agents.clone(),
            c.inhalants.clone(),
            c.model.clone(),
            procedure,
            seed,
        )?;
        let snap = s.step(1.0)?;
        self.next_id += 1;
        let id = format!("a{}", self.next_id);
        self.anesthesia_sessions.insert(id.clone(), s);
        Ok(json!({ "ok": true, "session": id, "snapshot": snapshot_json(&snap) }))
    }

    pub(crate) fn anesthesia_event(&mut self, opts: &Json) -> ApiResult {
        let m = obj(opts, "")?;
        let ev = parse_event(m.get("event").unwrap_or(&Json::Null))?;
        let s = self.anesthesia_session(m)?;
        s.apply(ev)?;
        Ok(json!({ "ok": true, "t_s": s.time_s() }))
    }

    pub(crate) fn anesthesia_step(&mut self, opts: &Json) -> ApiResult {
        let m = obj(opts, "")?;
        let dt = opt_f64(m, "dt_s", 1.0)?;
        let n = opt_u64(m, "n", 1)?.clamp(1, 3600) as usize;
        let all = opt_bool(m, "all", false)?;
        let waves = opt_f64(m, "waves_s", 0.0)?;
        let seed = opt_u64(m, "seed", 0)?;
        // auscultation demandée : configuration du module (clé « auscultation » de loadModules)
        let pcg_opts = match m.get("pcg") {
            None | Some(Json::Null) | Some(Json::Bool(false)) => None,
            Some(Json::Bool(true)) => Some(Map::new()),
            Some(Json::Object(o)) => Some(o.clone()),
            Some(_) => return Err(ApiError::one("/pcg", "booléen ou objet attendu")),
        };
        let pcg_cfg = match pcg_opts {
            Some(_) => Some(self.auscultation.clone().ok_or_else(|| {
                ApiError::one(
                    "/pcg",
                    "configuration auscultation non chargée (clé « auscultation » de loadModules)",
                )
            })?),
            None => None,
        };
        let s = self.anesthesia_session(m)?;
        let mut out = Vec::new();
        let mut last_eval = None;
        for i in 0..n {
            let snap = s.step(dt)?;
            if all || i + 1 == n {
                out.push(snapshot_json(&snap));
            }
            last_eval = snap.eval;
            if snap.dead.is_some() {
                break;
            }
        }
        let mut res = json!({ "ok": true, "snapshots": out });
        // Courbes du moniteur (ECG D II et pression artérielle) sur l'état final.
        if waves > 0.0 {
            if let Some(ev) = last_eval {
                use vetsim_core::signals::{patient_signals, SignalOptions};
                let dur = waves.min(10.0);
                let sg = patient_signals(
                    &ev,
                    &SignalOptions {
                        duration_s: dur,
                        ecg_fs: 250.0,
                        pcg_beats: 1,
                    },
                    seed,
                );
                res["ecg"] = json!(sg
                    .ecg
                    .v
                    .iter()
                    .map(|x| (*x as f32) as f64)
                    .collect::<Vec<_>>());
                res["ecg_fs"] = json!(250.0);
                res["pressures"] =
                    super::beat_pressures(&ev, &sg.autonomic.beats, seed, dur, 125.0)?;
                // Auscultation : même évaluation, même graine, même durée, donc mêmes battements
                // que l'ECG du moniteur (patient_pcg / patient_signals).
                if let (Some(o), Some(cfg)) = (&pcg_opts, &pcg_cfg) {
                    use vetsim_core::dsp::{base64, wav_pcm16};
                    use vetsim_core::pcg::{patient_pcg, PcgOptions};
                    let site = match o.get("site") {
                        None | Some(Json::Null) => None,
                        Some(v) => Some(
                            v.as_str()
                                .ok_or_else(|| ApiError::one("/pcg/site", "chaîne attendue"))?
                                .to_string(),
                        ),
                    };
                    let fs = opt_f64(o, "fs", cfg.fs)?;
                    if !(1000.0..=48000.0).contains(&fs) {
                        return Err(ApiError::one("/pcg/fs", "fréquence entre 1000 et 48000 Hz"));
                    }
                    let opts = PcgOptions {
                        fs: Some(fs),
                        duration_s: Some(dur),
                        resp_hz: None,
                        site,
                    };
                    let r = patient_pcg(&ev, cfg, &opts, seed)?;
                    let f = r.features;
                    res["pcg"] = json!({
                        "wav_base64": base64(&wav_pcm16(&r.samples, r.fs as u32)),
                        "fs": r.fs, "beats": r.beats, "grade": f.grade,
                        "murmur": f.murmur.label_fr(), "s3": f.s3,
                        "site": opts.site.clone().unwrap_or_else(|| "base".into()),
                    });
                }
            }
        }
        Ok(res)
    }

    pub(crate) fn anesthesia_record(&mut self, opts: &Json) -> ApiResult {
        let m = obj(opts, "")?;
        let s = self.anesthesia_session(m)?;
        let ms = s.milestones();
        Ok(json!({
            "ok": true,
            "record": s.record().iter().map(|r| json!({
                "t_s": r.t_s, "hr": r.hr, "sap": r.sap, "map": r.map, "dap": r.dap, "spo2": r.spo2,
                "etco2": r.etco2, "rr": r.rr, "temp_c": r.temp_c, "fe_agent": r.fe_agent,
                "delta": r.delta, "plane": r.plane.as_str(),
            })).collect::<Vec<_>>(),
            "log": s.log().iter().map(|(t, l)| json!({"t_s": t, "event": l})).collect::<Vec<_>>(),
            "milestones": {
                "loc": ms.loc, "intubation": ms.intubation, "vaporizer_off": ms.vaporizer_off,
                "swallowing": ms.swallowing, "extubation": ms.extubation,
                "head_lift": ms.head_lift, "sternal": ms.sternal,
            },
            "dead": s.dead(),
        }))
    }
}
