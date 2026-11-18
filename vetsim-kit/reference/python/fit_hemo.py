import numpy as np, json
from scipy.optimize import least_squares
from params import healthy_params
from physio import evaluate
BW=20.0
BSA=10.1*(BW*1000)**(2/3)/1e4
T=dict(map=(103,6),co=(4.42*BSA,0.5),pap=(14,2.5),la=(5.5,2),ef=(0.55,0.06))   # (cible, tolérance)
def mk(x):
    p=healthy_params(BW); p['hr']=87.0; p['map_set']=103.0
    p['r_sys']*=np.exp(x[0]); p['vol_shift']=x[1]; p['ees_lv']*=np.exp(x[2]); p['r_pul']*=np.exp(x[3]); p['b_lv']*=np.exp(x[4])
    return p
def res(x):
    r=evaluate(mk(x)); h=r['hemo']
    v=dict(map=h['map'],co=h['co'],pap=h['pap_mean'],la=h['la_mean'],ef=h['ef'])
    return np.array([(v[k]-T[k][0])/T[k][1] for k in T])
x0=np.array([-0.2,200,0.0,0.0,0.0])
s=least_squares(res,x0,diff_step=0.05,bounds=([-1,-300,-0.7,-1,-1.5],[1,1500,0.7,1,1.5]),max_nfev=60)
print(s.x, s.cost, res(s.x))
r=evaluate(mk(s.x));h=r['hemo']
print({k:round(float(h[k]),2) for k in ['map','sap','dap','co','sv','edv','esv','ef','la_mean','pap_mean','cvp','lv_edp']}, r['p']['hr'])
json.dump(s.x.tolist(),open('out/fit_hemo.json','w'))
