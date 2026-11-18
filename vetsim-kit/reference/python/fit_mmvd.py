import numpy as np
from scipy.optimize import least_squares
import diseases
from patient import Patient
KEYS=['g','gexp','cpv','v0','ret','aut']
FIX_AUT=0.5
SEV=dict(B1=0.17,B2=0.45,C=0.72,D=0.92)
# cibles par stade : (RF, POG mmHg) ; sources : régurgitation 17-81 % (modèle expérimental, JVIM 2004), compensée jusqu'à ~75 % (MSD), œdème pulmonaire si POG > 20 mmHg (MSD)
TG=dict(B1=(0.10,6.5),B2=(0.30,10.0),C=(0.55,22.0),D=(0.70,26.0))
def run(x):
    diseases.KM.update(dict(zip(KEYS,list(x[:5])+[FIX_AUT]))); diseases.KM['zsd']=0.0
    out={}
    for st,sv in SEV.items():
        pt=Patient(20.0,seed=0,variability=False); pt.add_disease('MMVD',severity=sv); r=pt.state(signals=False)
        h=r['hemo']; out[st]=(h['reg_fraction'],h['la_mean'],h['map'],h['co'],r['p']['hr'])
    return out
def res(x):
    o=run(x); v=[]
    for st,(rf,la) in TG.items():
        v+= [(o[st][0]-rf)/0.08,(o[st][1]-la)/2.5]
        v+= [(o[st][2]-100)/20,(o[st][3]-3.0)/1.0,(o[st][4]-{'C':130,'D':160}[st])/25] if st in('C','D') else []
    return v
x0=[4.4,2.37,0.2,0.1,0.13]
s=least_squares(res,x0,diff_step=0.05,bounds=([0.3,0.8,0.1,0.05,0.0],[10,3.5,3,1.2,1.0]),max_nfev=40)
print(dict(zip(KEYS,np.round(s.x,3)))); o=run(s.x)
for st,v in o.items(): print(st,np.round(v,2),TG[st])
