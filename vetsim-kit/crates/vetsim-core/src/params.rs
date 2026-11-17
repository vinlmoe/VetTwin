//! Paramètres physiologiques du patient (miroir de `reference/python/params.py`).
//!
//! La liste des noms est **fermée** : un module qui cite un paramètre absent est refusé au
//! chargement. Les valeurs ne sont pas codées ici : elles viennent de `modules/healthy_dog.json`.
//! Unités : mmHg, mL, s, mL/s/mmHg ; voir chaque champ.

macro_rules! define_params {
    ($( $(#[$doc:meta])* $name:ident ),* $(,)?) => {
        /// Jeu complet de paramètres d'un patient (sain ou modifié par maladies et médicaments).
        #[derive(Debug, Clone, PartialEq)]
        pub struct Params {
            $( $(#[$doc])* pub $name: f64, )*
        }

        impl Params {
            /// Noms des paramètres, dans l'ordre de `params.py`.
            pub const NAMES: &'static [&'static str] = &[$(stringify!($name)),*];

            /// Tous les paramètres à zéro (état intermédiaire avant évaluation du module sain).
            pub(crate) fn zeroed() -> Self {
                Self { $( $name: 0.0, )* }
            }

            /// Lit un paramètre par son nom.
            pub fn get(&self, name: &str) -> Option<f64> {
                match name {
                    $( stringify!($name) => Some(self.$name), )*
                    _ => None,
                }
            }

            /// Accès mutable à un paramètre par son nom.
            pub fn get_mut(&mut self, name: &str) -> Option<&mut f64> {
                match name {
                    $( stringify!($name) => Some(&mut self.$name), )*
                    _ => None,
                }
            }

            /// Paires (nom, valeur) dans l'ordre de [`Params::NAMES`].
            pub fn to_vec(&self) -> Vec<(&'static str, f64)> {
                vec![$( (stringify!($name), self.$name) ),*]
            }
        }
    };
}

define_params! {
    /// Poids corporel (kg).
    bw,
    // --- cardiovasculaire ---
    /// Fréquence cardiaque intrinsèque (/min).
    hr,
    /// Élastance télésystolique du VG (mmHg/mL).
    ees_lv,
    /// Volume mort du VG (mL).
    v0_lv,
    /// Coefficient de la relation pression-volume télédiastolique du VG (mmHg).
    a_lv,
    /// Raideur diastolique du VG (1/mL).
    b_lv,
    /// Élastance télésystolique du VD (mmHg/mL).
    ees_rv,
    /// Volume mort du VD (mL).
    v0_rv,
    /// Coefficient télédiastolique du VD (mmHg).
    a_rv,
    /// Raideur diastolique du VD (1/mL).
    b_rv,
    /// Compliance aortique (mL/mmHg).
    c_ao,
    /// Compliance veineuse systémique (mL/mmHg).
    c_sv,
    /// Compliance artérielle pulmonaire (mL/mmHg).
    c_pa,
    /// Compliance veineuse pulmonaire / OG (mL/mmHg).
    c_pv,
    /// Volume non contraint de l'aorte (mL).
    v0_ao,
    /// Volume non contraint des veines systémiques (mL).
    v0_sv,
    /// Volume non contraint de l'artère pulmonaire (mL).
    v0_pa,
    /// Volume non contraint des veines pulmonaires (mL).
    v0_pv,
    /// Résistance systémique (mmHg·s/mL).
    r_sys,
    /// Résistance pulmonaire (mmHg·s/mL).
    r_pul,
    /// Résistance valvulaire à l'ouverture (mmHg·s/mL).
    r_valve,
    /// Conductance de fuite mitrale (mL/s/mmHg).
    g_mr,
    /// Conductance de fuite aortique (mL/s/mmHg).
    g_ar,
    /// Conductance de fuite tricuspide (mL/s/mmHg).
    g_tr,
    /// Conductance de fuite pulmonaire (mL/s/mmHg).
    g_pr,
    /// Facteur de volémie (sans unité).
    bvf,
    /// Déficit (−) ou excès (+) de volume (mL).
    vol_shift,
    /// Consigne de PAM du baroréflexe (mmHg).
    map_set,
    /// Gain du baroréflexe (sans unité).
    baro_gain,
    // --- respiration / gaz ---
    /// Consommation d'O2 (mL/min/kg).
    vo2,
    /// Production de CO2 (mL/min/kg).
    vco2,
    /// Ventilation alvéolaire de base (L/min/kg).
    va0,
    /// Commande ventilatoire (sans unité).
    vent_drive,
    /// Gradient alvéolo-artériel de base (mmHg).
    aagrad,
    /// Pente du gradient A-a liée à l'œdème (mmHg par mmHg de POG > 18).
    edema_k,
    /// Fraction inspirée d'O2.
    fio2,
    /// Pression barométrique (mmHg).
    pb,
    /// Hémoglobine (g/dL).
    hb,
    /// P50 de l'hémoglobine à pH 7,4 (mmHg).
    p50,
    /// Coefficient de Hill de la courbe de dissociation.
    hill_n,
    /// Quotient respiratoire.
    rq,
    // --- milieu intérieur ---
    /// Sodium (mmol/L).
    na,
    /// Potassium (mmol/L).
    k,
    /// Chlorure (mmol/L).
    cl,
    /// Anions forts non mesurés (mmol/L).
    xa,
    /// Albumine (g/dL dans les paramètres).
    alb,
    /// Phosphate (mmol/L).
    phos,
    /// Lactate de base (mmol/L).
    lact0,
    /// Glucose (mmol/L).
    glucose,
    /// ALAT (U/L).
    alt,
    // --- rein / foie ---
    /// Masse néphronique fonctionnelle (1 = normale).
    nephron_mass,
    /// DFG de référence (mL/min/kg).
    gfr0,
    /// Créatininémie de référence (µmol/L).
    creat0,
    /// Urémie de référence (mmol/L).
    urea0,
    /// Fonction hépatique (1 = normale).
    hepatic_fn,
    // --- ECG de base ---
    /// Intervalle PR (s).
    pr,
    /// Durée du QRS (s).
    qrs,
    /// Intervalle QT (s).
    qt,
    /// Amplitude de R (mV).
    r_amp,
    /// Amplitude de T (mV).
    t_amp,
    /// Amplitude de P (mV).
    p_amp,
    // --- états modulés ---
    /// Activation sympathique additionnelle (sans unité).
    sympathetic,
    // --- système nerveux autonome ---
    /// Tonus sympathique à la PAM de consigne.
    s0,
    /// Tonus vagal à la PAM de consigne.
    v0,
    /// Gain chronotrope sympathique.
    a_s,
    /// Gain chronotrope vagal.
    a_v,
    /// Tonus vagal ajouté (médicaments, maladies).
    vagal_add,
    /// Blocage vagal (0–1).
    vagal_block,
    /// Blocage bêta (0–1).
    beta_block,
    /// Profondeur de modulation respiratoire du tonus vagal.
    rsa_depth,
    /// Bruit de dérive vagale.
    vag_noise,
    /// Bruit sympathique.
    sym_noise,
    /// Sensibilité du baroréflexe.
    baro_sens,
}

#[cfg(test)]
mod tests {
    use super::Params;

    #[test]
    fn names_are_unique_and_accessors_agree() {
        let mut p = Params::zeroed();
        for (i, n) in Params::NAMES.iter().enumerate() {
            assert!(!Params::NAMES[..i].contains(n), "doublon {n}");
            *p.get_mut(n).unwrap() = i as f64;
        }
        for (i, (n, v)) in p.to_vec().into_iter().enumerate() {
            assert_eq!(n, Params::NAMES[i]);
            assert_eq!(v, i as f64);
            assert_eq!(p.get(n), Some(v));
        }
        assert_eq!(p.get("nope"), None);
        assert_eq!(Params::NAMES.len(), 73);
    }
}
