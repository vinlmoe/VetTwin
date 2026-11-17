"""Paramètres du patient sain (chien) et mise à l'échelle allométrique.

ATTENTION : valeurs de départ plausibles, issues de plages physiologiques usuelles,
partiellement calées sur données canines publiées (voir calibrate.py, table TARGETS).
Les blocs non calés (PK atropine/bêtabloquant/opioïde, murmures, maladies) restent des placeholders.
"""
import numpy as np

REF_BW = 20.0  # kg


def healthy_params(bw=20.0):
    s = bw / REF_BW
    fq = s ** 0.75           # débits ~ BW^0.75
    p = dict(
        bw=bw,
        # --- cardiovasculaire (unités : mmHg, mL, s) ---
        hr=87.0 * s ** -0.25,          # Haskins 2005 : 87/min (97 chiens éveillés)
        ees_lv=7.0 * 0.649 / s, v0_lv=4.0 * s, a_lv=1.0, b_lv=0.06 * 0.258 / s,   # calage Haskins 2005
        ees_rv=1.6 / s, v0_rv=6.0 * s, a_rv=0.4, b_rv=0.05 / s,
        c_ao=0.7 * s, c_sv=55.0 * s, c_pa=1.5 * s, c_pv=5.0 * s,
        v0_ao=150.0 * s, v0_sv=900.0 * s, v0_pa=60.0 * s, v0_pv=120.0 * s,
        r_sys=2.3 * 0.764 / fq, r_pul=0.15 * 1.04 / fq, r_valve=0.03 / fq,
        g_mr=0.0005, g_ar=0.0002, g_tr=0.0002, g_pr=0.0002,   # conductances de fuite (mL/s/mmHg)
        bvf=1.0, vol_shift=-21.0 * s,          # facteur de volémie, déficit/excès de volume (mL)
        map_set=103.0, baro_gain=1.0,
        # --- respiration / gaz ---
        vo2=5.5 * s ** -0.25, vco2=4.6 * s ** -0.25, va0=0.0748, vent_drive=1.0,   # mL/min/kg, mL/min/kg, L/min/kg
        aagrad=3.0, edema_k=4.0, fio2=0.21, pb=760.0,
        hb=15.0, p50=30.0, hill_n=2.7, rq=0.84,
        # --- milieu intérieur ---
        na=150.0, k=4.1, cl=113.6, xa=-2.48, alb=3.77, phos=1.42,   # Constable 2005 (10 chiens sains)
        lact0=1.2, glucose=5.2, alt=45.0,
        # --- rein / foie ---
        nephron_mass=1.0, gfr0=3.5, creat0=75.0, urea0=5.0, hepatic_fn=1.0,
        # --- ECG de base (s, mV) ---
        pr=0.12, qrs=0.045, qt=0.20, r_amp=1.7, t_amp=0.30, p_amp=0.20,   # ECG Labrador/Berger allemand
        # --- états modulés par maladie / médicaments ---
        sympathetic=0.0,
        # --- système nerveux autonome ---
        s0=0.20, v0=0.60,            # tonus sympathique / vagal à la PAM de consigne (chien au repos : vagal dominant)
        a_s=0.40, a_v=0.75,          # gains chronotropes sympathique / vagal sur le nœud sinusal
        vagal_add=0.0, vagal_block=0.0, beta_block=0.0,   # modifiés par médicaments / maladies
        rsa_depth=0.95, vag_noise=0.025, sym_noise=0.22,              # profondeur de modulation respiratoire du tonus vagal central
        baro_sens=1.0,               # sensibilité du baroréflexe (réduite en insuffisance cardiaque)
        # --- décompensation : couplages mécanistiques (PLACEHOLDERS evidence D, docs/PROVENANCE.md) ---
        acid_ph0=7.20,               # pH artériel sous lequel l'acidémie déprime myocarde et tonus vasculaire
        acid_ees=1.5,                # perte relative d'élastance (VG, VD) par unité de pH sous acid_ph0
        acid_rsys=1.0,               # perte relative de résistance systémique par unité de pH sous acid_ph0
        hypox_pvo2=20.0,             # PvO2 (mmHg) sous laquelle l'hypoxie tissulaire déprime le myocarde
        hypox_ees=0.03,              # perte relative d'élastance par mmHg de PvO2 sous hypox_pvo2
        k_weak=2.5,                  # K+ (mmol/L) sous lequel les muscles respiratoires faiblissent
        k_weak_vent=0.4,             # perte relative de commande ventilatoire par mmol/L sous k_weak
        cpp_min=40.0,                # pression de perfusion coronaire PAD − PTDVG (mmHg) sous laquelle le myocarde s'ischémie
        cpp_ees=0.02,                # perte relative d'élastance par mmHg sous cpp_min
        hr_arrest=20.0,              # FC (/min) sous laquelle la pompe cardiaque est considérée arrêtée
        co_arrest_frac=0.15,         # débit / débit de référence sous lequel la circulation s'effondre
        # --- oreillettes contractiles : élastance active sur pv (OG + veines pulmonaires) et sv (OD + veines)
        #     P = (1 + k·e_a(t))·P_passive, activation PR avant le ventricule ; k = 0 : oreillettes passives
        k_la=1.45, k_ra=0.29,        # gains d'activation atriale : k_la calé sur E/A ≈ 1,39 (E 0,82, A 0,59 m/s, n = 53) ; k_ra = 0,2·k_la (placeholder D)
        ta_atr=0.09, tra_atr=0.06,   # durées de contraction et de relaxation atriales (s ; placeholders D)
        # --- conduction (rhythm.py) ; placeholders D sauf mention (docs/PROVENANCE.md)
        sa_block=0.0,                # probabilité de bloc de sortie sino-atrial (maladie du sinus)
        af=0.0, af_cycle=0.12, af_cv=0.3,   # fibrillation atriale : drapeau, cycle atrial moyen (s), CV
        hv=0.035,                    # temps His-ventricule (s)
        av_erp=0.15, av_beta=0.10, av_tau=0.15, av_conceal=0.75,   # nœud AV (s) ; ERP et conduction cachée calés sur la FA (FC 129,7, CVRR 0,309 ; n = 50) avec 1:1 sinusal jusqu'à 225/min
        av_vagal=0.5, av_k_gain=0.10,      # allongement nodal par le tonus vagal et par mmol/L de K au-delà de 5,5
        hp_conduction=1.0,           # probabilité de conduction His-Purkinje (bloc infranodal)
        k_hp_block=10.0, hp_k_slope=0.5,   # K au-delà duquel la conduction infranodale échoue (QRS large > 10,1) ; pente /mmol/L
        junct_rate=50.0,             # automatisme jonctionnel (/min), au-dessus de l'échappement ventriculaire
        vent_escape_rate=42.0,       # échappement ventriculaire (/min) : 42 ± 11 en BAV de haut degré (n = 97)
        esc_k_gain=0.10,             # dépression de l'automatisme d'échappement par mmol/L de K au-delà de 5,5
        esc_overdrive=1.3,           # allongement du premier intervalle d'échappement après un battement conduit (placeholder D)
        vpc_rate=0.0, vpc_k_gain=1.0, k_vpc=3.0,   # foyer ectopique (/min), gain hypokaliémique (/min par mmol/L sous k_vpc)
        k_atrial_standstill=8.5,     # K d'arrêt atrial, conduction sino-ventriculaire conservée (8,5-10 ; Hariman 1983)
    )
    return p


def copy_params(p):
    return {k: (v.copy() if isinstance(v, np.ndarray) else v) for k, v in p.items()}
