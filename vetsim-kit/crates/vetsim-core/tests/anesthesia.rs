//! Module d'anesthésie en temps réel (docs/ANESTHESIE.tex) : invariant d'équilibre, stocks de
//! gaz, absorption des halogénés, profondeur, nociception, réveil, déterminisme.
//! Propriétés qualitatives et ordres publiés ; chaque test peut échouer pour la raison qu'il
//! vérifie (comparaison à un témoin sans le facteur testé).

use std::path::PathBuf;
use vetsim_core::anesthesia::{
    AdminRoute, AnesthesiaModel, Event, InhalantModule, Plane, ProcedureModule, Session, Snapshot,
    Ventilation,
};
use vetsim_core::{evaluate, DrugModule, Error, Params, Patient, PatientModule};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
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

fn session_with(procedure: Option<&str>, seed: u64) -> Session {
    let model =
        AnesthesiaModel::from_json(&read("modules/anesthesia/anesthesia_model.json"), "m").unwrap();
    let agents = dir("modules/anesthesia/agents")
        .iter()
        .map(|s| DrugModule::from_json(s, "a").unwrap())
        .collect();
    let inhalants = dir("modules/anesthesia/inhalants")
        .iter()
        .map(|s| InhalantModule::from_json(s, "i").unwrap())
        .collect();
    let proc = procedure.map(|p| {
        ProcedureModule::from_json(&read(&format!("modules/anesthesia/procedures/{p}.json")), p)
            .unwrap()
    });
    Session::new(base(), agents, inhalants, model, proc, seed).unwrap()
}

fn session() -> Session {
    session_with(None, 1)
}

fn bolus(a: &str, mg: f64, route: AdminRoute) -> Event {
    Event::Bolus {
        agent: a.into(),
        mg_per_kg: mg,
        route,
        duration_s: None,
    }
}

/// Avance de `secs` par pas de `dt` et renvoie le dernier instantané.
fn run(s: &mut Session, secs: f64, dt: f64) -> Snapshot {
    let n = (secs / dt).round().max(1.0) as usize;
    let mut last = None;
    for _ in 0..n {
        last = Some(s.step(dt).unwrap());
    }
    last.unwrap()
}

fn hr(s: &Snapshot) -> f64 {
    s.eval.as_ref().unwrap().params.hr
}
fn map(s: &Snapshot) -> f64 {
    s.eval.as_ref().unwrap().hemo.summary.map
}

/// Induction propofol, intubation, isoflurane `pct` %, circuit filtre à 2 L/min.
fn anesthetized(s: &mut Session, agent: &str, pct: f64) {
    s.apply(Event::Oxygen(true)).unwrap();
    run(s, 120.0, 10.0);
    s.apply(bolus("propofol", 5.2, AdminRoute::Iv)).unwrap();
    // délai d'intubation : 140 s après 5 mg/kg chez Cattai 2018 ; déglutition levée vers 90 s ici
    run(s, 90.0, 5.0);
    s.apply(Event::Intubate).unwrap();
    s.apply(Event::FreshGas(2.0)).unwrap();
    s.apply(Event::Vaporizer {
        agent: agent.into(),
        pct,
    })
    .unwrap();
}

#[test]
fn session_without_drugs_reproduces_eval() {
    let e0 = evaluate(&base()).unwrap();
    let mut s = session();
    for _ in 0..4 {
        let sn = s.step(15.0).unwrap();
        let e = sn.eval.as_ref().unwrap();
        let rel = |a: f64, b: f64| ((a - b) / b).abs();
        assert!(rel(e.params.hr, e0.params.hr) < 1e-9);
        assert!(rel(e.hemo.summary.map, e0.hemo.summary.map) < 1e-9);
        assert!(rel(e.metabolic.art.pco2, e0.metabolic.art.pco2) < 1e-9);
        assert!(rel(e.metabolic.art.pao2, e0.metabolic.art.pao2) < 1e-9);
        assert!((sn.temp_c - 38.5).abs() < 1e-9);
        assert_eq!(sn.depth.plane, Plane::Awake);
        assert!(sn.alarms.is_empty(), "{:?}", sn.alarms);
    }
}

#[test]
fn apnea_raises_paco2_and_preoxygenation_delays_desaturation() {
    // délai de désaturation (SpO2 < 90 %) en apnée, à l'air puis après préoxygénation
    // sans préoxygénation : apnée dès l'intubation (PAO2 de l'air ambiant) ; avec : 3 min d'O2
    let desat = |preox: bool| {
        let mut s = session();
        if preox {
            s.apply(Event::Oxygen(true)).unwrap();
            run(&mut s, 120.0, 10.0);
        }
        s.apply(bolus("propofol", 6.0, AdminRoute::Iv)).unwrap();
        run(&mut s, 90.0, 5.0);
        s.apply(Event::Intubate).unwrap();
        s.apply(Event::FreshGas(3.0)).unwrap();
        if preox {
            s.apply(Event::SetVentilation(Ventilation::Controlled {
                rr: 12.0,
                vt_ml_kg: 12.0,
            }))
            .unwrap();
            run(&mut s, 180.0, 10.0);
        }
        let p0 = s.step(1.0).unwrap().paco2;
        s.apply(Event::SetVentilation(Ventilation::Controlled {
            rr: 0.0,
            vt_ml_kg: 0.0,
        }))
        .unwrap();
        let mut t = 0.0;
        let mut p_end = p0;
        let mut apnea = false;
        while t < 900.0 {
            let sn = s.step(5.0).unwrap();
            t += 5.0;
            p_end = sn.paco2;
            apnea |= sn.apnea;
            if sn.eval.as_ref().is_none_or(|e| e.metabolic.art.sao2 < 90.0) {
                break;
            }
        }
        assert!(apnea, "apnée non signalée");
        (t, p_end - p0)
    };
    let (t_air, dp_air) = desat(false);
    let (t_o2, dp_o2) = desat(true);
    assert!(dp_air > 0.0 && dp_o2 > 10.0, "PaCO2 : +{dp_air} / +{dp_o2}");
    assert!(
        t_o2 > 3.0 * t_air,
        "désaturation {t_air} s à l'air, {t_o2} s après O2"
    );
}

#[test]
fn sevoflurane_equilibrates_faster_than_isoflurane() {
    // FA/FI à 5 min, même multiple de MAC au vaporisateur, gaz frais élevé
    let ratio = |agent: &str, pct: f64| {
        let mut s = session();
        anesthetized(&mut s, agent, pct);
        s.apply(Event::FreshGas(5.0)).unwrap();
        let sn = run(&mut s, 300.0, 10.0);
        let g = sn.inhalants.iter().find(|g| g.id == agent).unwrap();
        g.fe / g.fi
    };
    let iso = ratio("isoflurane", 1.3 * 1.5);
    let sevo = ratio("sevoflurane", 2.2 * 1.5);
    assert!(
        sevo > iso + 0.05,
        "FA/FI sévoflurane {sevo}, isoflurane {iso}"
    );
    assert!(iso > 0.3 && sevo < 1.0);
}

#[test]
fn low_fresh_gas_flow_slows_deepening() {
    let fe = |fgf: f64| {
        let mut s = session();
        anesthetized(&mut s, "isoflurane", 2.5);
        s.apply(Event::FreshGas(fgf)).unwrap();
        run(&mut s, 300.0, 10.0).inhalants[0].fe
    };
    assert!(fe(4.0) > 1.3 * fe(0.5));
}

#[test]
fn isoflurane_depresses_blood_pressure_and_ventilation_dose_dependently() {
    let mut s = session();
    anesthetized(&mut s, "isoflurane", 1.6);
    let light = run(&mut s, 1500.0, 30.0);
    // palier profond ≈ 1,5 MAC cérébrale (3 % ne donnait que 1,15 MAC : avec la compensation au pH
    // recalée le 20/11/2026, la PaCO2 n'y monte que de 2,8 mmHg, conforme à la courbe de Steffey)
    s.apply(Event::Vaporizer {
        agent: "isoflurane".into(),
        pct: 4.0,
    })
    .unwrap();
    let deep = run(&mut s, 1500.0, 30.0);
    assert!(deep.inhalants[0].fe > light.inhalants[0].fe);
    assert!(
        map(&deep) < map(&light) - 5.0,
        "{} / {}",
        map(&deep),
        map(&light)
    );
    assert!(
        deep.paco2 > light.paco2 + 3.0,
        "PaCO2 {} / {} ; Fe {} / {} ; MAC cérébrale {} / {} ; FR {} / {}",
        deep.paco2,
        light.paco2,
        deep.inhalants[0].fe,
        light.inhalants[0].fe,
        deep.inhalants[0].brain_mac,
        light.inhalants[0].brain_mac,
        deep.rr,
        light.rr
    );
    assert!(deep.depth.delta > light.depth.delta);
}

#[test]
fn fentanyl_spares_mac() {
    let delta = |fenta: bool| {
        let mut s = session();
        anesthetized(&mut s, "isoflurane", 1.8);
        if fenta {
            s.apply(bolus("fentanyl", 0.005, AdminRoute::Iv)).unwrap();
            s.apply(Event::Cri {
                agent: "fentanyl".into(),
                mg_per_kg_h: 0.01,
            })
            .unwrap();
        }
        let sn = run(&mut s, 1200.0, 30.0);
        (sn.depth.delta, sn.depth.mac_reduction, sn.inhalants[0].fe)
    };
    let (d0, r0, fe0) = delta(false);
    let (d1, r1, fe1) = delta(true);
    assert_eq!(r0, 0.0);
    // 9 à 10 µg/kg/h : −35 % de MAC publiés à l'état stable (Ueyama 2009) ; 20 min ici
    assert!(r1 > 0.2, "épargne {r1}");
    assert!(d1 > 1.3 * d0, "Δ {d0} → {d1} (Fe {fe0} / {fe1})");
}

#[test]
fn surgical_stimulus_activates_sympathetic_and_analgesia_blunts_it() {
    // réponse de la FC et de la PAM au pédicule ovarien, avec et sans fentanyl, avec bloc
    let response = |fenta: bool, block: bool| {
        let mut s = session_with(Some("ovariohysterectomy"), 3);
        anesthetized(&mut s, "isoflurane", 1.7);
        if fenta {
            s.apply(bolus("fentanyl", 0.005, AdminRoute::Iv)).unwrap();
            s.apply(Event::Cri {
                agent: "fentanyl".into(),
                mg_per_kg_h: 0.01,
            })
            .unwrap();
        }
        if block {
            s.apply(Event::LocalBlock {
                agent: "bupivacaine".into(),
                territory: "viscera".into(),
            })
            .unwrap();
        }
        let before = run(&mut s, 1200.0, 30.0);
        s.apply(Event::ProcedureStep(2)).unwrap();
        let during = run(&mut s, 90.0, 5.0);
        (
            hr(&during) - hr(&before),
            map(&during) - map(&before),
            during.nociception.nu,
        )
    };
    let (dh0, dm0, nu0) = response(false, false);
    let (dh1, dm1, nu1) = response(true, false);
    let (_, _, nu2) = response(false, true);
    assert!(
        dh0 > 3.0 && dm0 > 3.0,
        "réponse sans analgésie : FC +{dh0}, PAM +{dm0}"
    );
    assert!(
        nu1 < 0.75 * nu0 && dh1 < dh0 && dm1 < dm0,
        "fentanyl : ν {nu0} → {nu1}, FC +{dh0} → +{dh1}, PAM +{dm0} → +{dm1}"
    );
    assert!(nu2 < 0.2 * nu0, "bloc : ν {nu0} → {nu2}");
}

#[test]
fn light_plane_moves_under_supramaximal_stimulus() {
    let moved = |pct: f64| {
        let mut s = session_with(None, 7);
        anesthetized(&mut s, "isoflurane", pct);
        run(&mut s, 1500.0, 30.0);
        s.apply(Event::Stimulus {
            intensity: 1.0,
            duration_s: 60.0,
            territory: "skin".into(),
        })
        .unwrap();
        let sn = run(&mut s, 30.0, 5.0);
        (sn.reflexes.movement, sn.depth.delta)
    };
    // plan léger : inconscient mais sous 1 MAC ; plan profond : Δ > 1,3. Sous isoflurane, la
    // conscience se perd à 0,77 MAC (MAC-éveil, Hofmeister 2008) : réglage 1,95 % (≈ 0,84 MAC ;
    // 1,6 % avant le 24/11/2026, soit 0,69 MAC, sous le MAC-éveil publié de l'isoflurane).
    let (m_light, d_light) = moved(1.95);
    let (m_deep, d_deep) = moved(4.0);
    assert!(
        d_light > 0.65 && d_light < 0.95 && d_deep > 1.3,
        "Δ {d_light} / {d_deep}"
    );
    assert!(m_light && !m_deep);
}

#[test]
fn sevoflurane_recovers_faster_than_isoflurane() {
    // 60 min à 1,5 MAC télé-expiratoire (vaporisateur réglé toutes les 30 s sur la FE visée, comme
    // les études publiées : Lopez 2009, FE isoflurane 2,0 % et sévoflurane 3,2 %), arrêt du
    // vaporisateur, délai jusqu'au retour de la déglutition. Avant le 24/11/2026, le vaporisateur
    // restait fixe : l'agent le plus soluble finissait moins profond (0,96 contre 1,25 MAC).
    let recovery = |agent: &str, mac: f64| {
        let mut s = session();
        let mut dial = 1.5 * mac * 1.15;
        anesthetized(&mut s, agent, dial);
        for _ in 0..120 {
            let sn = s.step(30.0).unwrap();
            let fe = sn.inhalants.iter().find(|g| g.id == agent).unwrap().fe;
            dial = (dial + 0.8 * (1.5 * mac - fe)).clamp(0.0, 8.0);
            s.apply(Event::Vaporizer {
                agent: agent.into(),
                pct: dial,
            })
            .unwrap();
        }
        s.apply(Event::Vaporizer {
            agent: agent.into(),
            pct: 0.0,
        })
        .unwrap();
        s.apply(Event::FreshGas(4.0)).unwrap();
        let t0 = s.time_s();
        for _ in 0..240 {
            let sn = s.step(10.0).unwrap();
            if sn.reflexes.swallowing {
                return sn.t_s - t0;
            }
        }
        f64::INFINITY
    };
    let iso = recovery("isoflurane", 1.3);
    let sevo = recovery("sevoflurane", 2.2);
    assert!(
        iso.is_finite() && sevo < iso,
        "isoflurane {iso} s, sévoflurane {sevo} s"
    );
}

#[test]
fn atipamezole_reverses_dexmedetomidine() {
    let sed = |rev: bool| {
        let mut s = session();
        s.apply(bolus("dexmedetomidine", 0.01, AdminRoute::Im))
            .unwrap();
        let peak = run(&mut s, 1200.0, 30.0);
        if rev {
            s.apply(bolus("atipamezole", 0.1, AdminRoute::Im)).unwrap();
        }
        let after = run(&mut s, 600.0, 30.0);
        (
            peak.depth.sedation,
            after.depth.sedation,
            hr(&peak),
            hr(&after),
        )
    };
    let (p0, a0, h0, _) = sed(false);
    let (_, a1, _, h1) = sed(true);
    assert!(p0 > 0.3, "sédation au pic {p0}");
    assert!(h0 < 0.7 * 87.8, "bradycardie {h0}");
    assert!(a1 < 0.5 * a0, "sédation {a0} → {a1} sous atipamézole");
    assert!(h1 > h0);
}

#[test]
fn premedication_reduces_propofol_requirement() {
    // pic d'indice de perte de conscience après 2,6 mg/kg de propofol, avec et sans prémédication
    let peak = |premed: bool| {
        let mut s = session();
        if premed {
            s.apply(bolus("dexmedetomidine", 0.005, AdminRoute::Im))
                .unwrap();
            s.apply(bolus("methadone", 0.3, AdminRoute::Im)).unwrap();
        }
        run(&mut s, 1200.0, 30.0);
        s.apply(bolus("propofol", 2.6, AdminRoute::Iv)).unwrap();
        (0..30)
            .map(|_| s.step(4.0).unwrap().depth.loc_index)
            .fold(0.0, f64::max)
    };
    let alone = peak(false);
    let pre = peak(true);
    assert!(
        alone < 0.65 && pre > 0.65,
        "indice {alone} seul, {pre} prémédiqué"
    );
}

#[test]
fn uncontrolled_nociception_sensitizes_and_ketamine_limits_it() {
    let sens = |ket: bool| {
        let mut s = session();
        anesthetized(&mut s, "isoflurane", 1.8);
        if ket {
            s.apply(bolus("ketamine", 0.5, AdminRoute::Iv)).unwrap();
            s.apply(Event::Cri {
                agent: "ketamine".into(),
                mg_per_kg_h: 0.6,
            })
            .unwrap();
        }
        run(&mut s, 600.0, 30.0);
        s.apply(Event::Stimulus {
            intensity: 1.2,
            duration_s: 900.0,
            territory: "skin".into(),
        })
        .unwrap();
        run(&mut s, 900.0, 30.0).nociception.sensitization
    };
    let s0 = sens(false);
    let s1 = sens(true);
    assert!(s0 > 0.05, "sensibilisation {s0}");
    assert!(s1 < 0.8 * s0, "kétamine : {s0} → {s1}");
}

#[test]
fn anesthesia_cools_and_warming_limits_it() {
    let t = |warm: bool| {
        let mut s = session();
        anesthetized(&mut s, "isoflurane", 1.8);
        s.apply(Event::Warming(warm)).unwrap();
        run(&mut s, 3600.0, 60.0).temp_c
    };
    let cold = t(false);
    let warm = t(true);
    assert!(cold < 37.8, "{cold} °C");
    assert!(warm > cold + 0.5);
}

#[test]
fn hemorrhage_lowers_pressure_and_hematocrit_dilutes_with_fluids() {
    let mut s = session();
    let before = run(&mut s, 30.0, 10.0);
    s.apply(Event::Hemorrhage(20.0)).unwrap();
    let bled = run(&mut s, 30.0, 10.0);
    assert!(map(&bled) < map(&before) - 3.0);
    s.apply(Event::FluidBolus(30.0)).unwrap();
    let filled = run(&mut s, 30.0, 10.0);
    let hct = |x: &Snapshot| x.eval.as_ref().unwrap().metabolic.chem.hct;
    assert!(map(&filled) > map(&bled));
    assert!(hct(&filled) < hct(&before) - 2.0);
}

#[test]
fn deterministic_for_a_seed() {
    let go = |seed| {
        let mut s = session_with(Some("castration"), seed);
        anesthetized(&mut s, "sevoflurane", 2.0);
        run(&mut s, 300.0, 30.0);
        s.apply(Event::ProcedureStep(1)).unwrap();
        run(&mut s, 60.0, 5.0)
    };
    assert_eq!(go(5), go(5));
}

#[test]
fn record_and_milestones() {
    let mut s = session();
    anesthetized(&mut s, "sevoflurane", 3.0);
    run(&mut s, 900.0, 30.0);
    s.apply(Event::Vaporizer {
        agent: "sevoflurane".into(),
        pct: 0.0,
    })
    .unwrap();
    let last = run(&mut s, 1200.0, 30.0);
    assert!(s.record().len() >= 7);
    assert!(
        last.depth.loc_index < 0.5,
        "indice {} apnée {} PaCO2 {} Fe {:?}",
        last.depth.loc_index,
        last.apnea,
        last.paco2,
        last.inhalants
    );
    let ms = s.milestones();
    assert!(ms.loc.is_some() && ms.intubation.is_some() && ms.vaporizer_off.is_some());
    assert!(ms.swallowing.unwrap() > ms.vaporizer_off.unwrap());
    assert!(s.log().len() >= 5);
}

#[test]
fn invalid_actions_are_rejected() {
    let mut s = session();
    // chien vigile : la déglutition empêche l'intubation
    assert!(s.apply(Event::Intubate).is_err());
    assert!(matches!(
        s.apply(bolus("inconnu", 1.0, AdminRoute::Iv)),
        Err(Error::UnknownDrug(_))
    ));
    assert!(s
        .apply(Event::SetVentilation(Ventilation::Controlled {
            rr: 10.0,
            vt_ml_kg: 12.0
        }))
        .is_err());
    assert!(s.apply(bolus("fentanyl", 0.01, AdminRoute::Im)).is_err());
    assert!(s.apply(bolus("propofol", -1.0, AdminRoute::Iv)).is_err());
    assert!(s.apply(Event::ProcedureStep(0)).is_err());
    assert!(s.step(0.0).is_err());
}

#[test]
fn modules_load() {
    for s in dir("modules/anesthesia/agents") {
        let m = DrugModule::from_json(&s, "a").unwrap();
        assert!(m.anesthesia.is_some(), "{}", m.id);
    }
    // bloc anesthesia refusé s'il est invalide
    let bad =
        read("modules/anesthesia/agents/propofol.json").replace("\"hypnotic\"", "\"magique\"");
    assert!(DrugModule::from_json(&bad, "bad").is_err());
    let bad = read("modules/anesthesia/anesthesia_model.json")
        .replace("\"co_frac\": 0.75", "\"co_frac\": 0.5");
    assert!(AnesthesiaModel::from_json(&bad, "bad").is_err());
}

#[test]
fn fast_propofol_injection_causes_apnea_slow_does_not() {
    // même dose, vitesse d'injection différente (Murison 2001, Bigby 2017)
    let apnea_s = |dur: f64| {
        let mut s = session();
        s.apply(Event::Oxygen(true)).unwrap();
        s.apply(Event::Bolus {
            agent: "propofol".into(),
            mg_per_kg: 5.2,
            route: AdminRoute::Iv,
            duration_s: Some(dur),
        })
        .unwrap();
        (0..90).filter(|_| s.step(2.0).unwrap().apnea).count() as f64 * 2.0
    };
    let fast = apnea_s(10.0);
    let slow = apnea_s(150.0);
    assert!(fast >= 30.0, "apnée {fast} s en injection rapide");
    assert!(slow < 0.5 * fast, "apnée {slow} s en injection lente");
}

#[test]
fn breathing_resumes_when_co2_rises_after_hyperventilation() {
    // seuil apnéique : apnée après hyperventilation, reprise quand la PaCO2 remonte
    let mut s = session();
    anesthetized(&mut s, "isoflurane", 2.0);
    s.apply(Event::SetVentilation(Ventilation::Controlled {
        rr: 20.0,
        vt_ml_kg: 15.0,
    }))
    .unwrap();
    let low = run(&mut s, 1200.0, 30.0);
    assert!(low.paco2 < 25.0, "PaCO2 {} en hyperventilation", low.paco2);
    s.apply(Event::SetVentilation(Ventilation::Spontaneous))
        .unwrap();
    let first = s.step(5.0).unwrap();
    assert!(first.apnea, "apnée attendue au sevrage, VA {}", first.va);
    let later = run(&mut s, 600.0, 15.0);
    assert!(
        !later.apnea && later.paco2 > first.paco2 + 10.0,
        "reprise : PaCO2 {} VA {}",
        later.paco2,
        later.va
    );
}

#[test]
fn untreated_apnea_desaturates_then_causes_hypoxic_arrest() {
    // apnée totale (ventilateur à 0) chez un chien intubé sous O2 : réserve pulmonaire, puis
    // désaturation, dépression hypoxique et arrêt ; sans ventilation, le décès est inévitable
    let mut s = session();
    anesthetized(&mut s, "isoflurane", 1.8);
    run(&mut s, 300.0, 30.0);
    s.apply(Event::SetVentilation(Ventilation::Controlled {
        rr: 0.0,
        vt_ml_kg: 0.0,
    }))
    .unwrap();
    let (mut t_desat, mut t_dead) = (None, None);
    for _ in 0..180 {
        let sn = s.step(10.0).unwrap();
        let spo2 = sn.eval.as_ref().map_or(0.0, |e| e.metabolic.art.sao2);
        if t_desat.is_none() && spo2 < 90.0 {
            t_desat = Some(sn.t_s);
        }
        if sn.dead.is_some() {
            assert_eq!(sn.dead, Some("hypoxic_arrest"));
            t_dead = Some(sn.t_s);
            break;
        }
    }
    let (d, a) = (
        t_desat.expect("désaturation"),
        t_dead.expect("arrêt hypoxique"),
    );
    assert!(a > d + 60.0, "arrêt {a} s, désaturation {d} s");
}
