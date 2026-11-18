import json, numpy as np, matplotlib; matplotlib.use('Agg'); import matplotlib.pyplot as plt
d=json.load(open('out/calibration.json'))['healthy']
lab=[r['var'] for r in d]; z=[(r['model_mean']-r['target'])/r['tol'] for r in d]
col=['#2a7' if abs(x)<=1 else '#c33' for x in z]
fig,ax=plt.subplots(figsize=(7,7.2)); y=np.arange(len(d))[::-1]
ax.barh(y,z,color=col); ax.axvline(0,c='k',lw=.8); ax.axvspan(-1,1,color='#ccc',alpha=.4,zorder=0)
ax.set_yticks(y); ax.set_yticklabels(lab,fontsize=8); ax.set_xlabel("(moyenne modèle − cible) / tolérance"); ax.set_xlim(-2.5,2.5)
ax.set_title("Patient sain (n = 50, 20 kg) : écart aux cibles canines publiées",fontsize=10)
plt.tight_layout(); plt.savefig('out/fig4_calibration.png',dpi=160)
