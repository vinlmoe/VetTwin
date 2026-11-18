import numpy as np, itertools
from params import healthy_params
from physio import evaluate
from autonomic import simulate_autonomic
from multiprocessing import Pool
def run(args):
    av,rd,vn,sn=args
    p=healthy_params(20); p.update(a_v=av,rsa_depth=rd,vag_noise=vn,sym_noise=sn)
    r=evaluate(p); out=[]
    for sd in (1,2):
        h=simulate_autonomic(r,dur=300,seed=sd)['hrv']; out.append((h['sdnn_ms'],h['rmssd_ms'],h['lf_hf'],h['fc_moy']))
    o=np.mean(out,0); return args,o
if __name__=='__main__':
    G=list(itertools.product([0.6,0.8],[0.8,1.0],[0.0,0.15],[0.06,0.2]))
    with Pool(2) as P:
        for a,o in P.imap(run,G): print(a,np.round(o,2))
