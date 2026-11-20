//! Banc de validation hors calibrage (docs/VALIDATION_HORS_CALIBRAGE.tex) : rejoue dans une session
//! d'anesthésie des scénarios publiés qui n'ont servi à caler aucun coefficient, et imprime les
//! grandeurs comparées à la littérature. Déterministe (graine 1). Lancer depuis `vetsim-kit/` :
//! `cargo run --release -p vetsim-api --example validation_hors_calibrage`.
use std::path::PathBuf;
use vetsim_core::anesthesia::{
    AdminRoute, AnesthesiaModel, Circuit, Event, InhalantModule, Session, Snapshot, Ventilation,
};
use vetsim_core::{DrugModule, Params, Patient, PatientModule};

/// Racine du kit ; `VETSIM_ROOT` permet de rejouer avec une copie modifiée des modules.
fn root() -> PathBuf {
    std::env::var("VETSIM_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}
fn read(rel: &str) -> String {
    let p = root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}
fn dir(rel: &str) -> Vec<String> {
    let mut v: Vec<_> = std::fs::read_dir(root().join(rel))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    v.into_iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}
fn base() -> Params {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    Patient::healthy(&pm, 20.0).unwrap().params
}
fn session() -> Session {
    let model =
        AnesthesiaModel::from_json(&read("modules/anesthesia/anesthesia_model.json"), "m").unwrap();
    let agents = dir("modules/anesthesia/agents")
        .iter()
        .map(|s| DrugModule::from_json(s, "a").unwrap())
        .collect();
    let inh = dir("modules/anesthesia/inhalants")
        .iter()
        .map(|s| InhalantModule::from_json(s, "i").unwrap())
        .collect();
    Session::new(base(), agents, inh, model, None, 1).unwrap()
}
fn bolus(a: &str, mg: f64, route: AdminRoute) -> Event {
    Event::Bolus {
        agent: a.into(),
        mg_per_kg: mg,
        route,
        duration_s: None,
    }
}
fn run(s: &mut Session, secs: f64, dt: f64) -> Snapshot {
    let n = (secs / dt).round().max(1.0) as usize;
    let mut last = None;
    for _ in 0..n {
        last = Some(s.step(dt).unwrap());
    }
    last.unwrap()
}
/// FC, PAM, DC, VS, Ht, PaCO2, PaO2, SaO2 (ou `décès`).
fn vit(sn: &Snapshot) -> String {
    match sn.eval.as_ref() {
        None => format!("décès {:?}", sn.dead),
        Some(e) => format!(
            "fc {:.1} pam {:.1} dc {:.3} vs {:.1} ht {:.1} paco2 {:.1} pao2 {:.1} sao2 {:.1}",
            e.params.hr,
            e.hemo.summary.map,
            e.hemo.summary.co,
            e.hemo.summary.sv,
            e.metabolic.chem.hct,
            sn.paco2,
            e.metabolic.art.pao2,
            e.metabolic.art.sao2
        ),
    }
}
/// Induction propofol 5,2 mg/kg sous O2, intubation, halogéné à `mac_mult` MAC (circuit filtre 2 L/min).
fn anesthetized(s: &mut Session, agent: &str, mac_mult: f64) {
    let mac = if agent == "isoflurane" { 1.3 } else { 2.2 };
    s.apply(Event::Oxygen(true)).unwrap();
    run(s, 120.0, 10.0);
    s.apply(bolus("propofol", 5.2, AdminRoute::Iv)).unwrap();
    run(s, 90.0, 5.0);
    s.apply(Event::Intubate).unwrap();
    s.apply(Event::FreshGas(2.0)).unwrap();
    s.apply(Event::Vaporizer {
        agent: agent.into(),
        pct: mac_mult * mac * 1.15,
    })
    .unwrap();
}

/// Maintient la fraction télé-expiratoire à `target` MAC pendant `secs` (définition des études :
/// MAC télé-expiratoire) ; réglage corrigé toutes les 15 s, gain modéré. Renvoie aussi le maximum
/// de MAC cérébrale atteint (contrôle de dépassement).
fn hold_mac(s: &mut Session, agent: &str, target: f64, secs: f64) -> Snapshot {
    hold_mac_trace(s, agent, target, secs, false).0
}
fn hold_mac_trace(
    s: &mut Session,
    agent: &str,
    target: f64,
    secs: f64,
    trace: bool,
) -> (Snapshot, f64) {
    let mac = if agent == "isoflurane" { 1.3 } else { 2.2 };
    let vmax = if agent == "isoflurane" { 5.0 } else { 8.0 };
    let mut pct = s
        .step(0.0001)
        .unwrap()
        .inhalants
        .iter()
        .find(|i| i.id == agent)
        .map_or(0.0, |i| i.vaporizer);
    let mut last = None;
    let mut bmax: f64 = 0.0;
    let n = ((secs / 15.0).round() as usize).max(1);
    for k in 0..n {
        let sn = s.step(15.0).unwrap();
        if sn.eval.is_none() {
            return (sn, bmax);
        }
        let i = sn.inhalants.iter().find(|i| i.id == agent).unwrap();
        bmax = bmax.max(i.brain_mac);
        if trace && k % 8 == 0 {
            println!(
                "  t {:.0} s vap {:.2} fi {:.2} fe/MAC {:.2} cerveau {:.2} pam {:.0}",
                sn.t_s,
                pct,
                i.fi,
                i.fe / mac,
                i.brain_mac,
                sn.eval.as_ref().unwrap().hemo.summary.map
            );
        }
        pct = (pct + 0.25 * (target - i.fe / mac) * mac).clamp(0.05, vmax);
        s.apply(Event::Vaporizer {
            agent: agent.into(),
            pct,
        })
        .unwrap();
        last = Some(sn);
    }
    (last.unwrap(), bmax)
}
fn bmac(sn: &Snapshot, agent: &str) -> f64 {
    sn.inhalants
        .iter()
        .find(|i| i.id == agent)
        .map_or(0.0, |i| i.brain_mac)
}

fn hemorrhage(anesth: bool) {
    // retraits successifs de 5 % du volume sanguin (85 mL/kg) toutes les 5 min
    let mut s = session();
    if anesth {
        anesthetized(&mut s, "isoflurane", 1.3);
        let sn = hold_mac(&mut s, "isoflurane", 1.3, 1800.0);
        println!(
            "hemorragie iso MAC cérébrale {:.2}",
            bmac(&sn, "isoflurane")
        );
    } else {
        run(&mut s, 60.0, 10.0);
    }
    println!(
        "hemorragie {} 0 % : {}",
        if anesth { "iso1.3" } else { "vigile" },
        vit(&run(&mut s, 10.0, 10.0))
    );
    for k in 1..=10 {
        s.apply(Event::Hemorrhage(0.05 * 85.0)).unwrap();
        let sn = run(&mut s, 300.0, 10.0);
        println!(
            "hemorragie {} {} % : {}",
            if anesth { "iso1.3" } else { "vigile" },
            5 * k,
            vit(&sn)
        );
        if sn.eval.is_none() {
            break;
        }
    }
}

fn fluid_bolus() {
    // 20 mL/kg en 15 min (4 bolus de 5 mL/kg) chez le chien vigile, puis suivi
    let mut s = session();
    println!("bolus20 t=0 : {}", vit(&run(&mut s, 30.0, 10.0)));
    for _ in 0..4 {
        s.apply(Event::FluidBolus(5.0)).unwrap();
        run(&mut s, 225.0, 15.0);
    }
    println!("bolus20 fin (15 min) : {}", vit(&run(&mut s, 1.0, 1.0)));
    for m in [30.0, 60.0] {
        let sn = run(
            &mut s,
            if m == 30.0 { 15.0 * 60.0 } else { 30.0 * 60.0 },
            30.0,
        );
        println!("bolus20 +{m} min après la fin : {}", vit(&sn));
    }
}

fn apnea() {
    // (a) induction rapide au propofol (6 mg/kg en 5 s) chez le chien non intubé, à l'air ou après
    //     3 min d'O2 au masque (masque maintenu) : délai injection → SaO2 < 90 %
    for preox in [false, true] {
        let mut s = session();
        if preox {
            s.apply(Event::Oxygen(true)).unwrap();
            run(&mut s, 180.0, 10.0);
        }
        s.apply(Event::Bolus {
            agent: "propofol".into(),
            mg_per_kg: 6.0,
            route: AdminRoute::Iv,
            duration_s: Some(5.0),
        })
        .unwrap();
        let (mut t, mut t_ap, mut t_des, mut min_sat) = (0.0_f64, None, None, 100.0_f64);
        while t < 900.0 {
            let sn = s.step(2.0).unwrap();
            t += 2.0;
            if t_ap.is_none() && sn.apnea {
                t_ap = Some(t);
            }
            let sat = sn.eval.as_ref().map_or(0.0, |e| e.metabolic.art.sao2);
            min_sat = min_sat.min(sat);
            if t_des.is_none() && sat < 90.0 {
                t_des = Some(t);
            }
        }
        println!(
            "apnee propofol 6 mg/kg en 5 s preox={preox} : apnée à {t_ap:?} s, SaO2<90 % à {t_des:?} s, SaO2 min {min_sat:.1}"
        );
    }
    // (b) chien intubé sous isoflurane 1,3 MAC, ventilation contrôlée en O2 pur, puis ventilateur arrêté
    let mut s = session();
    anesthetized(&mut s, "isoflurane", 1.3);
    s.apply(Event::SetVentilation(Ventilation::Controlled {
        rr: 12.0,
        vt_ml_kg: 12.0,
    }))
    .unwrap();
    hold_mac(&mut s, "isoflurane", 1.3, 600.0);
    let sn = s.step(1.0).unwrap();
    let p0 = sn.paco2;
    s.apply(Event::SetVentilation(Ventilation::Controlled {
        rr: 0.0,
        vt_ml_kg: 0.0,
    }))
    .unwrap();
    let (mut t, mut p1, mut p5, mut t_des) = (0.0_f64, f64::NAN, f64::NAN, None);
    while t < 1800.0 {
        let sn = s.step(5.0).unwrap();
        t += 5.0;
        if (t - 60.0).abs() < 1e-6 {
            p1 = sn.paco2;
        }
        if (t - 300.0).abs() < 1e-6 {
            p5 = sn.paco2;
        }
        let sat = sn.eval.as_ref().map_or(0.0, |e| e.metabolic.art.sao2);
        if t_des.is_none() && sat < 90.0 {
            t_des = Some(t);
        }
        if sn.dead.is_some() {
            println!("apnee intubé O2 : décès {:?} à {t} s", sn.dead);
            break;
        }
    }
    println!(
        "apnee intubé O2 iso 1,3 MAC : PaCO2 {p0:.1} → {p1:.1} (1 min) → {p5:.1} (5 min) ; SaO2<90 % à {t_des:?} s"
    );
}

fn oxygen_and_ventilation() {
    for (agent, m) in [
        ("isoflurane", 1.0),
        ("isoflurane", 1.5),
        ("isoflurane", 2.0),
    ] {
        let mut s = session();
        anesthetized(&mut s, agent, m);
        let sn = hold_mac(&mut s, agent, m, 3600.0);
        println!(
            "spontane {agent} {m} MAC (60 min, FiO2 {:.2}) : brain_mac {:.2} fr {:.1} {}",
            sn.fio2,
            bmac(&sn, agent),
            sn.rr,
            vit(&sn)
        );
        s.apply(Event::SetVentilation(Ventilation::Controlled {
            rr: 12.0,
            vt_ml_kg: 12.0,
        }))
        .unwrap();
        let sn = hold_mac(&mut s, agent, m, 900.0);
        println!("controle 12x12 mL/kg {agent} {m} MAC : {}", vit(&sn));
    }
}

fn uptake() {
    // FE/FI à vaporisateur constant (1,5 MAC), circuit sans réinhalation, 5 L/min
    for (agent, mac) in [("isoflurane", 1.3), ("sevoflurane", 2.2)] {
        let mut s = session();
        s.apply(Event::Oxygen(true)).unwrap();
        run(&mut s, 120.0, 10.0);
        s.apply(bolus("propofol", 5.2, AdminRoute::Iv)).unwrap();
        run(&mut s, 90.0, 5.0);
        s.apply(Event::Intubate).unwrap();
        s.apply(Event::SetCircuit(Circuit::NonRebreathing)).unwrap();
        s.apply(Event::FreshGas(5.0)).unwrap();
        s.apply(Event::Vaporizer {
            agent: agent.into(),
            pct: 1.5 * mac,
        })
        .unwrap();
        let mut out = vec![];
        let mut t = 0.0;
        for tgt in [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0] {
            let sn = run(&mut s, (tgt - t) * 60.0, 5.0);
            t = tgt;
            let i = sn.inhalants.iter().find(|i| i.id == agent).unwrap();
            out.push(format!("{tgt} min {:.3}", i.fe / i.fi));
        }
        println!("FE/FI {agent} : {}", out.join(", "));
    }
}

fn recovery() {
    // 60 et 120 min à 1,3 MAC, vaporisateur fermé, O2 4 L/min ; extubation au retour de la déglutition
    for (agent, dur) in [
        ("isoflurane", 60.0),
        ("sevoflurane", 60.0),
        ("isoflurane", 120.0),
        ("sevoflurane", 120.0),
    ] {
        let mut s = session();
        anesthetized(&mut s, agent, 1.3);
        let sn = hold_mac(&mut s, agent, 1.3, dur * 60.0);
        let bm_end = bmac(&sn, agent);
        s.apply(Event::Vaporizer {
            agent: agent.into(),
            pct: 0.0,
        })
        .unwrap();
        s.apply(Event::FreshGas(4.0)).unwrap();
        let t0 = s.time_s();
        let (mut sw, mut head, mut st) = (None, None, None);
        for _ in 0..1800 {
            let sn = s.step(5.0).unwrap();
            if sw.is_none() && sn.reflexes.swallowing {
                sw = Some((sn.t_s - t0) / 60.0);
                s.apply(Event::Extubate).unwrap();
            }
            if head.is_none() && sn.depth.awareness > 0.5 {
                head = Some((sn.t_s - t0) / 60.0);
            }
            if st.is_none() && sn.depth.awareness > 0.8 {
                st = Some((sn.t_s - t0) / 60.0);
                break;
            }
        }
        println!(
            "reveil {agent} {dur} min (MAC cérébrale finale {bm_end:.2}) : déglutition/extubation {sw:.1?} min, tête {head:.1?} min, sternal {st:.1?} min"
        );
    }
}

fn hypothermia() {
    for warm in [false, true] {
        let mut s = session();
        anesthetized(&mut s, "isoflurane", 1.3);
        hold_mac(&mut s, "isoflurane", 1.3, 30.0);
        s.apply(Event::Warming(warm)).unwrap();
        let t0 = s.step(1.0).unwrap().temp_c;
        let mut out = vec![format!("t0 {t0:.2}")];
        for _ in 0..6 {
            let sn = hold_mac(&mut s, "isoflurane", 1.3, 1800.0);
            out.push(format!("{:.0} min {:.2}", sn.t_s / 60.0, sn.temp_c));
        }
        println!("temperature rechauffement={warm} : {}", out.join(", "));
    }
}

fn mac_sparing() {
    // CRI seule (pas de charge), 3 h, épargne de MAC du modèle ; débits en µg/kg/min
    for (agent, rates) in [
        ("fentanyl", vec![0.1, 0.15, 0.3, 0.6, 0.8]),
        ("lidocaine", vec![50.0, 100.0, 200.0]),
        ("ketamine", vec![10.0, 25.0, 50.0, 100.0]),
        ("dexmedetomidine", vec![0.0083, 0.0167, 0.05]),
    ] {
        let mut out = vec![];
        for r in rates {
            let mut s = session();
            s.apply(Event::Cri {
                agent: agent.into(),
                mg_per_kg_h: r * 60.0 / 1000.0,
            })
            .unwrap();
            let a = run(&mut s, 3600.0, 60.0).depth.mac_reduction;
            let b = run(&mut s, 7200.0, 60.0);
            let cp = b.agents.iter().find(|x| x.id == agent).unwrap().cp;
            out.push(format!(
                "{r} µg/kg/min : 60 min {:.0} %, 180 min {:.0} % (Cp {:.4} mg/L)",
                100.0 * a,
                100.0 * b.depth.mac_reduction,
                cp
            ));
        }
        println!("epargne {agent} : {}", out.join(" ; "));
    }
}

fn dex_iv() {
    for d in [0.001, 0.002, 0.005] {
        let mut s = session();
        let b = run(&mut s, 30.0, 10.0);
        let (h0, p0, c0) = {
            let e = b.eval.as_ref().unwrap();
            (e.params.hr, e.hemo.summary.map, e.hemo.summary.co)
        };
        s.apply(Event::Bolus {
            agent: "dexmedetomidine".into(),
            mg_per_kg: d,
            route: AdminRoute::Iv,
            duration_s: Some(30.0),
        })
        .unwrap();
        let mut out = vec![];
        let mut t = 0.0;
        for m in [2.0, 5.0, 15.0, 30.0, 60.0] {
            let sn = run(&mut s, (m - t) * 60.0, 10.0);
            t = m;
            let e = sn.eval.as_ref().unwrap();
            out.push(format!(
                "{m} min FC {:+.0} % PAM {:+.0} % DC {:+.0} %",
                100.0 * (e.params.hr / h0 - 1.0),
                100.0 * (e.hemo.summary.map / p0 - 1.0),
                100.0 * (e.hemo.summary.co / c0 - 1.0)
            ));
        }
        println!("dex IV {} µg/kg : {}", d * 1000.0, out.join(" ; "));
    }
}

fn propofol_dose() {
    // plus petite dose (pas de 0,1 mg/kg, injection en 30 s) qui fait perdre conscience
    let need = |premed: &str| {
        let mut d = 0.5;
        while d < 12.0 {
            let mut s = session();
            match premed {
                "acp+metha" => {
                    s.apply(bolus("acepromazine", 0.02, AdminRoute::Im))
                        .unwrap();
                    s.apply(bolus("methadone", 0.3, AdminRoute::Im)).unwrap();
                }
                "dex5+metha" => {
                    s.apply(bolus("dexmedetomidine", 0.005, AdminRoute::Im))
                        .unwrap();
                    s.apply(bolus("methadone", 0.3, AdminRoute::Im)).unwrap();
                }
                _ => {}
            }
            run(&mut s, 1200.0, 30.0);
            s.apply(bolus("propofol", d, AdminRoute::Iv)).unwrap();
            let mut ok = false;
            for _ in 0..40 {
                if s.step(3.0).unwrap().depth.awareness < 0.05 {
                    ok = true;
                    break;
                }
            }
            if ok {
                return d;
            }
            d += 0.1;
        }
        f64::NAN
    };
    for p in ["aucune", "acp+metha", "dex5+metha"] {
        println!(
            "propofol dose minimale de perte de conscience, prémédication {p} : {:.1} mg/kg",
            need(p)
        );
    }
}

fn atipamezole() {
    let mut s = session();
    let h0 = run(&mut s, 30.0, 10.0).eval.as_ref().unwrap().params.hr;
    s.apply(bolus("dexmedetomidine", 0.01, AdminRoute::Im))
        .unwrap();
    let sn = run(&mut s, 1200.0, 30.0);
    let hd = sn.eval.as_ref().unwrap().params.hr;
    s.apply(bolus("atipamezole", 0.1, AdminRoute::Im)).unwrap();
    let t0 = s.time_s();
    let (mut t_sed, mut t_hr) = (None, None);
    for _ in 0..240 {
        let sn = s.step(5.0).unwrap();
        if t_sed.is_none() && sn.depth.sedation < 0.1 {
            t_sed = Some((sn.t_s - t0) / 60.0);
        }
        if t_hr.is_none() && sn.eval.as_ref().unwrap().params.hr > 0.8 * h0 {
            t_hr = Some((sn.t_s - t0) / 60.0);
        }
    }
    println!(
        "atipamezole 100 µg/kg IM après dex 10 µg/kg IM (20 min) : FC {h0:.0} → {hd:.0} ; sédation < 0,1 à {t_sed:.1?} min ; FC > 80 % de base à {t_hr:.1?} min"
    );
}

fn main() {
    let which: Vec<String> = std::env::args().skip(1).collect();
    let on = |k: &str| which.is_empty() || which.iter().any(|w| w == k);
    if on("hemorragie") {
        hemorrhage(false);
        hemorrhage(true);
    }
    if on("bolus") {
        fluid_bolus();
    }
    if on("apnee") {
        apnea();
    }
    if on("ventilation") {
        oxygen_and_ventilation();
    }
    if on("captation") {
        uptake();
    }
    if on("reveil") {
        recovery();
    }
    if on("temperature") {
        hypothermia();
    }
    if on("epargne") {
        mac_sparing();
    }
    if on("dex") {
        dex_iv();
    }
    if on("propofol") {
        propofol_dose();
    }
    if on("convergence") {
        // contrôle : le dernier des 11 battements de simulate_hemo est-il périodique ? on rejoue les
        // paramètres effectifs (après baroréflexe) retenus par evaluate avec 11 puis 80 battements
        for d in [0.0, 170.0, 340.0, 510.0] {
            let mut p = base();
            p.vol_shift -= d;
            let e = vetsim_core::evaluate(&p).unwrap();
            for nb in [11, 22, 44, 80] {
                let h = vetsim_core::hemo::simulate_hemo(&e.params, nb).unwrap();
                let m = &h.summary;
                println!(
                    "convergence déficit {d} mL, {nb} battements : FC {:.1} PAM {:.1} DC {:.3} FC×VS {:.3} converged {} dérive {:.2e}",
                    e.params.hr,
                    m.map,
                    m.co,
                    e.params.hr * m.sv / 1000.0,
                    m.converged,
                    m.map_drift
                );
            }
        }
    }
    if on("dexcri") {
        // protocoles publiés : charge IV (µg/kg, 10 min) puis CRI (µg/kg/h) ; épargne et Cp à 60 et 90 min
        for (load, rate, src) in [
            (0.5, 0.5, "Pascoe 2006 : −18 %, 0,198 ng/mL"),
            (3.0, 3.0, "Pascoe 2006 : −59 %, 1,903 ng/mL"),
            (2.0, 2.0, "Acevedo-Arcique 2014 : −43,4 %"),
        ] {
            let mut s = session();
            s.apply(Event::Bolus {
                agent: "dexmedetomidine".into(),
                mg_per_kg: load / 1000.0,
                route: AdminRoute::Iv,
                duration_s: Some(600.0),
            })
            .unwrap();
            s.apply(Event::Cri {
                agent: "dexmedetomidine".into(),
                mg_per_kg_h: rate / 1000.0,
            })
            .unwrap();
            let mut out = vec![];
            let mut t = 0.0;
            for m in [30.0, 60.0, 90.0] {
                let sn = run(&mut s, (m - t) * 60.0, 30.0);
                t = m;
                let a = sn
                    .agents
                    .iter()
                    .find(|x| x.id == "dexmedetomidine")
                    .unwrap();
                out.push(format!(
                    "{m} min épargne {:.0} % Cp {:.3} ng/mL Ce {:.3}",
                    100.0 * sn.depth.mac_reduction,
                    a.cp * 1e3,
                    a.ce * 1e3
                ));
            }
            println!(
                "dexcri {load} µg/kg + {rate} µg/kg/h ({src}) : {}",
                out.join(" ; ")
            );
        }
    }
    if on("reveil2") {
        // protocoles publiés : Rastas 2025 (iso FE 1,3 % ≈ 1,0 MAC, 2 h), Lopez 2009 (iso 2,0 % /
        // sévo 3,2 % FE ≈ 1,5 MAC, 2 h) ; trace de la MAC cérébrale après fermeture
        for (agent, m, dur) in [
            ("isoflurane", 1.0, 120.0),
            ("isoflurane", 1.54, 120.0),
            ("sevoflurane", 1.45, 120.0),
        ] {
            let mut s = session();
            anesthetized(&mut s, agent, m);
            hold_mac(&mut s, agent, m, dur * 60.0);
            s.apply(Event::Vaporizer {
                agent: agent.into(),
                pct: 0.0,
            })
            .unwrap();
            s.apply(Event::FreshGas(4.0)).unwrap();
            let t0 = s.time_s();
            let (mut sw, mut head, mut st) = (None, None, None);
            let mut trace = vec![];
            for k in 0..1440 {
                let sn = s.step(5.0).unwrap();
                let tm = (sn.t_s - t0) / 60.0;
                if k % 24 == 23 {
                    trace.push(format!("{tm:.0}′ {:.2}", bmac(&sn, agent)));
                }
                if sw.is_none() && sn.reflexes.swallowing {
                    sw = Some(tm);
                    s.apply(Event::Extubate).unwrap();
                }
                if head.is_none() && sn.depth.awareness > 0.5 {
                    head = Some(tm);
                }
                if st.is_none() && sn.depth.awareness > 0.8 {
                    st = Some(tm);
                    break;
                }
            }
            println!(
                "reveil2 {agent} {m} MAC {dur} min : extubation {sw:.1?}, tête {head:.1?}, sternal {st:.1?} min ; MAC cérébrale {}",
                trace.join(", ")
            );
        }
    }
    if on("pao2") {
        // Itami 2019 : iso 1,3–1,5 % (≈ 1,1 MAC), ventilation contrôlée, O2 ; Kato 2024 : sévo
        // 1,3 MAC en ventilation spontanée, O2 > 95 %
        for (agent, m, ctrl) in [("isoflurane", 1.1, true), ("sevoflurane", 1.3, false)] {
            let mut s = session();
            anesthetized(&mut s, agent, m);
            if ctrl {
                s.apply(Event::SetVentilation(Ventilation::Controlled {
                    rr: 12.0,
                    vt_ml_kg: 12.0,
                }))
                .unwrap();
            }
            let sn = hold_mac(&mut s, agent, m, 2400.0);
            println!(
                "pao2 {agent} {m} MAC {} : FiO2 {:.2} {}",
                if ctrl { "contrôlée" } else { "spontanée" },
                sn.fio2,
                vit(&sn)
            );
        }
    }
    if on("titration") {
        for (a, m) in [("sevoflurane", 1.3), ("isoflurane", 2.0)] {
            let mut s = session();
            anesthetized(&mut s, a, m);
            println!("titration {a} {m} MAC");
            let (sn, bmax) = hold_mac_trace(&mut s, a, m, 3600.0, true);
            println!(
                "  fin : cerveau {:.2}, max {bmax:.2}, {}",
                bmac(&sn, a),
                vit(&sn)
            );
        }
    }
    if on("atipamezole") {
        atipamezole();
    }
}
