"""Calage du module auscultation sur la bibliothèque TiHo (Heartsound Library, Hanovre) :
mesure les enregistrements réels et la synthèse par la même chaîne, et imprime les tableaux de
docs/VERIFICATION_PCG.md.

  python tests/pcg_calibration_real.py --real DOSSIER --cmd target/release/vetsim-cli [--seeds 8]

DOSSIER : fichiers audio téléchargés depuis la bibliothèque (noms d'origine), HORS du dépôt
(licence CC BY-NC-ND 4.0 : aucun enregistrement copié ni embarqué ; seuls les chiffres sortent).
ffmpeg est requis pour lire les mp3.

Chaîne commune : mono, 4000 Hz, crête à 1 ; enveloppe de pcg_metrics (bande 25–300 Hz, Hilbert,
passe-bas 25 Hz). Repérage de B1 et B2 :
  - réel : pics d'enveloppe appariés comme dans pcg_metrics (intervalle < 0,3 s précédé d'un
    intervalle ≥ 0,3 s) ;
  - synthèse : instants connus (battements et fermeture mitrale renvoyés par le CLI), B2 = pic
    d'enveloppe suivant. L'appariement de pcg_metrics échoue sur la synthèse parce que sa systole
    (0,26 s) est trop proche du seuil de 0,3 s.
Niveaux par bande : valeur efficace dans la fenêtre, rapportée à celle de B1 (bande 50–200 Hz,
±25 ms autour du pic). Systole : de B1 + 60 ms à B2 − 60 ms. Diastole : de B2 + 100 ms à
B1 suivant − 60 ms. Souffle seul = puissance systolique − puissance diastolique.
Ce script MESURE ; il ne modifie aucun paramètre."""
import argparse, base64, glob, json, os, shlex, subprocess, sys, tempfile, warnings
warnings.filterwarnings('ignore')
sys.path.insert(0, 'tests')
import numpy as np
from scipy.signal import butter, find_peaks, hilbert, peak_widths, sosfiltfilt
from pcg_compare_real import FS, load

BANDS = [(50, 100), (100, 200), (200, 400), (400, 800)]
# enregistrements utilisés (préfixe du nom de fichier d'origine, libellé de la bibliothèque)
NORMAL = [('Nr-1.1', 'Arrhythmias 1.1, rythme normal (beagle sain)'), ('Nr-4.1', 'Arrhythmias 4.1, arythmie sinusale physiologique'),
          ('Nr-3.1.2', 'Arrhythmias 3.1.2, bradycardie avec ESV isolées')]
MITRAL = [('HAN_Grade_1', 'Grading 1/6, insuffisance mitrale', 1), ('HAN_Grade_2', 'Grading 2/6 (= Examples 001)', 2),
          ('HAN_Grade_3', 'Grading 3/6 (= Examples 002)', 3), ('003-', 'Examples 003, insuffisance mitrale 4/6', 4),
          ('HAN_Grade_5', 'Grading 5/6, insuffisance mitrale', 5)]
# sévérités MMVD dont le débit régurgitant tombe dans les grades 2, 3, 4 et 5 du modèle
SEVERITY = {2: '0.2', 3: '0.3', 4: '0.4', 5: '0.45'}
rms = lambda v: float(np.sqrt(np.mean(v ** 2)))
db = lambda r: 20 * np.log10(max(r, 1e-12))


def envelope(x):
    y = sosfiltfilt(butter(4, [25, 300], 'band', fs=FS, output='sos'), x)
    env = sosfiltfilt(butter(2, 25, 'low', fs=FS, output='sos'), np.abs(hilbert(y)))
    return np.maximum(env, 0) / env.max()


def pairs_real(env):
    pk, _ = find_peaks(env, height=0.2, distance=int(.1 * FS))
    d = np.diff(pk) / FS
    return [(pk[i], pk[i + 1]) for i in range(len(d)) if d[i] < 0.3 and (i == 0 or d[i - 1] >= 0.3)]


def pairs_known(env, beats, t_mc):
    s1 = []
    for b in beats:
        c = int((b + t_mc + 0.02) * FS); a, e = max(c - int(.05 * FS), 0), c + int(.05 * FS)
        if e >= len(env):
            break
        s1.append(a + int(np.argmax(env[a:e])))
    out = []
    for i, p in enumerate(s1):
        lo, hi = p + int(.09 * FS), p + int(.42 * FS)
        if i + 1 < len(s1):
            hi = min(hi, s1[i + 1] - int(.08 * FS))
        if hi <= len(env) and hi - lo >= 20:
            out.append((p, lo + int(np.argmax(env[lo:hi]))))
    return out


def rise_fall(env, peaks):
    """Durées (ms) entre la mi-hauteur montante et le pic, puis entre le pic et la mi-hauteur."""
    r, f = [], []
    for p in peaks:
        h, i, j = env[p] / 2, p, p
        while i > 0 and env[i] > h: i -= 1
        while j < len(env) - 1 and env[j] > h: j += 1
        r.append((p - i) / FS * 1e3); f.append((j - p) / FS * 1e3)
    return float(np.median(r)), float(np.median(f))


def measure(x, pairs, env):
    s1 = np.array([a for a, _ in pairs]); s2 = np.array([b for _, b in pairs])
    ref = sosfiltfilt(butter(4, [50, 200], 'band', fs=FS, output='sos'), x); w = int(.025 * FS)
    r1 = np.median([rms(ref[a - w:a + w]) for a in s1 if a > w])
    rr = np.diff(s1) / FS
    o = dict(n=len(pairs), hr=60 / float(np.median(rr)), sys=float(np.median((s2 - s1) / FS)), b2b1=float(np.median(env[s2] / env[s1])),
             w1=float(np.median(peak_widths(env, s1, rel_height=.5)[0]) / FS * 1e3),
             w2=float(np.median(peak_widths(env, s2, rel_height=.5)[0]) / FS * 1e3))
    o['r1'], o['f1'] = rise_fall(env, s1); o['r2'], o['f2'] = rise_fall(env, s2)
    e = int(.06 * FS)
    for lo, hi in BANDS:
        z = sosfiltfilt(butter(4, [lo, hi], 'band', fs=FS, output='sos'), x)
        sy = [rms(z[a + e:b - e]) for a, b in pairs if b - a > 2 * e + 40]
        di = [rms(z[pairs[i][1] + int(.1 * FS):pairs[i + 1][0] - e]) for i in range(len(pairs) - 1)
              if 0.06 * FS < (pairs[i + 1][0] - e) - (pairs[i][1] + int(.1 * FS)) < 1.5 * FS]
        s, d = float(np.median(sy)) / r1, (float(np.median(di)) / r1 if di else float('nan'))
        o[f's{lo}'], o[f'd{lo}'] = db(s), db(d)
        o[f'm{lo}'] = db(np.sqrt(s * s - d * d)) if s > 1.12 * d else float('nan')   # souffle seul si ≥ 1 dB au-dessus
    return o


def real(folder, prefix):
    f = glob.glob(os.path.join(folder, prefix + '*'))
    if not f:
        raise SystemExit(f'enregistrement « {prefix}* » absent de {folder}')
    x = load(f[0]); env = envelope(x)
    return measure(x, pairs_real(env), env)


def synth(cmd, args, seeds):
    rows = []
    for seed in range(seeds):
        o = json.loads(subprocess.check_output(shlex.split(cmd) + ['pcg', '--seed', str(seed), '--duration', '20'] + args, text=True))
        with tempfile.NamedTemporaryFile(suffix='.wav', delete=False) as f:
            f.write(base64.b64decode(o['wav_base64']))
        x = load(f.name); os.unlink(f.name); env = envelope(x)
        m = measure(x, pairs_known(env, o['beats'], o['t_mc']), env)
        m.update(grade=o['grade'], q=o['murmur_peak_flow']); rows.append(m)
    return {k: float(np.nanmedian([r[k] for r in rows])) for k in rows[0]}


def all_real(folder):
    out = []
    for f in sorted(glob.glob(os.path.join(folder, '*'))):
        if not os.path.isfile(f) or '001-' in f or '002-' in f:      # 001 et 002 = doublons de Grading 2 et 3
            continue
        x = load(f); env = envelope(x); p = pairs_real(env)
        if len(p) >= 8:
            out.append(measure(x, p, env))
    return out


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('--real', required=True); ap.add_argument('--cmd', required=True); ap.add_argument('--seeds', type=int, default=8)
    a = ap.parse_args()
    f1 = lambda v: '—' if v != v else f'{v:.1f}'
    med = lambda rows, k: float(np.nanmedian([r[k] for r in rows]))

    print('### 1. Bruits normaux et bruit de fond (site base, chien sain)\n')
    R = [real(a.real, p) for p, _ in NORMAL]; S = synth(a.cmd, [], a.seeds)
    print('| Source | FC | B1→B2 (s) | B2/B1 | B1 : largeur / montée / descente (ms) | B2 : largeur / montée / descente (ms) | Fond 50–100 | 100–200 | 200–400 | 400–800 Hz (dB / B1) |')
    print('|---|---|---|---|---|---|---|---|---|---|')
    row = lambda nm, o: print(f"| {nm} | {o['hr']:.0f} | {o['sys']:.3f} | {o['b2b1']:.2f} | {o['w1']:.0f} / {o['r1']:.1f} / {o['f1']:.1f} | {o['w2']:.0f} / {o['r2']:.1f} / {o['f2']:.1f} | "
                              + ' | '.join(f1(o[f'd{lo}']) for lo, _ in BANDS) + ' |')
    for (p, lab), o in zip(NORMAL, R):
        row(lab, o)
    row('**Réel, médiane**', {k: med(R, k) for k in R[0]}); row(f'**Synthèse** ({a.seeds} graines)', S)

    print('\n### 2. Durée de B1 et de B2 sur les 8 enregistrements de calage\n')
    M = [real(a.real, p) for p, _, _ in MITRAL]
    print('| | B1 largeur (ms) | B1 montée | B1 descente | B2 largeur (ms) | B2 montée | B2 descente |\n|---|---|---|---|---|---|---|')
    for nm, o in (('Réel, médiane (3 sans souffle + 5 mitraux)', {k: med(R + M, k) for k in R[0]}), (f'Synthèse, chien sain ({a.seeds} graines)', S)):
        print(f"| {nm} | {o['w1']:.0f} | {o['r1']:.1f} | {o['f1']:.1f} | {o['w2']:.0f} | {o['r2']:.1f} | {o['f2']:.1f} |")

    print('\n### 3. Régurgitation mitrale à l\'apex gauche : rapport B2/B1 et niveau du souffle\n')
    print('Niveau du souffle seul (bruit de fond diastolique retiré), en dB par rapport à B1 ; « — » : pas de souffle distinct du fond dans la bande.\n')
    print('| Source | Grade | B2/B1 | Souffle 100–200 Hz | 200–400 Hz | 400–800 Hz |\n|---|---|---|---|---|---|')
    err = []
    for (p, lab, g), o in zip(MITRAL, M):
        print(f"| {lab} | {g} | {o['b2b1']:.2f} | {f1(o['m100'])} | {f1(o['m200'])} | {f1(o['m400'])} |")
        if g in SEVERITY:
            s = synth(a.cmd, ['--disease', 'mmvd:' + SEVERITY[g], '--site', 'apex'], a.seeds)
            print(f"| Synthèse, MMVD {SEVERITY[g]} à l'apex (débit {s['q']:.0f} mL/s, grade {s['grade']:.0f}) | {s['grade']:.0f} | {s['b2b1']:.2f} | {f1(s['m100'])} | {f1(s['m200'])} | {f1(s['m400'])} |")
            err += [s[k] - o[k] for k in ('m100', 'm200', 'm400') if o[k] == o[k] and s[k] == s[k]]
    print(f"\nRapport B2/B1 réel à l'apex : médiane {med(M, 'b2b1'):.2f}. Écart quadratique moyen synthèse − réel sur les niveaux du souffle : {np.sqrt(np.mean(np.square(err))):.1f} dB ({len(err)} valeurs).")

    print('\n### 4. Ensemble de la bibliothèque : intervalle B1→B2 et rapport B2/B1\n')
    allr = all_real(a.real)
    sy = [r['sys'] for r in allr]; b = [r['b2b1'] for r in allr]
    print(f"{len(allr)} enregistrements avec au moins 8 paires B1/B2 repérées. B1→B2 : médiane {np.median(sy):.3f} s (de {min(sy):.3f} à {max(sy):.3f}). "
          f"B2/B1 : médiane {np.median(b):.2f} (de {min(b):.2f} à {max(b):.2f}). Synthèse, chien sain : B1→B2 {S['sys']:.3f} s.")
