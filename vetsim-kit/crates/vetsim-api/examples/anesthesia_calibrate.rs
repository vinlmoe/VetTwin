//! Banc de calibration du module d'anesthésie (docs/ANESTHESIE.tex, § Calibration).
//!
//! `cargo run --release -p vetsim-api --example anesthesia_calibrate [-- --fit [--inhalants | --agents] [--dex]]`
//!
//! Sans `--fit` : affiche, pour les modules actuels, chaque cible publiée et la valeur du modèle.
//! Avec `--fit` : cherche les coefficients (balayage déterministe) et les écrit en JSON sur la
//! sortie standard, à reporter dans `tests/anesthesia_modules/coefficients.json` (générateurs du même dossier). Aucune valeur n'est
//! modifiée automatiquement dans `modules/`.

use std::fs;
use std::path::PathBuf;
use vetsim_core::anesthesia::{AdminRoute, AnesthesiaModel, Event, InhalantModule, Session};
use vetsim_core::dsl::Env;
use vetsim_core::{
    apply_modifiers, evaluate, evaluate_effects, DrugModule, Params, Patient, PatientModule,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

struct World {
    base: Params,
    model: AnesthesiaModel,
    agents: Vec<DrugModule>,
    inhalants: Vec<InhalantModule>,
}

fn world() -> World {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let base = Patient::healthy(&pm, 20.0).unwrap().params;
    let model =
        AnesthesiaModel::from_json(&read("modules/anesthesia/anesthesia_model.json"), "m").unwrap();
    let mut agents = Vec::new();
    let mut inhalants = Vec::new();
    for (dir, inh) in [
        ("modules/anesthesia/agents", false),
        ("modules/anesthesia/inhalants", true),
    ] {
        let mut paths: Vec<_> = fs::read_dir(root().join(dir))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        for p in paths {
            let s = fs::read_to_string(&p).unwrap();
            let o = p.display().to_string();
            if inh {
                inhalants.push(InhalantModule::from_json(&s, &o).unwrap());
            } else {
                agents.push(DrugModule::from_json(&s, &o).unwrap());
            }
        }
    }
    World {
        base,
        model,
        agents,
        inhalants,
    }
}

impl World {
    fn session(&self) -> Session {
        Session::new(
            self.base.clone(),
            self.agents.clone(),
            self.inhalants.clone(),
            self.model.clone(),
            None,
            1,
        )
        .unwrap()
    }
    fn agent_mut(&mut self, id: &str) -> &mut DrugModule {
        self.agents.iter_mut().find(|a| a.id == id).unwrap()
    }
    fn inhalant_mut(&mut self, id: &str) -> &mut InhalantModule {
        self.inhalants.iter_mut().find(|a| a.id == id).unwrap()
    }
}

/// Régime permanent sous `M` MAC d'un halogéné (effets du module appliqués directement).
fn at_mac(w: &World, id: &str, m: f64) -> (f64, f64, f64, f64) {
    let g = w.inhalants.iter().find(|g| g.id == id).unwrap();
    let mut q = w.base.clone();
    let mut env = Env::new();
    env.set_num("M", m);
    env.set_num("MB", m);
    let mods = evaluate_effects(&g.effects, &env, id).unwrap();
    apply_modifiers(&mut q, &mods).unwrap();
    match evaluate(&q) {
        Ok(e) => (
            e.hemo.summary.map,
            e.params.hr,
            e.hemo.summary.co,
            e.metabolic.art.pco2,
        ),
        Err(_) => (f64::NAN, f64::NAN, f64::NAN, f64::NAN),
    }
}

fn set_pow(e: &mut vetsim_core::modules::Effect, c: f64, p: f64, lo: f64) {
    let j = serde_json::json!({"max": [lo, {"sub": [1, {"mul": [c, {"pow": ["M", p]}]}]}]});
    let mut issues = Vec::new();
    e.expr = vetsim_core::dsl::Expr::parse(&j, "", &["M", "MB"], &mut issues).unwrap();
}

fn set_vent(e: &mut vetsim_core::modules::Effect, a: f64, n: f64) {
    let j = serde_json::json!({"max": [0, {"sub": [1, {"mul": [a, {"pow": ["M", n]}]}]}]});
    let mut issues = Vec::new();
    e.expr = vetsim_core::dsl::Expr::parse(&j, "", &["M", "MB"], &mut issues).unwrap();
}

fn set_lin(e: &mut vetsim_core::modules::Effect, c: f64, var: &str, add: bool) {
    let j = if add {
        serde_json::json!({"mul": [c, var]})
    } else {
        serde_json::json!({"add": [1, {"mul": [c, var]}]})
    };
    let mut issues = Vec::new();
    e.expr = vetsim_core::dsl::Expr::parse(&j, "", &["E", "EP"], &mut issues).unwrap();
}

/// Bissection monotone : `f(x) − target` change de signe sur [lo, hi].
fn bisect(mut lo: f64, mut hi: f64, target: f64, f: impl Fn(f64) -> f64) -> f64 {
    let flo = f(lo) - target;
    for _ in 0..22 {
        let mid = 0.5 * (lo + hi);
        let fm = f(mid) - target;
        if (fm > 0.0) == (flo > 0.0) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

struct Run {
    hr: Vec<f64>,
    map: Vec<f64>,
    co: Vec<f64>,
    loc: Vec<f64>,
    ce: Vec<(String, Vec<f64>)>,
    delta: Vec<f64>,
    r: Vec<f64>,
}

fn run(w: &World, events: &[(f64, Event)], t_end: f64, dt: f64) -> Run {
    let mut s = w.session();
    let mut out = Run {
        hr: vec![],
        map: vec![],
        co: vec![],
        loc: vec![],
        ce: vec![],
        delta: vec![],
        r: vec![],
    };
    let mut evs: Vec<_> = events.to_vec();
    evs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut i = 0;
    let mut t = 0.0;
    while t <= t_end + 1e-9 {
        while i < evs.len() && evs[i].0 <= t + 1e-9 {
            s.apply(evs[i].1.clone()).unwrap();
            i += 1;
        }
        let sn = s.step(dt).unwrap();
        let (hr, map, co) = sn
            .eval
            .as_ref()
            .map(|e| (e.params.hr, e.hemo.summary.map, e.hemo.summary.co))
            .unwrap_or((f64::NAN, f64::NAN, f64::NAN));
        out.hr.push(hr);
        out.map.push(map);
        out.co.push(co);
        out.loc.push(sn.depth.loc_index);
        out.delta.push(sn.depth.delta);
        out.r.push(sn.depth.mac_reduction);
        if out.ce.is_empty() {
            out.ce = sn.agents.iter().map(|a| (a.id.clone(), vec![])).collect();
        }
        for (k, a) in sn.agents.iter().enumerate() {
            out.ce[k].1.push(a.ce);
        }
        t += dt;
    }
    out
}

fn bolus(agent: &str, mg: f64, route: AdminRoute) -> Event {
    Event::Bolus {
        agent: agent.into(),
        mg_per_kg: mg,
        route,
        duration_s: None,
    }
}

fn max(v: &[f64]) -> f64 {
    v.iter()
        .copied()
        .filter(|x| x.is_finite())
        .fold(f64::MIN, f64::max)
}
fn min(v: &[f64]) -> f64 {
    v.iter()
        .copied()
        .filter(|x| x.is_finite())
        .fold(f64::MAX, f64::min)
}

fn main() {
    let fit = std::env::args().any(|a| a == "--fit");
    let only_inhalants = std::env::args().any(|a| a == "--inhalants");
    let mut w = world();
    let e0 = evaluate(&w.base).unwrap();
    let (map0, hr0, co0) = (e0.hemo.summary.map, e0.params.hr, e0.hemo.summary.co);
    println!(
        "# chien sain 20 kg : PAM {map0:.1}, FC {hr0:.1}, DC {co0:.2}, PaCO2 {:.1}",
        e0.metabolic.art.pco2
    );
    let mut k = serde_json::Map::new();

    // ---------------- halogénés : PAM −22 % à 1,2 MAC ; PaCO2 46,2 à 1 MAC ; apnée à l'indice
    let skip_inh = std::env::args().any(|a| a == "--agents");
    for (id, ai) in [("isoflurane", 2.61_f64), ("sevoflurane", 3.45)] {
        if fit && !skip_inh {
            // effets [ees_lv, ees_rv, r_sys, vent_drive, baro_gain] : r_sys = 1 − c·M^p,
            // baro_gain = 1 − b·M ; cibles PAM −22 % (1,2 MAC), −38 % (2 MAC), DC −17 % (2 MAC)
            let mk = |c: f64, p: f64, b: f64| {
                let mut w2 = World {
                    base: w.base.clone(),
                    model: w.model.clone(),
                    agents: vec![],
                    inhalants: w.inhalants.clone(),
                };
                set_pow(&mut w2.inhalant_mut(id).effects[2], c, p, 0.1);
                set_pow(&mut w2.inhalant_mut(id).effects[4], b, 1.0, 0.0);
                w2
            };
            let mut best = (f64::MAX, 0.0, 0.0, 0.0);
            for bi in 0..=25 {
                let b = 0.02 * bi as f64;
                for pi in 0..=20 {
                    let p = 0.5 + 0.1 * pi as f64;
                    let c = bisect(0.0, 0.8, 0.78, |c| at_mac(&mk(c, p, b), id, 1.2).0 / map0);
                    let (m2, _, co2, _) = at_mac(&mk(c, p, b), id, 2.0);
                    let err = (m2 / map0 - 0.62) * (m2 / map0 - 0.62)
                        + (co2 / co0 - 0.83) * (co2 / co0 - 0.83);
                    if err < best.0 {
                        best = (err, c, p, b);
                    }
                }
            }
            let (_, c, p, b) = best;
            set_pow(&mut w.inhalant_mut(id).effects[2], c, p, 0.1);
            set_pow(&mut w.inhalant_mut(id).effects[4], b, 1.0, 0.0);
            let pre = if id == "isoflurane" { "iso" } else { "sevo" };
            k.insert(format!("{pre}_rsys"), c.into());
            k.insert(format!("{pre}_rsys_p"), p.into());
            k.insert(format!("{pre}_baro"), b.into());
            let n = bisect(0.3, 6.0, 46.2, |n| {
                let mut w2 = World {
                    base: w.base.clone(),
                    model: w.model.clone(),
                    agents: vec![],
                    inhalants: w.inhalants.clone(),
                };
                set_vent(
                    &mut w2.inhalant_mut(id).effects[3],
                    vetsim_core::math::pow(ai, -n),
                    n,
                );
                at_mac(&w2, id, 1.0).3
            });
            set_vent(
                &mut w.inhalant_mut(id).effects[3],
                vetsim_core::math::pow(ai, -n),
                n,
            );
            let p = if id == "isoflurane" { "iso" } else { "sevo" };
            k.insert(format!("{p}_va"), vetsim_core::math::pow(ai, -n).into());
            k.insert(format!("{p}_vn"), n.into());
        }
        for m in [1.0, 1.2, 1.5, 2.0] {
            let (map, hr, co, pco2) = at_mac(&w, id, m);
            println!(
                "{id} {m} MAC : PAM {:+.0} %, FC {:+.0} %, DC {:+.0} %, PaCO2 {pco2:.1}",
                100.0 * (map / map0 - 1.0),
                100.0 * (hr / hr0 - 1.0),
                100.0 * (co / co0 - 1.0)
            );
        }
    }
    println!("  cibles sévoflurane (Bernard 1990) : 1,2 MAC PAM −22 %, FC +60 %, DC 0 ; 2 MAC PAM −38 %, FC +54 %, DC −17 % ; PaCO2 iso 1 MAC 46,2 (Steffey 1993)");
    if only_inhalants {
        if fit {
            println!("{}", serde_json::Value::Object(k));
        }
        return;
    }

    // ---------------- dexmédétomidine 10 µg/kg IM : FC −55 %, DC −59 %, PAM en hausse
    let dex = |w: &World| {
        let r = run(
            w,
            &[(0.0, bolus("dexmedetomidine", 0.010, AdminRoute::Im))],
            40.0 * 60.0,
            10.0,
        );
        let i15 = (15.0 * 60.0 / 10.0) as usize;
        (
            r.hr[i15] / hr0 - 1.0,
            r.co[i15] / co0 - 1.0,
            r.map[i15] / map0 - 1.0,
            max(&r.map) / map0 - 1.0,
        )
    };
    // Dexmédétomidine : la bissection explore des résistances extrêmes très coûteuses ;
    // calage par petite grille bornée (examples/anesthesia_timing.rs), `--dex` pour la relancer.
    if fit && std::env::args().any(|a| a == "--dex") {
        // vagal → FC ; s0 → DC ; r_sys (EP) → PAM : balayage coordonné
        let (mut v, mut s0, mut rs) = (0.3, 0.15, 1.0);
        for _ in 0..2 {
            v = bisect(0.0, 0.6, -0.55, |x| {
                let mut w2 = World {
                    base: w.base.clone(),
                    model: w.model.clone(),
                    agents: w.agents.clone(),
                    inhalants: vec![],
                };
                let a = w2.agent_mut("dexmedetomidine");
                set_lin(&mut a.pd.effects[0], rs, "EP", false);
                set_lin(&mut a.pd.effects[1], -s0, "E", true);
                set_lin(&mut a.pd.effects[2], x, "E", true);
                dex(&w2).0
            });
            rs = bisect(0.0, 3.0, 0.20, |x| {
                let mut w2 = World {
                    base: w.base.clone(),
                    model: w.model.clone(),
                    agents: w.agents.clone(),
                    inhalants: vec![],
                };
                let a = w2.agent_mut("dexmedetomidine");
                set_lin(&mut a.pd.effects[0], x, "EP", false);
                set_lin(&mut a.pd.effects[1], -s0, "E", true);
                set_lin(&mut a.pd.effects[2], v, "E", true);
                dex(&w2).2
            });
            s0 = bisect(0.0, 0.2, -0.59, |x| {
                let mut w2 = World {
                    base: w.base.clone(),
                    model: w.model.clone(),
                    agents: w.agents.clone(),
                    inhalants: vec![],
                };
                let a = w2.agent_mut("dexmedetomidine");
                set_lin(&mut a.pd.effects[0], rs, "EP", false);
                set_lin(&mut a.pd.effects[1], -x, "E", true);
                set_lin(&mut a.pd.effects[2], v, "E", true);
                dex(&w2).1
            });
        }
        let a = w.agent_mut("dexmedetomidine");
        set_lin(&mut a.pd.effects[0], rs, "EP", false);
        set_lin(&mut a.pd.effects[1], -s0, "E", true);
        set_lin(&mut a.pd.effects[2], v, "E", true);
        k.insert("dex_vagal".into(), v.into());
        k.insert("dex_s0".into(), s0.into());
        k.insert("dex_rsys".into(), rs.into());
    }
    let (hr, co, map, mapmax) = dex(&w);
    println!("dexmédétomidine 10 µg/kg IM à 15 min : FC {:+.0} % (cible −55), DC {:+.0} % (−59), PAM {:+.0} % (hausse ; +36 % à 18 min après 2 µg/kg IV), PAM max {:+.0} %", 100.0 * hr, 100.0 * co, 100.0 * map, 100.0 * mapmax);

    // ---------------- acépromazine à effet maximal : PAM −17 %
    let ace = |w: &World| {
        let r = run(
            w,
            &[(0.0, bolus("acepromazine", 0.1, AdminRoute::Iv))],
            60.0 * 60.0,
            60.0,
        );
        min(&r.map) / map0 - 1.0
    };
    if fit {
        let c = bisect(0.0, 2.0, -0.17, |x| {
            let mut w2 = World {
                base: w.base.clone(),
                model: w.model.clone(),
                agents: w.agents.clone(),
                inhalants: vec![],
            };
            set_lin(
                &mut w2.agent_mut("acepromazine").pd.effects[0],
                x,
                "E",
                false,
            );
            ace(&w2)
        });
        set_lin(
            &mut w.agent_mut("acepromazine").pd.effects[0],
            c,
            "E",
            false,
        );
        k.insert("ace_vencap".into(), c.into());
    }
    println!(
        "acépromazine 0,1 mg/kg IV, nadir : PAM {:+.0} % (cible −17 %, Rangel 2020)",
        100.0 * ace(&w)
    );

    // ---------------- propofol 5 mg/kg après acépromazine + méthadone : PAM −31 % au nadir
    let prop = |w: &World| {
        let ev = [
            (0.0, bolus("acepromazine", 0.03, AdminRoute::Im)),
            (0.0, bolus("methadone", 0.3, AdminRoute::Im)),
            (1800.0, bolus("propofol", 5.0, AdminRoute::Iv)),
        ];
        let r = run(w, &ev, 1800.0 + 300.0, 5.0);
        let i0 = (1795.0 / 5.0) as usize;
        // (pas de 5 s : nadir de PAM à 55 s chez Cattai 2018)
        (
            min(&r.map[i0 + 1..]) / r.map[i0] - 1.0,
            max(&r.hr[i0 + 1..]) / r.hr[i0] - 1.0,
        )
    };
    if fit {
        let c = bisect(0.0, 0.95, -0.31, |x| {
            let mut w2 = World {
                base: w.base.clone(),
                model: w.model.clone(),
                agents: w.agents.clone(),
                inhalants: vec![],
            };
            set_lin(&mut w2.agent_mut("propofol").pd.effects[0], -x, "E", false);
            prop(&w2).0
        });
        set_lin(&mut w.agent_mut("propofol").pd.effects[0], -c, "E", false);
        k.insert("prop_rsys".into(), c.into());
    }
    let (m, h) = prop(&w);
    println!("propofol 5 mg/kg (acépromazine + méthadone) : PAM {:+.0} % (cible −31 %), FC {:+.0} % (+62 % chez Cattai 2018)", 100.0 * m, 100.0 * h);

    // ---------------- doses d'induction : même pic d'indice de perte de conscience
    let peak_loc = |w: &World, evs: &[(f64, Event)]| max(&run(w, evs, 300.0, 2.0).loc);
    let p_ref = peak_loc(&w, &[(0.0, bolus("propofol", 5.2, AdminRoute::Iv))]);
    println!("propofol 5,2 mg/kg IV seul : pic d'indice de conscience {p_ref:.2} (perte de conscience à {:.2})", w.model.loc_mac);
    let cimm = |w: &World, id: &str| {
        w.agents
            .iter()
            .find(|a| a.id == id)
            .unwrap()
            .anesthesia
            .as_ref()
            .unwrap()
            .c_imm_mg_l
            .unwrap()
    };
    for (id, dose, extra) in [
        ("alfaxalone", 2.7, None),
        ("ketamine", 6.1, Some(("midazolam", 0.26))),
    ] {
        let mut evs = vec![(0.0, bolus(id, dose, AdminRoute::Iv))];
        if let Some((b, d)) = extra {
            evs.push((0.0, bolus(b, d, AdminRoute::Iv)));
        }
        let pk = peak_loc(&w, &evs);
        if fit {
            let c = cimm(&w, id) * pk / p_ref;
            k.insert(format!("{}_cimm", &id[..3]), c.into());
            w.agent_mut(id).anesthesia.as_mut().unwrap().c_imm_mg_l = Some(c);
        }
        println!(
            "{id} {dose} mg/kg IV : pic {:.2} (même marge que le propofol visée)",
            peak_loc(&w, &evs)
        );
    }
    // ---------------- kétamine-midazolam : FC +75 %
    let ket = |w: &World| {
        let r = run(
            w,
            &[
                (0.0, bolus("ketamine", 6.1, AdminRoute::Iv)),
                (0.0, bolus("midazolam", 0.26, AdminRoute::Iv)),
            ],
            300.0,
            5.0,
        );
        max(&r.hr) / hr0 - 1.0
    };
    if fit {
        let c = bisect(0.0, 0.8, 0.75, |x| {
            let mut w2 = World {
                base: w.base.clone(),
                model: w.model.clone(),
                agents: w.agents.clone(),
                inhalants: vec![],
            };
            set_lin(&mut w2.agent_mut("ketamine").pd.effects[0], x, "E", true);
            ket(&w2)
        });
        set_lin(&mut w.agent_mut("ketamine").pd.effects[0], c, "E", true);
        k.insert("ket_s0".into(), c.into());
    }
    println!(
        "kétamine 6,1 + midazolam 0,26 mg/kg IV : FC {:+.0} % (cible +75 %, Hampton 2019)",
        100.0 * ket(&w)
    );

    // ---------------- méthadone : épargne de MAC (Credie 2010)
    let meth = |w: &World, dose: f64| {
        let r = run(
            w,
            &[(0.0, bolus("methadone", dose, AdminRoute::Iv))],
            5.0 * 3600.0,
            60.0,
        );
        (r.r[150], r.r[300])
    };
    let target = [(1.0, 0.48, 0.30), (0.5, 0.35, 0.15)];
    if fit {
        // l'épargne ne dépend que de Ce : on relève Ce une fois par dose, puis ajustement direct
        let ce_at = |dose: f64| {
            let r = run(
                &w,
                &[(0.0, bolus("methadone", dose, AdminRoute::Iv))],
                5.0 * 3600.0,
                60.0,
            );
            let c = &r.ce.iter().find(|(id, _)| id == "methadone").unwrap().1;
            (c[150], c[300])
        };
        let ce: Vec<(f64, f64)> = target.iter().map(|(d, _, _)| ce_at(*d)).collect();
        let h = w
            .agents
            .iter()
            .find(|a| a.id == "methadone")
            .unwrap()
            .anesthesia
            .as_ref()
            .unwrap()
            .mac_reduction
            .unwrap()
            .hill;
        let sat = |mx: f64, ec: f64, c: f64| {
            mx * vetsim_core::math::pow(c, h)
                / (vetsim_core::math::pow(ec, h) + vetsim_core::math::pow(c, h))
        };
        let mut best = (f64::MAX, 0.0, 0.0);
        for mi in 0..=50 {
            let mx = 0.4 + 0.01 * mi as f64;
            for ei in 0..=200 {
                let ec = 0.002 * vetsim_core::math::pow(1.03, ei as f64);
                let err: f64 = target
                    .iter()
                    .zip(&ce)
                    .map(|((_, a, b), (c1, c2))| {
                        let (x, y) = (sat(mx, ec, *c1) - a, sat(mx, ec, *c2) - b);
                        x * x + y * y
                    })
                    .sum();
                if err < best.0 {
                    best = (err, mx, ec);
                }
            }
        }
        let s = w
            .agent_mut("methadone")
            .anesthesia
            .as_mut()
            .unwrap()
            .mac_reduction
            .as_mut()
            .unwrap();
        s.max = best.1;
        s.ec50_mg_l = best.2;
        k.insert("meth_macmax".into(), best.1.into());
        k.insert("meth_macec50".into(), best.2.into());
    }
    for (d, a, b) in target {
        let (x, y) = meth(&w, d);
        println!("méthadone {d} mg/kg IV : épargne {:.0} % à 2,5 h (cible {:.0}), {:.0} % à 5 h (cible {:.0})", 100.0 * x, 100.0 * a, 100.0 * y, 100.0 * b);
    }
    if fit {
        println!("{}", serde_json::Value::Object(k));
    }
}
