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
    )
    return p


def copy_params(p):
    return {k: (v.copy() if isinstance(v, np.ndarray) else v) for k, v in p.items()}
