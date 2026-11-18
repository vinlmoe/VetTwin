"""Démonstration : python demo.py  -> figures + WAV dans ./out"""
import os, sys
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from scipy.io import wavfile
from multiprocessing import Pool
from patient import Patient, summary
from autonomic import simulate_autonomic, spectrum

os.makedirs('out', exist_ok=True)
plt.rcParams.update({'font.size': 8, 'axes.spines.top': False, 'axes.spines.right': False})


def make(name, ds, seed=1):
    pt = Patient(seed=seed, variability=False)
    for d, s in ds:
        pt.add_disease(d, severity=s)
    return pt.state()


def fig1():
    H = make('sain', [])
    M = make('mmvd', [('MMVD', 0.8)])
    A = make('addison', [('Addison', 0.9)])
    fig, ax = plt.subplots(3, 3, figsize=(11, 7.5))
    for row, (r, lab) in enumerate([(H, 'Sain'), (M, 'MMVD (sévérité 0,8)'), (A, 'Addison, crise (0,9)')]):
        e = r['ecg']; m = e['t'] < 3.0
        ax[row, 0].plot(e['t'][m], e['v'][m], lw=.8, c='k'); ax[row, 0].set_ylabel(lab + '\nECG D II (mV)')
        ax[row, 0].set_title(f"FC {e['hr']:.0f}/min, K+ {r['p']['k']:.1f}" if row else 'ECG', fontsize=8)
        c = r['pcg']; mm = c['t'] < 2.0
        ax[row, 1].plot(c['t'][mm], c['v'][mm], lw=.5, c='C0')
        ax[row, 1].set_title(f"PCG : souffle {c['murmur']}, grade {c['grade']}, S3 {'oui' if c['s3'] else 'non'}", fontsize=8)
        h = r['hemo']
        ax[row, 2].plot(h['t'], h['P_lv'], label='VG'); ax[row, 2].plot(h['t'], h['P_ao'], label='Ao'); ax[row, 2].plot(h['t'], h['P_pv'], label='OG')
        ax[row, 2].set_title(f"PAM {h['map']:.0f}, DC {h['co']:.1f} L/min, FR {100*h['reg_fraction']:.0f} %, POG {h['la_mean']:.0f}", fontsize=8)
        if row == 0: ax[row, 2].legend(frameon=False, fontsize=7)
    for a in ax[2, :]:
        a.set_xlabel('temps (s)')
    ax[0, 2].set_ylabel('mmHg'); ax[1, 2].set_ylabel('mmHg'); ax[2, 2].set_ylabel('mmHg')
    plt.tight_layout(); plt.savefig('out/fig1_physiologie.png', dpi=130); plt.close()
    wavfile.write('out/auscultation_sain.wav', 4000, (H['pcg']['v'] * 30000).astype(np.int16))
    wavfile.write('out/auscultation_MMVD.wav', 4000, (M['pcg']['v'] * 30000).astype(np.int16))
    # boucles pression-volume
    plt.figure(figsize=(3.6, 3))
    for r, lab in [(H, 'sain'), (M, 'MMVD 0,8')]:
        plt.plot(r['hemo']['V_lv'], r['hemo']['P_lv'], label=lab)
    plt.xlabel('V VG (mL)'); plt.ylabel('P VG (mmHg)'); plt.legend(frameon=False)
    plt.tight_layout(); plt.savefig('out/fig_pv.png', dpi=130); plt.close()
    return H, M, A


def one(args):
    seed, disease = args
    pt = Patient(seed=seed)
    case = pt.add_disease(disease)
    r = pt.state(signals=True)
    return disease, case.severity, case.meta.get('stage'), summary(r), r['pcg']['grade']


def fig2():
    pt = Patient(seed=3, variability=False)
    pt.add_disease('MMVD', severity=0.8)
    doses = [(0.0, 'furosemide', 2.0, 'iv'), (0.0, 'pimobendan', 0.25, 'po')]
    ts = [0, .25, .5, 1, 1.5, 2, 3, 4, 6, 8, 10, 12]
    tc, out = pt.timecourse(doses, 12.0, ts)
    S = [summary(r) for _, _, r in out]
    jobs = [(100 + i, d) for i, d in enumerate(['MMVD'] * 60 + ['CKD'] * 40 + ['Addison'] * 40)]
    with Pool(2) as pool:
        mc = pool.map(one, jobs)
    fig, ax = plt.subplots(2, 3, figsize=(11, 6))
    ax[0, 0].plot(tc['t'], tc['furosemide'], label='furosémide (mg/L)'); ax[0, 0].plot(tc['t'], tc['pimobendan'] * 100, label='pimobendane (x100 mg/L)')
    ax[0, 0].set_title('PK (placeholders)', fontsize=8); ax[0, 0].legend(frameon=False, fontsize=7)
    ax[0, 1].plot(tc['t'], [s['POG moy'] for s in S], 'o-', label='POG moy (mmHg)'); ax[0, 1].plot(tc['t'], [s['PaO2'] / 4 for s in S], 's-', label='PaO2/4')
    ax[0, 1].set_title('MMVD 0,8 : effet furosémide + pimobendane', fontsize=8); ax[0, 1].legend(frameon=False, fontsize=7)
    ax[0, 2].plot(tc['t'], [s['pH art'] for s in S], 'o-', c='C2'); ax2 = ax[0, 2].twinx()
    ax2.plot(tc['t'], [s['K'] for s in S], 's-', c='C3'); ax[0, 2].set_ylabel('pH art', color='C2'); ax2.set_ylabel('K+ (mmol/L)', color='C3')
    ax[0, 2].set_title('alcalose de contraction, hypokaliémie', fontsize=8)
    for a in ax[0]: a.set_xlabel('temps (h)')
    # Monte-Carlo
    cols = {'MMVD': 'C0', 'CKD': 'C1', 'Addison': 'C3'}
    for d in ['MMVD']:
        x = [m[3]['Fraction régurg. %'] for m in mc if m[0] == d]; y = [m[3]['POG moy'] for m in mc if m[0] == d]
        ax[1, 0].scatter(x, y, s=10, c=cols[d]); ax[1, 0].set_xlabel('fraction de régurgitation (%)'); ax[1, 0].set_ylabel('POG moyenne (mmHg)')
        ax[1, 0].set_title('60 MMVD tirés dans le prior', fontsize=8)
    for d in ['CKD', 'Addison']:
        x = [m[3]['K'] for m in mc if m[0] == d]; y = [m[3]['Créat µmol/L'] for m in mc if m[0] == d]
        ax[1, 1].scatter(x, y, s=10, c=cols[d], label=d)
    ax[1, 1].set_xlabel('K+ (mmol/L)'); ax[1, 1].set_ylabel('créatinine (µmol/L)'); ax[1, 1].legend(frameon=False); ax[1, 1].set_title('CKD vs Addison', fontsize=8)
    x = [m[3]['Fraction régurg. %'] for m in mc if m[0] == 'MMVD']; g = [m[4] for m in mc if m[0] == 'MMVD']
    ax[1, 2].scatter(x, g, s=10, c='C0'); ax[1, 2].set_xlabel('fraction de régurgitation (%)'); ax[1, 2].set_ylabel('grade du souffle (indicatif)')
    plt.tight_layout(); plt.savefig('out/fig2_pkpd_montecarlo.png', dpi=130); plt.close()
    return mc


def fig3():
    sc = [('sain', [], None), ('atropine', [], {'atropine': 0.05}), ('bêtabloquant', [], {'betabloquant': 1.0}),
          ('opioïde', [], {'opioide': 0.05}), ('MMVD B2', [('MMVD', 0.5)], None), ('MMVD D', [('MMVD', 0.95)], None),
          ('Addison', [('Addison', 0.9)], None)]
    R = {}
    for name, ds, conc in sc:
        pt = Patient(seed=5, variability=False)
        for dd, s in ds:
            pt.add_disease(dd, severity=s)
        R[name] = pt.state(conc=conc)
    H = R['sain']; au = H['autonomic_ts']
    fig, ax = plt.subplots(2, 3, figsize=(11.5, 6.4))
    b = au['beats']; hr_i = 60 / np.diff(b); m = (b[1:] > 5) & (b[1:] < 45)
    ax[0, 0].plot(b[1:][m], hr_i[m], 'o-', ms=3, lw=.8, c='C3', label='FC instantanée')
    ax[0, 0].set_ylabel('FC (bpm)'); ax[0, 0].set_xlabel('temps (s)')
    a2 = ax[0, 0].twinx(); mt = (au['t'] > 5) & (au['t'] < 45)
    a2.plot(au['t'][mt], au['rho'][mt], c='C0', lw=.8, alpha=.7); a2.set_yticks([]); a2.set_ylabel('volume pulmonaire', color='C0')
    ax[0, 0].set_title('Arythmie sinusale respiratoire (sain)', fontsize=8)
    ax[0, 1].plot(au['t'][mt], au['s'][mt], label='sympathique'); ax[0, 1].plot(au['t'][mt], au['v'][mt], label='vagal')
    ax[0, 1].set_title('tonus efférents (effecteurs)', fontsize=8); ax[0, 1].legend(frameon=False, fontsize=7); ax[0, 1].set_xlabel('temps (s)')
    for name, c in [('sain', 'C0'), ('atropine', 'C1'), ('MMVD D', 'C2'), ('Addison', 'C3')]:
        f, P = spectrum(R[name]['autonomic_ts']['beats']); k = f < 1.0
        ax[0, 2].semilogy(f[k], P[k] + 1e-3, label=name, c=c)
    ax[0, 2].axvspan(0.04, 0.15, color='grey', alpha=.12); ax[0, 2].set_xlabel('fréquence (Hz)'); ax[0, 2].set_ylabel('puissance RR (ms²/Hz)')
    ax[0, 2].legend(frameon=False, fontsize=7); ax[0, 2].set_title('spectre du tachogramme (zone grisée : LF)', fontsize=8)
    names = list(R); rsa = [R[n]['autonomic_ts']['hrv']['rsa_bpm'] for n in names]; fcm = [R[n]['autonomic_ts']['hrv']['fc_moy'] for n in names]
    ax[1, 0].bar(range(len(names)), rsa, color='C0'); ax[1, 0].set_xticks(range(len(names))); ax[1, 0].set_xticklabels(names, rotation=35, ha='right')
    ax[1, 0].set_ylabel('amplitude de RSA (bpm, crête à crête)')
    for i, (x, y) in enumerate(zip(rsa, fcm)):
        ax[1, 0].text(i, x + .3, f'FC {y:.0f}', ha='center', fontsize=6.5)
    ax[1, 0].set_title('RSA et FC moyenne par scénario', fontsize=8)
    rates = [8, 15, 20, 30, 45, 70, 100]; amp = []
    for rr_ in rates:
        r2 = dict(H); r2['art'] = dict(H['art'], rr=rr_)
        amp.append(simulate_autonomic(r2, dur=90, seed=2)['hrv']['rsa_bpm'])
    ax[1, 1].plot(rates, amp, 'o-'); ax[1, 1].set_xlabel('fréquence respiratoire (/min)'); ax[1, 1].set_ylabel('RSA (bpm)')
    ax[1, 1].set_title('la RSA chute avec la fréquence respiratoire', fontsize=8)
    e = H['ecg']; mm = e['t'] < 6
    ax[1, 2].plot(e['t'][mm], e['v'][mm], lw=.7, c='k'); ax[1, 2].set_xlabel('temps (s)'); ax[1, 2].set_ylabel('ECG D II (mV)')
    ax[1, 2].set_title('ECG sain avec intervalles RR modulés', fontsize=8)
    plt.tight_layout(); plt.savefig('out/fig3_autonome.png', dpi=130); plt.close()
    return R


if __name__ == '__main__':
    H, M, A = fig1()
    for r, n in [(H, 'SAIN'), (M, 'MMVD 0.8'), (A, 'ADDISON 0.9')]:
        print(n, summary(r))
    R3 = fig3()
    for n, r in R3.items():
        h = r['autonomic_ts']['hrv']; print(n, {k: round(v, 2) for k, v in h.items()})
    mc = fig2()
    import json
    json.dump([dict(d=m[0], s=m[1], stage=m[2], g=m[4], **{k: float(v) for k, v in m[3].items()}) for m in mc], open('out/montecarlo.json', 'w'))
    print('ok')
