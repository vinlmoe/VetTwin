"""Métriques d'acceptation PCG : python PCG_compare.py fichier.wav -> JSON (spectre, paires B1/B2, systole)."""
import sys, json, numpy as np
from scipy.io import wavfile
from scipy.signal import welch, butter, sosfiltfilt, hilbert, find_peaks
def metrics(f):
    fs, x = wavfile.read(f); x = x.astype(float); x = x.mean(1) if x.ndim > 1 else x; x /= np.abs(x).max()
    fr, P = welch(x, fs, nperseg=min(len(x), int(fs * .5))); m = (fr >= 20) & (fr <= 1000); tot = P[m].sum()
    bands = [(20, 50), (50, 100), (100, 200), (200, 400)]
    frac = [float(P[(fr >= a) & (fr < b)].sum() / tot) for a, b in bands]
    y = sosfiltfilt(butter(4, [25, 300], 'band', fs=fs, output='sos'), x)
    env = sosfiltfilt(butter(2, 25, 'low', fs=fs, output='sos'), np.abs(hilbert(y))); env /= env.max()
    p, _ = find_peaks(env, height=0.2, distance=int(.1 * fs)); t = p / fs; a = env[p]; d = np.diff(t)
    r, sy = [], []
    for i in range(len(d)):
        if d[i] < 0.3 and (i == 0 or d[i - 1] >= 0.3): r.append(a[i + 1] / a[i]); sy.append(d[i])
    r = np.array(r)
    return dict(centroide_hz=float((fr[m] * P[m]).sum() / tot), bandes_20_50_50_100_100_200_200_400=frac,
                n_paires=len(r), ratio_B2_B1_median=float(np.median(r)) if len(r) else None,
                part_B2_gt_B1=float(np.mean(r > 1)) if len(r) else None,
                systole_B1_B2_median_s=float(np.median(sy)) if len(sy) else None)
if __name__ == '__main__': print(json.dumps(metrics(sys.argv[1]), indent=1))
