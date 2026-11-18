# vetsim — patient virtuel mécaniste (prototype)

Patient canin sain + modules maladie probabilistes + PK/PD + système nerveux autonome. Sorties : hémodynamique,
gaz du sang (artériel et veineux), biochimie, ECG (dérivation II, intervalles RR issus du modèle autonome),
phonocardiogramme (WAV), variabilité de la FC (RSA, SDNN, RMSSD, LF/HF).

    pip install numpy scipy matplotlib
    python demo.py            # figures et WAV dans ./out (~45 s)

    from patient import Patient, summary
    pt = Patient(bw=20, seed=1)
    pt.add_disease('MMVD', stage='C')          # ou severity=0.75
    r = pt.state()                             # r['hemo'], r['art'], r['ven'], r['chem'], r['ecg'], r['pcg']
    r['autonomic_ts']['hrv']                   # RSA, SDNN, RMSSD, LF/HF ; r['autonomic_ts']['beats'] = instants des ondes R
    pt.state(conc={'atropine': 0.05})          # blocage vagal ; aussi 'betabloquant', 'opioide'
    tc, out = pt.timecourse([(0,'furosemide',2.0,'iv')], 12, [0,1,2,4])

Ajouter une maladie : classe dans diseases.py (sample() renvoie des modificateurs de paramètres).
Ajouter un médicament : entrée dans pkpd.DRUGS et règle dans pkpd.apply_drug_effects.

AVERTISSEMENT : valeurs de paramètres plausibles mais NON validées ; PK/PD = placeholders ;
outil pédagogique, jamais pour une décision clinique.
