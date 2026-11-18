//! Relation force-intervalle et dyssynchronie (hemo::beat_schedule, simulate_hemo_beats).
//!
//! Séquence construite à la main : rythme sinusal régulier, puis une extrasystole ventriculaire
//! précoce (QRS large) suivie de sa pause compensatrice (l'onde P suivante est bloquée).
//! 1. Rythme régulier : amplitude 1 pour chaque battement (régime établi, le périodique est retrouvé).
//! 2. Extrasystole : amplitude < 1 (restitution × dyssynchronie) ; battement post-pause > 1
//!    (potentialisation post-extrasystolique).
//! 3. Hémodynamique : l'extrasystole précoce n'éjecte presque rien (non productive) ; le
//!    battement post-pause éjecte plus qu'un battement sinusal.

use std::path::PathBuf;
use vetsim_core::hemo::{beat_schedule, simulate_hemo_beats};
use vetsim_core::rhythm::{AtrialEvent, Conduction, Origin, VentricularEvent};
use vetsim_core::{Params, Patient, PatientModule};

const PERIOD: f64 = 0.68;
const PR: f64 = 0.1;

fn params() -> Params {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/healthy_dog.json");
    let m = PatientModule::from_json(&std::fs::read_to_string(p).unwrap(), "p").unwrap();
    Patient::healthy(&m, 20.0).unwrap().params
}

fn sinus(t: f64) -> VentricularEvent {
    VentricularEvent {
        t,
        origin: Origin::Sinus,
        t_a: Some(t - PR),
        pr: Some(PR),
        wide: false,
    }
}

/// Sinusal régulier sur [0, 30 s) ; si `vpc` : extrasystole couplée à `coupling` s après le
/// battement sinusal d'indice 30, battement sinusal suivant bloqué (pause compensatrice).
fn sequence(vpc: Option<f64>) -> (Conduction, usize) {
    let n = (30.0 / PERIOD) as usize;
    let mut c = Conduction::default();
    let mut i_vpc = usize::MAX;
    for k in 0..n {
        let t = PR + k as f64 * PERIOD;
        c.atrial.push(AtrialEvent {
            t: t - PR,
            organized: true,
        });
        match vpc {
            Some(_) if k == 31 => continue, // onde P bloquée : nœud AV réfractaire (rétrograde)
            _ => c.ventricular.push(sinus(t)),
        }
        if let Some(cpl) = vpc {
            if k == 30 {
                i_vpc = c.ventricular.len();
                c.ventricular.push(VentricularEvent {
                    t: t + cpl,
                    origin: Origin::Ectopic,
                    t_a: None,
                    pr: None,
                    wide: true,
                });
            }
        }
    }
    (c, i_vpc)
}

#[test]
fn regular_rhythm_keeps_unit_amplitude() {
    let p = params();
    let (c, _) = sequence(None);
    let (beats, _) = beat_schedule(&c, PERIOD, &p);
    for b in &beats {
        assert!(
            (b.c - 1.0).abs() < 1e-12,
            "amplitude {} en rythme régulier",
            b.c
        );
    }
}

#[test]
fn premature_beat_is_weak_and_post_pause_beat_is_potentiated() {
    let p = params();
    let (c, i) = sequence(Some(0.40));
    let (beats, _) = beat_schedule(&c, PERIOD, &p);
    assert!((beats[i - 1].c - 1.0).abs() < 1e-9);
    assert!(beats[i].c < 0.8 * p.ees_wide, "extrasystole {}", beats[i].c);
    assert!(beats[i + 1].c > 1.05, "post-pause {}", beats[i + 1].c);
    // retour progressif au régime établi
    assert!(beats[i + 1].c > beats[i + 2].c && beats[i + 2].c > 1.0);
    assert!((beats[i + 12].c - 1.0).abs() < 0.01);
}

#[test]
fn early_vpc_is_not_productive() {
    let p = params();
    let (c, i) = sequence(Some(0.40));
    let r = simulate_hemo_beats(&p, &c, 30.0, 12.0, PERIOD).unwrap();
    // les battements mécaniques débutent au début du QRS + délai électromécanique
    let t_vpc = c.ventricular[i].t + vetsim_core::hemo::mech_offset(&p, true);
    let k = r
        .beats
        .iter()
        .position(|b| (b.t - t_vpc).abs() < 1e-12)
        .unwrap();
    let (before, vpc, after) = (r.beats[k - 1], r.beats[k], r.beats[k + 1]);
    assert!(
        vpc.sv < 0.25 * before.sv,
        "VES de l'extrasystole {} / sinusal {}",
        vpc.sv,
        before.sv
    );
    assert!(
        after.sv > 1.1 * before.sv,
        "post-pause {} / {}",
        after.sv,
        before.sv
    );
    // VTD de l'extrasystole : remplissage écourté, pas le remplissage de la pause qui suit
    assert!(vpc.edv < before.edv);
}
