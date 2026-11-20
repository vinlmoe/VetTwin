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
    /// Capacitance veineuse systémique relative (1 = basale) : multiplie `c_sv` dans la pression
    /// veineuse, pas dans le volume initial (une veinodilatation redistribue le volume).
    ven_cap,
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
    /// Orifice mitral dynamique : `g_eff = g_mr·(VTD/mr_vref)^mr_k`, fixé par battement (point fixe,
    /// Yoran 1979) ; 0 = orifice fixe.
    mr_k,
    /// Volume VG de référence de l'orifice dynamique (mL).
    mr_vref,
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
    /// Volume de distribution de la créatinine (L/kg ; cinétique en évolution temporelle).
    creat_vd,
    /// Volume de distribution de l'urée (L/kg ; cinétique en évolution temporelle).
    urea_vd,
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
    // --- décompensation : couplages mécanistiques (placeholders evidence D) ---
    /// pH artériel sous lequel l'acidémie déprime myocarde et tonus vasculaire.
    acid_ph0,
    /// Perte relative d'élastance (VG, VD) par unité de pH sous `acid_ph0`.
    acid_ees,
    /// Perte relative de résistance systémique par unité de pH sous `acid_ph0`.
    acid_rsys,
    /// PvO2 (mmHg) sous laquelle l'hypoxie tissulaire déprime le myocarde.
    hypox_pvo2,
    /// Perte relative d'élastance par mmHg de PvO2 sous `hypox_pvo2`.
    hypox_ees,
    /// PaO2 (mmHg) sous laquelle l'hypoxémie active la réponse sympathique (commande h).
    hypox_pao2,
    /// PaO2 (mmHg) où la commande hypoxique sature (h = 1).
    hypox_pao2_sat,
    /// Exposant de la commande `h = clip((hypox_pao2 − PaO2)/(hypox_pao2 − hypox_pao2_sat), 0, 1)^n`.
    hypox_n,
    /// Hausse du tonus sympathique de base par unité de `h` (FC).
    hypox_symp,
    /// Hausse relative d'élastance (VG, VD) par unité de `h` (inotropisme).
    hypox_inotropy,
    /// Baisse relative de résistance systémique par unité de `h` (vasodilatation hypoxique).
    hypox_vaso,
    /// K+ (mmol/L) sous lequel les muscles respiratoires faiblissent.
    k_weak,
    /// Perte relative de commande ventilatoire par mmol/L sous `k_weak`.
    k_weak_vent,
    /// Pression de perfusion coronaire PAD − PTDVG (mmHg) sous laquelle le myocarde s'ischémie.
    cpp_min,
    /// Perte relative d'élastance par mmHg sous `cpp_min`.
    cpp_ees,
    /// FC (/min) sous laquelle la pompe cardiaque est considérée arrêtée.
    hr_arrest,
    /// Débit / débit de référence sous lequel la circulation s'effondre.
    co_arrest_frac,
    /// PAM (mmHg) sous laquelle la perfusion ne permet plus la survie (choc irréversible).
    map_arrest,
    // --- oreillettes contractiles (élastance active sur pv et sv) ---
    /// Gain d'activation de l'OG : `P_pv = (1 + k_la·e_a)·P_passive`.
    k_la,
    /// Gain d'activation de l'OD : `P_sv = (1 + k_ra·e_a)·P_passive`.
    k_ra,
    /// Durée de contraction atriale (s).
    ta_atr,
    /// Durée de relaxation atriale (s).
    tra_atr,
    /// Restitution de la systole au RR précédent (exposant, battement par battement).
    ts_restitution,
    /// Transfert du Ca²⁺ captation → libération du réticulum (constante, s).
    ca_tau,
    /// Fraction du Ca²⁺ libéré recaptée par le réticulum.
    ca_recirc,
    /// Amplitude d'élastance d'un battement à QRS large (dyssynchronie).
    ees_wide,
    /// Coefficient de la durée de systole `ts = ts_coef·√T`.
    ts_coef,
    /// Forme de la montée de l'activation ventriculaire `½(1 − cos(π(t/ts)^γ))` (γ = 1 :
    /// demi-cosinus ; γ < 1 : contraction isovolumique plus brève).
    act_gamma,
    /// Durée de relaxation en fraction de la systole (`tr = tr_frac·ts`).
    tr_frac,
    /// Délai électromécanique ventriculaire : début du QRS → début de l'activation (s).
    emd_v,
    /// Délai électromécanique atrial : début de l'onde P → début de la contraction atriale (s).
    emd_a,
    /// Conduction intra-atriale : début de l'onde P → arrivée au nœud AV (s) ; AH = PR − HV − PA.
    pa_ia,
    // --- conduction (rhythm) ---
    /// Probabilité de bloc de sortie sino-atrial (maladie du sinus).
    sa_block,
    /// Fibrillation atriale (0 ou 1).
    af,
    /// Cycle atrial moyen en FA (s).
    af_cycle,
    /// Coefficient de variation du cycle atrial en FA.
    af_cv,
    /// Temps His-ventricule (s).
    hv,
    /// Période réfractaire du nœud AV (s).
    av_erp,
    /// Allongement maximal du délai nodal à récupération incomplète (s).
    av_beta,
    /// Constante de récupération nodale (s).
    av_tau,
    /// Fraction de recul de la récupération nodale par un influx bloqué (conduction cachée).
    av_conceal,
    /// Allongement nodal par unité de tonus vagal au-delà de 0,6.
    av_vagal,
    /// Raccourcissement nodal (ERP et AH) par le tonus sympathique effectif :
    /// × (1 − av_symp·(s_eff − 0,2)) ; bloqué par les bêtabloquants.
    av_symp,
    /// Allongement nodal par mmol/L de K au-delà de 5,5.
    av_k_gain,
    /// Probabilité de conduction His-Purkinje.
    hp_conduction,
    /// K (mmol/L) au-delà duquel la conduction infranodale échoue.
    k_hp_block,
    /// Perte de conduction infranodale par mmol/L au-delà de `k_hp_block`.
    hp_k_slope,
    /// Automatisme jonctionnel (/min).
    junct_rate,
    /// Échappement ventriculaire (/min).
    vent_escape_rate,
    /// Dépression de l'automatisme d'échappement par mmol/L de K au-delà de 5,5.
    esc_k_gain,
    /// Suppression des foyers d'échappement par entraînement : allongement du premier
    /// intervalle d'échappement = 1 + esc_sup_gain·max(0, cycle_échappement/RR_entraîné − 1)·
    /// (1 − exp(−durée d'entraînement/esc_sup_tau)).
    esc_sup_gain,
    /// Constante de temps d'installation de la suppression par entraînement (s).
    esc_sup_tau,
    /// Accélération des foyers d'échappement (jonctionnel et ventriculaire) par le tonus
    /// sympathique effectif : × (1 + esc_a_s·s_eff)/(1 + esc_a_s·0,2).
    esc_a_s,
    /// Ralentissement du foyer jonctionnel par le tonus vagal effectif :
    /// × (1 − esc_a_v·v_eff)/(1 − esc_a_v·0,6) ; sans effet sur le foyer ventriculaire.
    esc_a_v,
    /// Décharges ectopiques ventriculaires (/min).
    vpc_rate,
    /// Décharges ectopiques supplémentaires par mmol/L de K sous `k_vpc` (/min).
    vpc_k_gain,
    /// K (mmol/L) sous lequel l'automatisme ectopique augmente.
    k_vpc,
    /// K (mmol/L) d'arrêt atrial.
    k_atrial_standstill,
    // --- périphérie : thermorégulation, pupille, digestif, SNC (`periph.rs`) ---
    /// Température centrale courante (°C) : entrée de l'évaluation (FC, VO2, P50, CAM) ;
    /// intégrée dans le temps par `periph::thermal_course`.
    temp_c,
    /// Point de consigne thermique hypothalamique (°C).
    t_set,
    /// Température ambiante (°C).
    t_amb,
    /// Réchauffement actif (W/kg, couverture, air pulsé).
    warming,
    /// Anesthésique halogéné en multiples de CAM (n'agit que sur la périphérie : les effets
    /// hémodynamiques et respiratoires des halogénés ne sont pas modélisés).
    anesth_mac,
    /// Blocage muscarinique périphérique (glandes, muscle lisse digestif), 0–1.
    musc_block,
    /// Blocage muscarinique du sphincter de l'iris, 0–1.
    pupil_block,
    /// Commande constrictrice ajoutée du noyau d'Edinger-Westphal (opioïdes).
    pupil_ew,
    /// Éclairement (1 : lumière ambiante ≈ 220 lux ; 0 : obscurité).
    light,
    /// Inhibition non cholinergique de la motilité digestive (µ-opioïdes), 0–1.
    gi_inhib,
    /// Commande sédative (sans dimension ; 0 : vigile).
    sedation,
    /// Commande d'excitation centrale (entrée de scénario ; aucun médicament du catalogue ne
    /// l'actionne : délire anticholinergique et dysphorie non modélisés, faute de données
    /// canines chiffrées, voir `modules/drugs/*.json`).
    excitation,
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
        assert_eq!(Params::NAMES.len(), 148); // + 6 réponse à l'hypoxémie (20/11/2026) ; 73 + 11 décompensation + 6 oreillettes/systole + 3 force-intervalle + 22 conduction + 7 corrections du 18/11/2026 (creat_vd, urea_vd, map_arrest, av_symp, ven_cap, mr_k, mr_vref) + 3 échappements (esc_overdrive → esc_sup_gain, esc_sup_tau, esc_a_s, esc_a_v) + 5 chronologie (act_gamma, tr_frac, emd_v, emd_a, pa_ia) + 12 périphérie (temp_c … excitation)
    }
}
