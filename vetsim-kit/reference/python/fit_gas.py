import numpy as np
from scipy.optimize import least_squares
from params import healthy_params
from physio import evaluate
def mk(x):
    p=healthy_params(20); p['xa']=x[0]; p['va0']=x[1]; return p
def res(x):
    r=evaluate(mk(x)); a=r['art']
    return [(a['ph']-7.38)/0.03,(a['pco2']-40.2)/3]
s=least_squares(res,[2.5,0.126],diff_step=0.02); print(s.x,res(s.x))
r=evaluate(mk(s.x)); print(r['art']); print(r['ven']); print(r['chem']['anion_gap'])
