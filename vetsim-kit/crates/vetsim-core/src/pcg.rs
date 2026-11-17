//! Auscultation : synthèse des bruits du cœur du chien (port fidèle de
//! `reference/python/signals_pcg_v5.py::make_pcg`, référence normative).
//!
//! Par battement (instants du module autonome, donc avec l'arythmie sinusale respiratoire) :
//! - **B1** à la fermeture mitrale : 3 sinusoïdes amorties de fréquences voisines (spectre
//!   large), attaque de 4 ms, phases aléatoires ; amplitude liée au dP/dt max du VG ;
//! - **B2** aortique puis pulmonaire, dédoublé à l'inspiration ; la systole raccourcit peu
//!   quand le RR diminue ; amplitude liée à la pression de fermeture aortique ;
//! - **B3** si la POG dépasse 15 mmHg (remplissage rapide) ;
//! - **souffle** : bruit filtré dont l'enveloppe suit √(débit régurgitant simulé) ;
//! - variabilité battement à battement, modulation respiratoire, bruit de fond de contact.
//!
//! Toutes les constantes viennent de `modules/auscultation.json` ([`AuscultationConfig`]).
//! Calage fondé sur DEUX enregistrements (bibliothèque TiHo, CC BY-NC-ND 4.0, aucun
//! enregistrement embarqué) : limite de validité, voir docs/AUSCULTATION.md.

use crate::autonomic::{arange, interp};
use crate::dsp::{butter_bandpass, interp_right, std};
use crate::error::{Error, Issue, LoadError};
use crate::math;
use crate::rng::NoiseSource;
use crate::signals::{beat_events, sosfilt, Murmur, PcgInputs};
use serde_json::Value as Json;
use std::f64::consts::PI;

/// Composante d'un bruit cardiaque : rapport de fréquence, poids, facteur de constante de temps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundComponent {
    /// Fréquence = f0 × ratio.
    pub ratio: f64,
    /// Poids.
    pub weight: f64,
    /// Constante de temps = tau × tau_factor.
    pub tau_factor: f64,
}

/// Gains d'un site d'auscultation.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    /// Nom (`base`, `apex`).
    pub name: String,
    /// Gain de B1.
    pub s1_gain: f64,
    /// Gain de B2.
    pub s2_gain: f64,
    /// Gain du souffle.
    pub murmur_gain: f64,
    /// Niveau de preuve.
    pub evidence: String,
}

/// Configuration du module (contenu de `modules/auscultation.json`).
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub struct AuscultationConfig {
    pub version: String,
    // defaults
    pub fs: f64,
    pub duration_s: f64,
    pub resp_hz: f64,
    pub first_beat_s: f64,
    pub end_margin_s: f64,
    pub rest_hr: f64,
    // events
    pub guard_systole_frac: f64,
    // sound
    pub components: Vec<SoundComponent>,
    pub attack_tau_s: f64,
    // s1
    pub s1_f0: f64,
    pub s1_tau: f64,
    pub s1_duration: f64,
    pub s1_base: f64,
    pub s1_dpdt_ref: f64,
    pub s1_cap: f64,
    // s2
    pub s2_f0_aortic: f64,
    pub s2_f0_pulmonic: f64,
    pub s2_tau: f64,
    pub s2_duration: f64,
    pub s2_base: f64,
    pub s2_dp_ref: f64,
    pub s2_cap: f64,
    pub s2_aortic_insp_drop: f64,
    pub s2_pulmonic_base: f64,
    pub s2_pulmonic_insp: f64,
    pub s2_split_base: f64,
    pub s2_split_insp: f64,
    pub s2_systole_rr_exp: f64,
    pub s2_gain_rr_exp: f64,
    // s3
    pub s3_la_threshold: f64,
    pub s3_f0: f64,
    pub s3_tau: f64,
    pub s3_duration: f64,
    pub s3_amp: f64,
    pub s3_q_ref: f64,
    pub s3_delay: f64,
    pub s3_audible: f64,
    // murmur
    pub murmur_band: (f64, f64),
    pub murmur_order: usize,
    pub murmur_amp: f64,
    pub murmur_flow_threshold: f64,
    pub murmur_jitter: (f64, f64),
    // variability
    pub gain_sd: f64,
    pub resp_mod: f64,
    // background
    pub bg_band: (f64, f64),
    pub bg_order: usize,
    pub bg_amp: f64,
    pub swell_base: f64,
    pub swell_amp: f64,
    pub swell_phase: f64,
    // grade
    pub grade_thresholds: Vec<f64>,
    pub grade_min_flow: f64,
    pub grade_max: u8,
    // sites
    pub sites: Vec<Site>,
}

struct Reader<'a> {
    root: &'a Json,
    issues: Vec<Issue>,
}

impl<'a> Reader<'a> {
    fn get(&mut self, group: &str, key: &str) -> Option<&'a Json> {
        let v = self.root.get(group).and_then(|g| g.get(key));
        if v.is_none() {
            self.issues.push(Issue::new(
                format!("/{group}/{key}"),
                "champ requis manquant",
            ));
        }
        v
    }
    fn num(&mut self, group: &str, key: &str) -> f64 {
        match self.get(group, key).map(|v| v.as_f64()) {
            Some(Some(x)) if x.is_finite() => x,
            Some(_) => {
                self.issues
                    .push(Issue::new(format!("/{group}/{key}"), "nombre attendu"));
                f64::NAN
            }
            None => f64::NAN,
        }
    }
    fn pos(&mut self, group: &str, key: &str) -> f64 {
        let x = self.num(group, key);
        if x.is_finite() && x <= 0.0 {
            self.issues.push(Issue::new(
                format!("/{group}/{key}"),
                format!("{x} : valeur > 0 attendue"),
            ));
        }
        x
    }
    fn pair(&mut self, group: &str, key: &str) -> (f64, f64) {
        match self.get(group, key).and_then(|v| v.as_array()) {
            Some(a) if a.len() == 2 && a.iter().all(|x| x.as_f64().is_some()) => {
                let (lo, hi) = (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0));
                if lo >= hi {
                    self.issues.push(Issue::new(
                        format!("/{group}/{key}"),
                        "bornes [lo, hi] avec lo < hi attendues",
                    ));
                }
                (lo, hi)
            }
            Some(_) => {
                self.issues.push(Issue::new(
                    format!("/{group}/{key}"),
                    "tableau de 2 nombres attendu",
                ));
                (f64::NAN, f64::NAN)
            }
            None => (f64::NAN, f64::NAN),
        }
    }
    fn order(&mut self, group: &str, key: &str) -> usize {
        match self.get(group, key).and_then(|v| v.as_u64()) {
            Some(n @ 1..=8) => n as usize,
            _ => {
                self.issues.push(Issue::new(
                    format!("/{group}/{key}"),
                    "ordre entier de 1 à 8 attendu",
                ));
                1
            }
        }
    }
}

impl AuscultationConfig {
    /// Charge et valide la configuration (chemins JSON dans les messages d'erreur).
    pub fn from_json(json: &str, origin: &str) -> Result<Self, LoadError> {
        let root: Json = serde_json::from_str(json).map_err(|source| LoadError::Json {
            origin: origin.into(),
            source,
        })?;
        let mut r = Reader {
            root: &root,
            issues: Vec::new(),
        };
        if root.get("schema_version").and_then(Json::as_str) != Some("1.0.0") {
            r.issues
                .push(Issue::new("/schema_version", "« 1.0.0 » attendu"));
        }
        if root.get("id").and_then(Json::as_str) != Some("auscultation") {
            r.issues.push(Issue::new("/id", "« auscultation » attendu"));
        }
        for g in [
            "defaults",
            "presets",
            "events",
            "sound",
            "s1",
            "s2",
            "s3",
            "murmur",
            "variability",
            "background",
            "grade",
        ] {
            for k in ["source", "evidence", "calibre_sur"] {
                if root
                    .get(g)
                    .and_then(|x| x.get(k))
                    .and_then(Json::as_str)
                    .is_none()
                {
                    r.issues.push(Issue::new(
                        format!("/{g}/{k}"),
                        "provenance requise (source, evidence, calibre_sur)",
                    ));
                }
            }
        }
        let components = match root
            .get("sound")
            .and_then(|s| s.get("components"))
            .and_then(Json::as_array)
        {
            Some(a) if !a.is_empty() => a
                .iter()
                .enumerate()
                .filter_map(|(i, c)| {
                    match c
                        .as_array()
                        .map(|v| v.iter().filter_map(Json::as_f64).collect::<Vec<_>>())
                    {
                        Some(v) if v.len() == 3 => Some(SoundComponent {
                            ratio: v[0],
                            weight: v[1],
                            tau_factor: v[2],
                        }),
                        _ => {
                            r.issues.push(Issue::new(
                                format!("/sound/components/{i}"),
                                "[ratio, poids, facteur tau] attendu",
                            ));
                            None
                        }
                    }
                })
                .collect(),
            _ => {
                r.issues
                    .push(Issue::new("/sound/components", "tableau non vide attendu"));
                Vec::new()
            }
        };
        let mut sites = Vec::new();
        match root.get("sites").and_then(Json::as_object) {
            Some(m) if m.contains_key("base") => {
                for (name, s) in m {
                    let f = |k: &str| s.get(k).and_then(Json::as_f64);
                    match (
                        f("s1_gain"),
                        f("s2_gain"),
                        f("murmur_gain"),
                        s.get("evidence").and_then(Json::as_str),
                    ) {
                        (Some(a), Some(b), Some(c), Some(e)) if a > 0.0 && b > 0.0 && c >= 0.0 => {
                            sites.push(Site {
                                name: name.clone(),
                                s1_gain: a,
                                s2_gain: b,
                                murmur_gain: c,
                                evidence: e.to_string(),
                            })
                        }
                        _ => r.issues.push(Issue::new(
                            format!("/sites/{name}"),
                            "s1_gain, s2_gain (> 0), murmur_gain (≥ 0) et evidence requis",
                        )),
                    }
                }
            }
            _ => r.issues.push(Issue::new(
                "/sites",
                "objet avec au moins le site « base » attendu",
            )),
        }
        let thresholds: Vec<f64> = root
            .get("grade")
            .and_then(|g| g.get("thresholds_ml_s"))
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_f64).collect())
            .unwrap_or_default();
        if thresholds.is_empty() || thresholds.windows(2).any(|w| w[0] >= w[1]) {
            r.issues.push(Issue::new(
                "/grade/thresholds_ml_s",
                "seuils croissants attendus",
            ));
        }
        let cfg = Self {
            version: root
                .get("version")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
            fs: r.pos("defaults", "fs"),
            duration_s: r.pos("defaults", "duration_s"),
            resp_hz: r.pos("defaults", "resp_hz"),
            first_beat_s: r.num("defaults", "first_beat_s"),
            end_margin_s: r.num("defaults", "end_margin_s"),
            rest_hr: r.pos("presets", "rest_hr"),
            guard_systole_frac: r.num("events", "guard_systole_frac"),
            components,
            attack_tau_s: r.pos("sound", "attack_tau_s"),
            s1_f0: r.pos("s1", "f0_hz"),
            s1_tau: r.pos("s1", "tau_s"),
            s1_duration: r.pos("s1", "duration_s"),
            s1_base: r.num("s1", "base_amp"),
            s1_dpdt_ref: r.pos("s1", "dpdt_ref"),
            s1_cap: r.num("s1", "cap"),
            s2_f0_aortic: r.pos("s2", "f0_aortic_hz"),
            s2_f0_pulmonic: r.pos("s2", "f0_pulmonic_hz"),
            s2_tau: r.pos("s2", "tau_s"),
            s2_duration: r.pos("s2", "duration_s"),
            s2_base: r.num("s2", "base_amp"),
            s2_dp_ref: r.pos("s2", "dp_ref"),
            s2_cap: r.num("s2", "cap"),
            s2_aortic_insp_drop: r.num("s2", "aortic_insp_drop"),
            s2_pulmonic_base: r.num("s2", "pulmonic_base"),
            s2_pulmonic_insp: r.num("s2", "pulmonic_insp"),
            s2_split_base: r.num("s2", "split_base_s"),
            s2_split_insp: r.num("s2", "split_insp_s"),
            s2_systole_rr_exp: r.num("s2", "systole_rr_exp"),
            s2_gain_rr_exp: r.num("s2", "gain_rr_exp"),
            s3_la_threshold: r.num("s3", "la_threshold_mmhg"),
            s3_f0: r.pos("s3", "f0_hz"),
            s3_tau: r.pos("s3", "tau_s"),
            s3_duration: r.pos("s3", "duration_s"),
            s3_amp: r.num("s3", "amp"),
            s3_q_ref: r.pos("s3", "q_ref"),
            s3_delay: r.num("s3", "delay_s"),
            s3_audible: r.num("s3", "audible_threshold"),
            murmur_band: r.pair("murmur", "band_hz"),
            murmur_order: r.order("murmur", "filter_order"),
            murmur_amp: r.num("murmur", "amp"),
            murmur_flow_threshold: r.num("murmur", "flow_threshold"),
            murmur_jitter: r.pair("murmur", "jitter"),
            gain_sd: r.num("variability", "gain_lognormal_sd"),
            resp_mod: r.num("variability", "resp_modulation"),
            bg_band: r.pair("background", "band_hz"),
            bg_order: r.order("background", "filter_order"),
            bg_amp: r.num("background", "amp"),
            swell_base: r.num("background", "swell_base"),
            swell_amp: r.num("background", "swell_amp"),
            swell_phase: r.num("background", "swell_phase"),
            grade_thresholds: thresholds,
            grade_min_flow: r.num("grade", "min_flow_ml_s"),
            grade_max: r.num("grade", "max").clamp(0.0, 6.0) as u8,
            sites,
        };
        let mut issues = r.issues;
        // Les filtres doivent être réalisables à la fréquence par défaut.
        for (path, (lo, hi), order) in [
            ("/murmur/band_hz", cfg.murmur_band, cfg.murmur_order),
            ("/background/band_hz", cfg.bg_band, cfg.bg_order),
        ] {
            if lo.is_finite() && hi.is_finite() && cfg.fs.is_finite() {
                if let Err(e) = butter_bandpass(order, lo, hi, cfg.fs) {
                    issues.push(Issue::new(path, e.to_string()));
                }
            }
        }
        if issues.is_empty() {
            Ok(cfg)
        } else {
            Err(LoadError::Invalid {
                origin: origin.into(),
                issues,
            })
        }
    }

    /// Site par nom.
    pub fn site(&self, name: &str) -> Option<&Site> {
        self.sites.iter().find(|s| s.name == name)
    }
}

/// Options d'une synthèse (les valeurs absentes prennent celles de la configuration).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PcgOptions {
    /// Fréquence d'échantillonnage (Hz).
    pub fs: Option<f64>,
    /// Durée (s).
    pub duration_s: Option<f64>,
    /// Fréquence respiratoire (Hz).
    pub resp_hz: Option<f64>,
    /// Site d'auscultation (`base` par défaut).
    pub site: Option<String>,
}

/// Caractéristiques auscultatoires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuscultationFeatures {
    /// Amplitude de base de B1 (avant variabilité).
    pub s1: f64,
    /// Amplitude de base de B2.
    pub s2: f64,
    /// Amplitude de B3 (0 si absent).
    pub s3_amp: f64,
    /// B3 audible.
    pub s3: bool,
    /// Débit régurgitant maximal (mL/s).
    pub murmur_peak_flow: f64,
    /// Type de souffle.
    pub murmur: Murmur,
    /// Grade de Levine indicatif (0–6), seuils NON calibrés.
    pub grade: u8,
}

/// Résultat d'une synthèse.
#[derive(Debug, Clone, PartialEq)]
pub struct PcgResult {
    /// Fréquence d'échantillonnage (Hz).
    pub fs: f64,
    /// Signal normalisé (crête à 1).
    pub samples: Vec<f64>,
    /// Caractéristiques.
    pub features: AuscultationFeatures,
    /// Instants des battements utilisés (s).
    pub beats: Vec<f64>,
    /// Fermeture mitrale dans le battement (s).
    pub t_mc: f64,
    /// Fermeture aortique dans le battement, après garde-fou (s).
    pub t_ac: f64,
}

/// Bruit cardiaque : somme de sinusoïdes amorties (phases tirées), attaque exponentielle.
fn s_sound(
    n: usize,
    fs: f64,
    f0: f64,
    tau: f64,
    amp: f64,
    cfg: &AuscultationConfig,
    noise: &mut dyn NoiseSource,
) -> Vec<f64> {
    let mut x = vec![0.0; n];
    for c in &cfg.components {
        let ph = noise.uniform(0.0, 2.0 * PI);
        for (i, xi) in x.iter_mut().enumerate() {
            let tt = i as f64 / fs;
            *xi += c.weight
                * math::exp(-tt / (tau * c.tau_factor))
                * math::sin(2.0 * PI * f0 * c.ratio * tt + ph);
        }
    }
    x.iter()
        .enumerate()
        .map(|(i, xi)| {
            let tt = i as f64 / fs;
            let rise = 1.0 - math::exp(-tt / cfg.attack_tau_s);
            amp * xi * rise
        })
        .collect()
}

/// Sinusoïde amortie simple (B3).
fn burst(n: usize, fs: f64, f0: f64, tau: f64, amp: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let tt = i as f64 / fs;
            amp * math::exp(-tt / tau) * math::sin(2.0 * PI * f0 * tt + 0.0)
        })
        .collect()
}

fn put(sig: &mut [f64], t0: f64, fs: f64, x: &[f64]) {
    let i0 = (t0 * fs) as usize;
    for (j, v) in x.iter().enumerate() {
        match sig.get_mut(i0 + j) {
            Some(s) => *s += v,
            None => break,
        }
    }
}

/// Synthèse du PCG (`make_pcg` de la référence). `beat_times` : instants des battements du
/// module autonome ; à défaut, battements réguliers de période `T` à partir de
/// `first_beat_s`. Ordre des tirages de `noise` : bruit du souffle (N normales), puis par
/// battement 2 lognormales, 3 + 3 + 3 phases uniformes, [1 uniforme si souffle], enfin bruit
/// de fond (N normales).
pub fn synthesize_pcg(
    h: &PcgInputs,
    beat_times: Option<&[f64]>,
    cfg: &AuscultationConfig,
    opts: &PcgOptions,
    noise: &mut dyn NoiseSource,
) -> Result<PcgResult, Error> {
    let fs = opts.fs.unwrap_or(cfg.fs);
    let dur = opts.duration_s.unwrap_or(cfg.duration_s);
    let resp_hz = opts.resp_hz.unwrap_or(cfg.resp_hz);
    let site_name = opts.site.as_deref().unwrap_or("base");
    let site = cfg.site(site_name).ok_or_else(|| {
        Error::InvalidInput(format!("site d'auscultation inconnu « {site_name} »"))
    })?;
    if !(fs > 0.0 && dur > 0.0 && dur <= 600.0 && resp_hz.is_finite() && resp_hz >= 0.0) {
        return Err(Error::InvalidInput(format!(
            "options PCG invalides (fs {fs}, durée {dur}, f_resp {resp_hz})"
        )));
    }
    let filt = |(lo, hi): (f64, f64), order| {
        butter_bandpass(order, lo, hi, fs).map_err(|e| Error::InvalidInput(e.to_string()))
    };
    let sos_m = filt(cfg.murmur_band, cfg.murmur_order)?;
    let sos_b = filt(cfg.bg_band, cfg.bg_order)?;

    let dt = h.t[1] - h.t[0];
    let ev = beat_events(h, dt);
    let period = h.period;
    let beats: Vec<f64> = match beat_times {
        Some(b) => b.to_vec(),
        None => arange(cfg.first_beat_s, dur, period),
    }
    .into_iter()
    .filter(|b| *b < dur - cfg.end_margin_s)
    .collect();
    let n = (dur * fs) as usize;
    let mut sig = vec![0.0; n];
    let q_reg: Vec<f64> = h
        .q_mi
        .iter()
        .zip(&h.q_ao)
        .map(|(m, a)| (-m).max(0.0) + (-a).max(0.0))
        .collect();
    let peak_reg = q_reg.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let tc: Vec<f64> = (0..(period * fs) as usize).map(|i| i as f64 / fs).collect();
    let q_reg_u = interp(&tc, &h.t, &q_reg);
    let white: Vec<f64> = (0..n).map(|_| noise.standard_normal()).collect();
    let mut mnoise = sosfilt(&sos_m, &white);
    let sd = std(&mnoise);
    mnoise.iter_mut().for_each(|x| *x /= sd);
    let t_mc = ev.i_mc as f64 * dt;
    // garde-fou documenté : B2 pas avant guard × durée de contraction après B1
    let t_ac = f64::max(ev.i_ac as f64 * dt, t_mc + cfg.guard_systole_frac * h.ts);
    let s1_amp = site.s1_gain * cfg.s1_base * f64::min(cfg.s1_cap, ev.dpdt_max / cfg.s1_dpdt_ref);
    let s2_amp = site.s2_gain * cfg.s2_base * f64::min(cfg.s2_cap, ev.dp_close / cfg.s2_dp_ref);
    let s3_amp = if h.la_mean > cfg.s3_la_threshold {
        cfg.s3_amp * f64::max(0.0, (ev.q_early - cfg.s3_q_ref) / cfg.s3_q_ref)
    } else {
        0.0
    };
    let m_amp = if peak_reg > cfg.murmur_flow_threshold {
        site.murmur_gain * cfg.murmur_amp * peak_reg.sqrt()
    } else {
        0.0
    };
    let n1 = (cfg.s1_duration * fs) as usize;
    let n2 = (cfg.s2_duration * fs) as usize;
    let n3 = (cfg.s3_duration * fs) as usize;
    for (i, &b) in beats.iter().enumerate() {
        let rr = if i + 1 < beats.len() {
            beats[i + 1] - b
        } else {
            period
        };
        let resp = 1.0 + cfg.resp_mod * math::sin(2.0 * PI * resp_hz * b);
        let g1 = resp * noise.lognormal(0.0, cfg.gain_sd);
        let g2 =
            resp * noise.lognormal(0.0, cfg.gain_sd) * math::pow(rr / period, cfg.s2_gain_rr_exp);
        // la systole raccourcit peu quand le RR diminue
        let ts = (t_ac - t_mc) * math::pow(rr / period, cfg.s2_systole_rr_exp);
        let b1 = s_sound(n1, fs, cfg.s1_f0, cfg.s1_tau, s1_amp * g1, cfg, noise);
        put(&mut sig, b + t_mc, fs, &b1);
        // inspiration : dédoublement physiologique de B2
        let insp = f64::max(0.0, math::sin(2.0 * PI * resp_hz * b));
        let split = cfg.s2_split_base + cfg.s2_split_insp * insp;
        let a2 = s_sound(
            n2,
            fs,
            cfg.s2_f0_aortic,
            cfg.s2_tau,
            s2_amp * g2 * (1.0 - cfg.s2_aortic_insp_drop * insp),
            cfg,
            noise,
        );
        put(&mut sig, b + t_mc + ts, fs, &a2);
        let p2 = s_sound(
            n2,
            fs,
            cfg.s2_f0_pulmonic,
            cfg.s2_tau,
            s2_amp * g2 * (cfg.s2_pulmonic_base + cfg.s2_pulmonic_insp * insp),
            cfg,
            noise,
        );
        put(&mut sig, b + t_mc + ts + split, fs, &p2);
        if s3_amp > 0.0 {
            let b3 = burst(n3, fs, cfg.s3_f0, cfg.s3_tau, s3_amp * g1);
            put(&mut sig, b + t_mc + ts + cfg.s3_delay, fs, &b3);
        }
        if m_amp > 0.0 {
            let grid: Vec<f64> = (0..(rr * fs) as usize).map(|k| k as f64 / fs).collect();
            let env = interp_right(&grid, &tc, &q_reg_u, 0.0);
            let jitter = noise.uniform(cfg.murmur_jitter.0, cfg.murmur_jitter.1);
            let i0 = (b * fs) as usize;
            for (k, e) in env.iter().enumerate() {
                let idx = i0 + k;
                if idx >= n {
                    break;
                }
                let e = (e / f64::max(peak_reg, 1e-9)).sqrt() * m_amp * resp * jitter;
                sig[idx] += e * mnoise[idx];
            }
        }
    }
    // bruit de fond : contact du stéthoscope, modulé par la respiration
    let white: Vec<f64> = (0..n).map(|_| noise.standard_normal()).collect();
    let mut bg = sosfilt(&sos_b, &white);
    let sd = std(&bg);
    bg.iter_mut().for_each(|x| *x /= sd);
    for (k, s) in sig.iter_mut().enumerate() {
        // comme NumPy : 2π · f · arange(N) / fs (division après les produits)
        let sw = math::sin(2.0 * PI * resp_hz * k as f64 / fs + cfg.swell_phase);
        let swell = cfg.swell_base + cfg.swell_amp * (sw * sw);
        *s += cfg.bg_amp * bg[k] * swell;
    }
    let grade = if peak_reg > cfg.grade_min_flow {
        ((cfg
            .grade_thresholds
            .iter()
            .filter(|t| **t < peak_reg)
            .count()
            + 1) as u8)
            .min(cfg.grade_max)
    } else {
        0
    };
    let max_mi = h.q_mi.iter().map(|m| (-m).max(0.0)).fold(0.0, f64::max);
    let max_ao = h.q_ao.iter().map(|a| (-a).max(0.0)).fold(0.0, f64::max);
    let murmur = if max_mi > cfg.murmur_flow_threshold {
        Murmur::HolosystolicLeftApical
    } else if max_ao > cfg.murmur_flow_threshold {
        Murmur::DiastolicLeftBasilar
    } else {
        Murmur::None
    };
    let norm = sig.iter().map(|x| x.abs()).fold(0.0, f64::max).max(1e-9);
    sig.iter_mut().for_each(|x| *x /= norm);
    Ok(PcgResult {
        fs,
        samples: sig,
        features: AuscultationFeatures {
            s1: s1_amp,
            s2: s2_amp,
            s3_amp,
            s3: s3_amp > cfg.s3_audible,
            murmur_peak_flow: peak_reg,
            murmur,
            grade,
        },
        beats,
        t_mc,
        t_ac,
    })
}

/// Auscultation d'un patient évalué : simulation autonome (instants des battements avec
/// l'arythmie sinusale respiratoire, flux `streams::AUTONOMIC`, comme l'ECG), fréquence
/// respiratoire = FR du modèle / 60, synthèse avec le flux `streams::AUSCULTATION`.
/// Même graine et même durée : mêmes battements que l'ECG de `signals::patient_signals`.
pub fn patient_pcg(
    ev: &crate::engine::Evaluation,
    cfg: &AuscultationConfig,
    opts: &PcgOptions,
    seed: u64,
) -> Result<PcgResult, Error> {
    use crate::autonomic::{simulate_autonomic, AutonomicOptions};
    use crate::rng::SimRng;
    use crate::signals::streams;
    let dur = opts.duration_s.unwrap_or(cfg.duration_s);
    let au = simulate_autonomic(
        &ev.into(),
        &AutonomicOptions {
            duration_s: dur.max(120.0),
            ..AutonomicOptions::default()
        },
        &mut SimRng::for_stream(seed, streams::AUTONOMIC),
    );
    // battements = activations ventriculaires de la conduction (mêmes que l'ECG)
    let cond =
        crate::signals::patient_conduction(ev, &au.beats, seed.wrapping_add(streams::RHYTHM));
    let beats: Vec<f64> = cond
        .ventricular
        .iter()
        .map(|b| b.t)
        .filter(|b| *b < dur)
        .collect();
    let o = PcgOptions {
        resp_hz: opts.resp_hz.or(Some(ev.metabolic.art.rr / 60.0)),
        ..opts.clone()
    };
    synthesize_pcg(
        &PcgInputs::from(&ev.hemo),
        Some(&beats),
        cfg,
        &o,
        &mut SimRng::for_stream(seed, streams::AUSCULTATION),
    )
}
