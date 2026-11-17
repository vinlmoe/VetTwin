//! Blocs du modèle et rattachement de chaque paramètre aux blocs où il intervient.
//!
//! Sert à expliquer le modèle (schéma de la démo) : un module maladie ou médicament agit sur
//! des paramètres ; ce tableau indique dans quelles équations (blocs) chacun est utilisé.
//! Établi d'après le code : `hemo.rs`, `engine.rs`, `gas.rs`, `pkpd.rs`, `autonomic.rs`,
//! `signals.rs`. Un paramètre peut intervenir dans plusieurs blocs (ex. K⁺ : Stewart, FC, ECG).

/// Bloc du modèle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Block {
    /// Patient (poids, allométrie).
    Patient,
    /// Système nerveux autonome et baroréflexe (FC, tonus, RSA).
    Autonomic,
    /// Ventricule gauche (élastance, relation télédiastolique).
    LeftHeart,
    /// Ventricule droit.
    RightHeart,
    /// Valves (diodes avec fuite).
    Valves,
    /// Circulation systémique (aorte, veines, résistance).
    SystemicCirculation,
    /// Circulation pulmonaire et oreillette gauche.
    PulmonaryCirculation,
    /// Volémie (volume sanguin, déficit).
    Volume,
    /// Ventilation et échanges gazeux (PaCO₂, PaO₂, gradient A-a).
    Ventilation,
    /// Transport de l'O₂ par l'hémoglobine (Hill, Bohr).
    OxygenTransport,
    /// Acide-base de Stewart (SID, albumine, phosphate).
    AcidBase,
    /// Métabolisme (VO₂, lactate, glucose).
    Metabolism,
    /// Rein (perfusion, DFG, créatinine, urée).
    Kidney,
    /// Foie (clairance hépatique des médicaments, ALAT).
    Liver,
    /// ECG (intervalles, amplitudes).
    Ecg,
}

impl Block {
    /// Tous les blocs, dans l'ordre d'affichage.
    pub const ALL: [Block; 15] = [
        Block::Patient,
        Block::Autonomic,
        Block::LeftHeart,
        Block::RightHeart,
        Block::Valves,
        Block::SystemicCirculation,
        Block::PulmonaryCirculation,
        Block::Volume,
        Block::Ventilation,
        Block::OxygenTransport,
        Block::AcidBase,
        Block::Metabolism,
        Block::Kidney,
        Block::Liver,
        Block::Ecg,
    ];

    /// Identifiant stable (API JSON).
    pub fn id(self) -> &'static str {
        match self {
            Block::Patient => "patient",
            Block::Autonomic => "autonomic",
            Block::LeftHeart => "left_heart",
            Block::RightHeart => "right_heart",
            Block::Valves => "valves",
            Block::SystemicCirculation => "systemic_circulation",
            Block::PulmonaryCirculation => "pulmonary_circulation",
            Block::Volume => "volume",
            Block::Ventilation => "ventilation",
            Block::OxygenTransport => "oxygen_transport",
            Block::AcidBase => "acid_base",
            Block::Metabolism => "metabolism",
            Block::Kidney => "kidney",
            Block::Liver => "liver",
            Block::Ecg => "ecg",
        }
    }

    /// Libellé français.
    pub fn label_fr(self) -> &'static str {
        match self {
            Block::Patient => "Patient (poids, allométrie)",
            Block::Autonomic => "Système autonome et baroréflexe",
            Block::LeftHeart => "Ventricule gauche",
            Block::RightHeart => "Ventricule droit",
            Block::Valves => "Valves",
            Block::SystemicCirculation => "Circulation systémique",
            Block::PulmonaryCirculation => "Circulation pulmonaire et OG",
            Block::Volume => "Volémie",
            Block::Ventilation => "Ventilation et échanges gazeux",
            Block::OxygenTransport => "Transport de l'O₂ (Hb)",
            Block::AcidBase => "Acide-base (Stewart)",
            Block::Metabolism => "Métabolisme et lactate",
            Block::Kidney => "Rein (DFG, clairance)",
            Block::Liver => "Foie (clairance)",
            Block::Ecg => "ECG",
        }
    }
}

/// Blocs où intervient le paramètre `name` (vide si inconnu).
pub fn blocks_of(name: &str) -> &'static [Block] {
    use Block::*;
    match name {
        "bw" => &[Patient],
        "hr" | "map_set" | "baro_gain" | "s0" | "v0" | "a_s" | "a_v" | "vagal_add"
        | "vagal_block" | "beta_block" | "rsa_depth" | "vag_noise" | "sym_noise" | "baro_sens"
        | "sympathetic" => &[Autonomic],
        "ees_lv" | "v0_lv" | "a_lv" | "b_lv" => &[LeftHeart],
        "ees_rv" | "v0_rv" | "a_rv" | "b_rv" => &[RightHeart],
        "r_valve" | "g_mr" | "g_ar" | "g_tr" | "g_pr" => &[Valves],
        "c_ao" | "c_sv" | "v0_ao" | "v0_sv" | "r_sys" => &[SystemicCirculation],
        "c_pa" | "c_pv" | "v0_pa" | "v0_pv" | "r_pul" => &[PulmonaryCirculation],
        "bvf" | "vol_shift" => &[Volume],
        "vco2" | "va0" | "vent_drive" | "aagrad" | "fio2" | "pb" | "rq" => &[Ventilation],
        // œdème : la POG élève le gradient alvéolo-artériel
        "edema_k" => &[Ventilation, PulmonaryCirculation],
        "hb" => &[OxygenTransport, Metabolism],
        "p50" | "hill_n" => &[OxygenTransport],
        "na" | "cl" | "xa" | "alb" | "phos" => &[AcidBase],
        // K⁺ : SID de Stewart, dépression de la FC au-delà de 6 mmol/L, ECG
        "k" => &[AcidBase, Autonomic, Ecg],
        "vo2" => &[Metabolism],
        "lact0" | "glucose" => &[Metabolism],
        // masse néphronique : DFG, créatinine, urée, et clairance rénale des médicaments
        "nephron_mass" | "gfr0" | "creat0" | "urea0" => &[Kidney],
        "hepatic_fn" | "alt" => &[Liver],
        // PR : intervalle de l'ECG et délai mécanique oreillette → ventricule
        "pr" => &[Ecg, PulmonaryCirculation, SystemicCirculation],
        "qrs" | "qt" | "r_amp" | "t_amp" | "p_amp" => &[Ecg],
        // décompensation (engine::evaluate) : retours de l'aval sur la pompe et les vaisseaux
        "acid_ph0" | "acid_ees" => &[AcidBase, LeftHeart, RightHeart],
        "acid_rsys" => &[AcidBase, SystemicCirculation],
        "hypox_pvo2" | "hypox_ees" => &[Metabolism, LeftHeart, RightHeart],
        "cpp_min" | "cpp_ees" => &[SystemicCirculation, LeftHeart, RightHeart],
        "k_weak" | "k_weak_vent" => &[Ventilation],
        "hr_arrest" => &[Autonomic],
        "co_arrest_frac" => &[SystemicCirculation],
        // oreillettes contractiles : OG dans pv, OD dans sv ; activation PR avant le ventricule
        "k_la" => &[PulmonaryCirculation, LeftHeart],
        "k_ra" => &[SystemicCirculation, RightHeart],
        "ta_atr" | "tra_atr" => &[PulmonaryCirculation, SystemicCirculation],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Params;

    #[test]
    fn every_parameter_belongs_to_a_block() {
        for n in Params::NAMES {
            assert!(!blocks_of(n).is_empty(), "paramètre {n} sans bloc");
        }
        // Chaque bloc porte au moins un paramètre.
        for b in Block::ALL {
            assert!(
                Params::NAMES.iter().any(|n| blocks_of(n).contains(&b)),
                "{b:?} vide"
            );
        }
        assert!(blocks_of("inconnu").is_empty());
    }
}
