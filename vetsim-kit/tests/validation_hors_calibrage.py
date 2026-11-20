"""Volet stationnaire de docs/VALIDATION_HORS_CALIBRAGE.tex : grandeurs dérivées des sorties du CLI
Rust (référence normative) comparées à la littérature dans le rapport.

    python tests/validation_hors_calibrage.py --cmd target/release/vetsim-cli

Imprime : poids corporel (FC, DC/kg, DFG/kg, créatinine), gradient alvéolo-artériel, pente de
compensation respiratoire des acidoses métaboliques (MRC, Addison), réponse à l'hypoxémie
(pneumonie), troubles du rythme, et le contrôle de cohérence DC = FC × VS (hors MMVD, où la
régurgitation sépare les deux).
"""
import argparse
import json
import subprocess

ap = argparse.ArgumentParser()
ap.add_argument('--cmd', default='target/release/vetsim-cli')
a = ap.parse_args()


def ev(*args):
    r = subprocess.run([a.cmd, 'eval', *args], capture_output=True, text=True)
    if r.returncode != 0:
        return {'erreur': r.stderr.strip()}
    return json.loads(r.stdout)


def a_a(d):
    """Gradient alvéolo-artériel (air ambiant, Pb 760, QR 0,8 de l'équation clinique)."""
    return 0.21 * (760 - 47) - d['pco2'] / 0.8 - d['pao2']


print('== Poids corporel (chien sain)')
for bw in (3, 5, 10, 20, 30, 40, 60):
    d = ev('--bw', str(bw))
    print(f"{bw:>3} kg  FC {d['hr']:6.1f}  DC/kg {1000 * d['co'] / bw:6.1f} mL/min/kg  "
          f"DFG/kg {d['gfr'] / bw:5.2f} mL/min/kg  créat {d['creat']:5.1f} µmol/L  PaCO2 {d['pco2']:5.1f}")

print('== Gradient A-a (air ambiant)')
for dis in ([], ['--disease', 'pneumonia:0.5'], ['--disease', 'pneumonia:1'], ['--disease', 'mmvd:1']):
    d = ev('--bw', '20', *dis)
    print(f"{' '.join(dis) or 'sain':28s} PaO2 {d['pao2']:5.1f}  PaCO2 {d['pco2']:5.1f}  A-a {a_a(d):5.1f}  "
          f"SaO2 {d['sao2']:5.1f}  FC {d['hr']:5.1f}  PAM {d['map']:5.1f}  FR {d['rr']:4.1f}  lactate {d['lactate']:4.2f}")

print('== Compensation respiratoire des acidoses métaboliques (pente ΔPaCO2/ΔHCO3)')
h = ev('--bw', '20')
for dis in ('ckd', 'addison'):
    for s in (0.5, 0.8, 1.0):
        d = ev('--bw', '20', '--disease', f'{dis}:{s}')
        dh, dp = d['hco3'] - h['hco3'], d['pco2'] - h['pco2']
        pente = dp / dh if abs(dh) > 0.3 else float('nan')
        print(f"{dis}:{s}  HCO3 {d['hco3']:5.2f} (Δ {dh:+5.2f})  PaCO2 {d['pco2']:5.2f} (Δ {dp:+5.2f})  pente {pente:4.2f}  "
              f"pH {d['ph']:5.3f}  Na {d['na']:5.1f}  K {d['k']:4.2f}  Na/K {d['na'] / d['k']:4.1f}  glucose {d['glucose']:4.2f}")

print('== Troubles du rythme et hypovolémie : cohérence DC = FC × VS')
cas = [('sain', []), ('av_block:1', ['--disease', 'av_block:1']), ('atrial_fibrillation:0.8', ['--disease', 'atrial_fibrillation:0.8']),
       ('atrial_fibrillation:1', ['--disease', 'atrial_fibrillation:1']), ('sick_sinus:1', ['--disease', 'sick_sinus:1']),
       ('addison:1', ['--disease', 'addison:1'])] + [(f'déficit {D} mL', ['--D', str(D)]) for D in (170, 340, 510)]
for nom, args in cas:
    d = ev('--bw', '20', *args)
    if 'erreur' in d:
        print(f"{nom:26s} {d['erreur']}")
        continue
    hs = d['hr'] * d['sv'] / 1000
    print(f"{nom:26s} FC {d['hr']:6.1f}  PAM {d['map']:5.1f}  DC {d['co']:5.3f}  FC×VS {hs:5.3f}  écart {100 * (d['co'] / hs - 1):+5.1f} %")
