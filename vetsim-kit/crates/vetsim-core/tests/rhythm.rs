//! Couche de conduction : rejeu des tirages de la référence Python et comparaison des événements.
//! Référence : `tests/fixtures/python_reference_rhythm.json` (`tests/make_rhythm_fixture.py`).

use serde_json::Value as Json;
use std::path::PathBuf;
use vetsim_core::rhythm::{is_trivial, simulate_conduction, summarize, Origin};
use vetsim_core::{Patient, PatientModule};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * f64::max(1.0, f64::max(a.abs(), b.abs()))
}

#[test]
fn conduction_matches_python_replay() {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let fx: Json = serde_json::from_str(&read(
        "crates/vetsim-core/tests/fixtures/python_reference_rhythm.json",
    ))
    .unwrap();
    let mut bad = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut p = Patient::healthy(&pm, case["bw"].as_f64().unwrap())
            .unwrap()
            .params;
        for (k, v) in case["over"].as_object().unwrap() {
            *p.get_mut(k).unwrap() = v.as_f64().unwrap();
        }
        let draws: Vec<f64> = case["draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        let mut i = 0;
        let mut rand = || {
            let u = draws[i];
            i += 1;
            u
        };
        let (v_eff, per, t_end) = (
            case["v_eff"].as_f64().unwrap(),
            case["sinus_period"].as_f64().unwrap(),
            case["t_end"].as_f64().unwrap(),
        );
        let ev = simulate_conduction(&p, v_eff, per, t_end, &mut rand, None);
        if i != draws.len() {
            bad.push(format!("{id} : {i} tirages consommés / {}", draws.len()));
        }
        let ea = case["atrial"].as_array().unwrap();
        if ea.len() != ev.atrial.len() {
            bad.push(format!(
                "{id} : {} activations A / Python {}",
                ev.atrial.len(),
                ea.len()
            ));
        } else {
            for (a, e) in ev.atrial.iter().zip(ea) {
                if !close(a.t, e[0].as_f64().unwrap()) || a.organized != e[1].as_bool().unwrap() {
                    bad.push(format!("{id} : A {a:?} / {e}"));
                    break;
                }
            }
        }
        let ev_v = case["ventricular"].as_array().unwrap();
        if ev_v.len() != ev.ventricular.len() {
            bad.push(format!(
                "{id} : {} activations V / Python {}",
                ev.ventricular.len(),
                ev_v.len()
            ));
            continue;
        }
        for (b, e) in ev.ventricular.iter().zip(ev_v) {
            let opt = |x: Option<f64>, j: &Json| match (x, j.as_f64()) {
                (None, None) => true,
                (Some(a), Some(b)) => close(a, b),
                _ => false,
            };
            if !close(b.t, e[0].as_f64().unwrap())
                || b.origin.as_str() != e[1].as_str().unwrap()
                || !opt(b.t_a, &e[2])
                || !opt(b.pr, &e[3])
                || b.wide != e[4].as_bool().unwrap()
            {
                bad.push(format!("{id} : V {b:?} / {e}"));
                break;
            }
        }
        if is_trivial(&p, v_eff, per) != case["trivial"].as_bool().unwrap() {
            bad.push(format!("{id} : trivialité différente"));
        }
    }
    assert!(bad.is_empty(), "{} écarts\n{}", bad.len(), bad.join("\n"));
}

/// Contrôle d'échec : un influx bloqué ne doit pas produire d'activation ventriculaire ; sans
/// foyer d'échappement, le BAV complet donne l'asystolie (le mécanisme décide, pas un réglage).
#[test]
fn complete_block_without_escape_is_asystole() {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let mut p = Patient::healthy(&pm, 20.0).unwrap().params;
    p.hp_conduction = 0.0;
    let ev = simulate_conduction(&p, 0.6, 60.0 / 88.0, 30.0, &mut || 0.5, None);
    let s = summarize(&ev, 5.0, 30.0);
    assert!(s
        .origins
        .iter()
        .all(|(o, _)| *o == Origin::VentricularEscape));
    p.vent_escape_rate = 0.0;
    let ev = simulate_conduction(&p, 0.6, 60.0 / 88.0, 30.0, &mut || 0.5, None);
    assert!(ev.ventricular.is_empty());
}

/// Suppression par entraînement (Tanahashi 1979 ; Neely 1985) : après un long rythme sinusal,
/// l'arrêt du sinus est suivi d'une pause allongée avant le premier échappement jonctionnel, puis
/// le foyer bat à son propre cycle. En bloc complet d'emblée, aucune pause anormale.
#[test]
fn overdrive_suppression_lengthens_first_escape_only() {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let p = Patient::healthy(&pm, 20.0).unwrap().params;
    // 120 s de sinus à 90/min, puis arrêt sinusal
    let period = 60.0 / 90.0;
    let sinus: Vec<f64> = (0..180).map(|i| i as f64 * period).collect();
    let last_sinus = *sinus.last().unwrap();
    let c = simulate_conduction(&p, p.v0, period, 140.0, &mut || 0.0, Some(&sinus));
    let esc: Vec<f64> = c
        .ventricular
        .iter()
        .filter(|b| b.origin == Origin::Junctional)
        .map(|b| b.t)
        .collect();
    let jc = 60.0 / p.junct_rate;
    let last_v = c
        .ventricular
        .iter()
        .filter(|b| b.origin == Origin::Sinus)
        .map(|b| b.t)
        .fold(f64::MIN, f64::max);
    assert!(last_v > last_sinus, "{last_v}");
    let pause = esc[0] - last_v;
    // f̄ ≈ 1,5 /s, cycle 1,28 s : × (1 + 1,8·(1,91 − 1)) ≈ 2,6 → pause ≈ 3,3 s
    let f = 1.0 / period;
    let expected = jc * (1.0 + p.esc_sup_gain * (jc * f - 1.0));
    assert!(
        (pause - expected).abs() < 0.05 * expected,
        "pause {pause} / {expected}"
    );
    assert!(pause > 2.5 * jc, "pause {pause}");
    // les intervalles suivants se raccourcissent peu à peu vers le cycle propre : la suppression
    // s'efface sur des dizaines de battements (66 à 100 en rythme jonctionnel, Neely 1985)
    let rr: Vec<f64> = esc.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(
        rr[0] < pause && rr.windows(2).all(|w| w[1] <= w[0] + 1e-9),
        "{rr:?}"
    );
    assert!(rr.iter().all(|&x| x >= jc - 1e-9), "{rr:?}");

    // bloc infranodal complet d'emblée : échappement ventriculaire à son cycle propre
    let mut q = p.clone();
    q.hp_conduction = 0.0;
    let c = simulate_conduction(&q, q.v0, period, 30.0, &mut || 0.0, None);
    let vc = 60.0 / q.vent_escape_rate;
    for w in c.ventricular.windows(2) {
        assert!((w[1].t - w[0].t - vc).abs() < 1e-9);
    }
    assert!((c.ventricular[0].t - vc).abs() < 1e-9);
}

/// Modulation autonome des foyers d'échappement (Tanahashi 1979) : en bloc sino-atrial complet,
/// le rythme jonctionnel s'accélère sous atropine et ralentit sous bêtabloquant.
#[test]
fn escape_rhythm_follows_autonomic_tone() {
    let pm = PatientModule::from_json(&read("modules/healthy_dog.json"), "p").unwrap();
    let mut p = Patient::healthy(&pm, 20.0).unwrap().params;
    p.sa_block = 1.0;
    let rate = |q: &vetsim_core::Params| {
        let e = vetsim_core::evaluate(q).unwrap();
        let c = e.conduction.clone().unwrap();
        assert!(c.ventricular.iter().all(|b| b.origin == Origin::Junctional));
        e.params.junct_rate
    };
    let base = rate(&p);
    let mut a = p.clone();
    a.vagal_block = 1.0;
    let mut b = p.clone();
    b.beta_block = 1.0;
    let (ra, rb) = (rate(&a), rate(&b));
    assert!(
        base > p.vent_escape_rate,
        "jonctionnel {base} au-dessus du ventriculaire"
    );
    assert!(ra > 1.15 * base, "atropine {ra} / {base}");
    assert!(rb < 0.9 * base, "bêtabloquant {rb} / {base}");
}
