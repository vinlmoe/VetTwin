"""Comparaison de la synthèse d'auscultation avec de VRAIS enregistrements.

  python tests/pcg_compare_real.py --real DOSSIER --cmd target/release/vetsim-cli \
      [--args "--hr 90"] [--seeds 20] [--report rapport.md] [--json mesures.json]

DOSSIER : enregistrements réels (.wav ; autres formats convertis par ffmpeg s'il est installé).
Il doit rester HORS du dépôt : la bibliothèque TiHo est sous CC BY-NC-ND 4.0, aucun
enregistrement n'est copié ni embarqué ; seuls les chiffres mesurés sortent de ce script.
--args : arguments passés à « vetsim-cli pcg » (ex. "--disease mmvd:0.3" pour un souffle mitral).

Les deux côtés passent par la même chaîne : mono, rééchantillonnage à 4000 Hz, crête à 1.
Mesures (en plus de reference/python/pcg_metrics.py) :
  - spectre : énergie par bande jusqu'à 1000 Hz, pente 100-800 Hz (dB/octave) ;
  - bruit de fond : niveau (dB sous la crête des bruits) et centroïde des passages calmes ;
  - bruits : durée à mi-hauteur et à 20 % de l'enveloppe, variabilité d'amplitude (CV) ;
  - systole (souffle) : niveau moyen entre B1 et B2 rapporté à B1, forme (milieu / bords :
    1 = plateau, > 1 = crescendo-decrescendo), centroïde de l'intervalle systolique.
Ce script MESURE et affiche les écarts ; il ne modifie aucun paramètre."""
import argparse, base64, glob, io, json, os, shlex, shutil, subprocess, sys, tempfile
from math import gcd
sys.path.insert(0, 'reference/python')
import numpy as np
from scipy.io import wavfile
from scipy.signal import butter, find_peaks, hilbert, peak_widths, resample_poly, sosfiltfilt, welch
import pcg_metrics

FS = 4000


def load(path):
    """Signal mono à 4000 Hz, crête à 1."""
    if not path.lower().endswith('.wav'):
        if not shutil.which('ffmpeg'):
            raise SystemExit(f'{path} : format non WAV et ffmpeg absent')
        raw = subprocess.check_output(['ffmpeg', '-v', 'error', '-i', path, '-ac', '1', '-f', 'wav', '-'])
        fs, x = wavfile.read(io.BytesIO(raw))
    else:
        fs, x = wavfile.read(path)
    x = x.astype(float)
    if x.ndim > 1:
        x = x.mean(1)
    if fs != FS:
        g = gcd(int(fs), FS)
        x = resample_poly(x, FS // g, int(fs) // g)
    x -= x.mean()
    return x / max(np.abs(x).max(), 1e-12)


def centroid(x, lo=20, hi=1000):
    fr, p = welch(x, FS, nperseg=min(len(x), FS // 4))
    m = (fr >= lo) & (fr <= hi)
    return float((fr[m] * p[m]).sum() / p[m].sum())


def measure(x):
    """Mesures d'un signal (4000 Hz, crête à 1)."""
    with tempfile.NamedTemporaryFile(suffix='.wav', delete=False) as f:
        tmp = f.name
    wavfile.write(tmp, FS, np.round(x * 32767).astype(np.int16))
    out = dict(pcg_metrics.metrics(tmp))
    os.unlink(tmp)
    bands = out.pop('bandes_20_50_50_100_100_200_200_400')
    for k, v in zip(('e_20_50', 'e_50_100', 'e_100_200', 'e_200_400'), bands):
        out[k] = v
    fr, p = welch(x, FS, nperseg=min(len(x), FS // 2))
    tot = p[(fr >= 20) & (fr <= 1000)].sum()
    out['e_400_1000'] = float(p[(fr >= 400) & (fr <= 1000)].sum() / tot)
    m = (fr >= 100) & (fr <= 800)
    out['pente_db_oct'] = float(np.polyfit(np.log2(fr[m]), 10 * np.log10(p[m] + 1e-30), 1)[0])
    # même enveloppe que pcg_metrics
    y = sosfiltfilt(butter(4, [25, 300], 'band', fs=FS, output='sos'), x)
    env = sosfiltfilt(butter(2, 25, 'low', fs=FS, output='sos'), np.abs(hilbert(y)))
    env = np.maximum(env, 0) / env.max()
    pk, _ = find_peaks(env, height=0.2, distance=int(.1 * FS))
    if len(pk) < 2:
        return out
    a = env[pk]
    out['cv_amplitude'] = float(a.std() / a.mean())
    out['duree_mi_hauteur_ms'] = float(np.median(peak_widths(env, pk, rel_height=0.5)[0]) / FS * 1e3)
    out['duree_20pc_ms'] = float(np.median(peak_widths(env, pk, rel_height=0.8)[0]) / FS * 1e3)
    # passages calmes : échantillons sous le 20e centile de l'enveloppe
    quiet = env <= np.quantile(env, 0.2)
    out['fond_db'] = float(20 * np.log10(max(np.sqrt(np.mean(x[quiet] ** 2)), 1e-9) / np.median(a)))
    if quiet.sum() > FS // 4:
        out['fond_centroide_hz'] = centroid(x[quiet])
    # intervalle systolique : entre un pic et le suivant s'ils sont appariés (< 0,3 s), sans les 60 ms de bord
    lvl, shape, cen = [], [], []
    d = np.diff(pk) / FS
    edge = int(.06 * FS)
    for i in range(len(d)):
        if d[i] < 0.3 and (i == 0 or d[i - 1] >= 0.3):
            s = slice(pk[i] + edge, pk[i + 1] - edge)
            seg = env[s]
            if len(seg) < 12:
                continue
            third = len(seg) // 3
            lvl.append(seg.mean() / a[i])
            shape.append(seg[third:2 * third].mean() / max(.5 * (seg[:third].mean() + seg[2 * third:].mean()), 1e-9))
            if len(seg) >= 128:
                cen.append(centroid(x[s]))
    if lvl:
        out['systole_niveau'] = float(np.median(lvl))
        out['systole_forme'] = float(np.median(shape))
    if cen:
        out['systole_centroide_hz'] = float(np.median(cen))
    return out


LABELS = [
    ('centroide_hz', 'Centroïde spectral (Hz)'), ('e_20_50', 'Énergie 20–50 Hz'), ('e_50_100', 'Énergie 50–100 Hz'),
    ('e_100_200', 'Énergie 100–200 Hz'), ('e_200_400', 'Énergie 200–400 Hz'), ('e_400_1000', 'Énergie 400–1000 Hz'),
    ('pente_db_oct', 'Pente spectrale 100–800 Hz (dB/octave)'), ('systole_B1_B2_median_s', 'B1→B2 médian (s)'),
    ('ratio_B2_B1_median', 'Rapport B2/B1 médian'), ('part_B2_gt_B1', 'B2 > B1 (part des paires)'),
    ('duree_mi_hauteur_ms', 'Durée des bruits à mi-hauteur (ms)'), ('duree_20pc_ms', 'Durée des bruits à 20 % (ms)'),
    ('cv_amplitude', 'Variabilité d\'amplitude (CV)'), ('fond_db', 'Bruit de fond (dB / crête des bruits)'),
    ('fond_centroide_hz', 'Centroïde du bruit de fond (Hz)'), ('systole_niveau', 'Niveau systolique / B1'),
    ('systole_forme', 'Forme systolique (milieu / bords)'), ('systole_centroide_hz', 'Centroïde systolique (Hz)'),
]


def med(rows, k):
    v = [r[k] for r in rows if r.get(k) is not None]
    return (float(np.median(v)), len(v)) if v else (None, 0)


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('--real', required=True); ap.add_argument('--cmd', required=True)
    ap.add_argument('--args', default=''); ap.add_argument('--seeds', type=int, default=20)
    ap.add_argument('--report'); ap.add_argument('--json')
    a = ap.parse_args()
    files = sorted(f for f in glob.glob(os.path.join(a.real, '*')) if os.path.isfile(f))
    if not files:
        raise SystemExit(f'aucun enregistrement dans {a.real}')
    real = [measure(load(f)) for f in files]
    synth = []
    for seed in range(a.seeds):
        o = json.loads(subprocess.check_output(shlex.split(a.cmd) + ['pcg', '--seed', str(seed)] + shlex.split(a.args), text=True))
        with tempfile.NamedTemporaryFile(suffix='.wav', delete=False) as f:
            f.write(base64.b64decode(o['wav_base64']))
        synth.append(measure(load(f.name)))
        os.unlink(f.name)
    lines = [f'Réel : {len(files)} enregistrement(s) ; synthèse : {a.seeds} graines, arguments « {a.args} ». Médianes.', '',
             '| Mesure | Réel | n | Synthèse | Écart |', '|---|---|---|---|---|']
    for k, lab in LABELS:
        (r, n), (s, _) = med(real, k), med(synth, k)
        f = lambda v: '—' if v is None else f'{v:.3g}'
        gap = '—' if r is None or s is None else (f'{s - r:+.3g}' + (f' ({100 * (s - r) / abs(r):+.0f} %)' if r else ''))
        lines.append(f'| {lab} | {f(r)} | {n} | {f(s)} | {gap} |')
    txt = '\n'.join(lines)
    print(txt)
    if a.report:
        open(a.report, 'w').write(txt + '\n')
    if a.json:
        # noms de fichiers réels volontairement absents : seuls les chiffres sortent
        json.dump(dict(real=real, synth=synth), open(a.json, 'w'), indent=1)
