import json, numpy as np
d=json.load(open('out/calibration.json'))
def f(x,n=2):
    return ('%.'+str(n)+'f')%x
def tex(s): return s.replace('%','\\%').replace('_','\\_').replace('&','\\&')
L=[]
for r in d['healthy']:
    n=0 if abs(r['target'])>=50 else (1 if abs(r['target'])>=5 else 2)
    st='oui' if r['ok'] else 'non'
    L.append(f"{tex(r['var'])} & {f(r['target'],n)} $\\pm$ {f(r['tol'],n)} & {f(r['model_mean'],n)} $\\pm$ {f(r['model_sd'],n)} & {st} & {tex(r['source'].replace('*','$^*$'))} \\\\")
open('out/tab_healthy.tex','w').write('\n'.join(L))
L=[]
for r in d['pk']:
    n=0 if r['target']>=20 else 1
    L.append(f"{tex(r['module'].replace('PK ',''))} & {tex(r['var'])} & {f(r['target'],n)} {tex(r['unit'])} & {f(r['model_mean'],n)} & {tex(r['source'])} \\\\")
open('out/tab_pk.tex','w').write('\n'.join(L))
L=[]
for k,v in d['disease'].items():
    dn,st=k.split('|'); a=lambda q: np.array([x[q] for x in v])
    if dn=='MMVD':
        L.append(f"MMVD {st} & {a('hr').mean():.0f} & {100*a('rf').mean():.0f} & {a('la').mean():.1f} $\\pm$ {a('la').std():.1f} & {a('map').mean():.0f} & {a('co').mean():.2f} & {a('pao2').mean():.0f} \\\\")
open('out/tab_mmvd.tex','w').write('\n'.join(L))
L=[]
for k,v in d['disease'].items():
    dn,st=k.split('|'); a=lambda q: np.array([x[q] for x in v])
    if dn=='CKD':
        L.append(f"{st} & {a('creat').mean():.0f} ({np.percentile(a('creat'),5):.0f}--{np.percentile(a('creat'),95):.0f}) & {a('urea').mean():.0f} & {a('hco3').mean():.1f} & {a('map').mean():.0f} \\\\")
open('out/tab_ckd.tex','w').write('\n'.join(L))
L=[]
for k,v in d['disease'].items():
    dn,st=k.split('|'); a=lambda q: np.array([x[q] for x in v])
    if dn=='Addison':
        L.append(f"{st} & {a('na').mean():.0f} & {a('k').mean():.1f} & {(a('na')/a('k')).mean():.1f} & {a('map').mean():.0f} & {a('creat').mean():.0f} \\\\")
open('out/tab_add.tex','w').write('\n'.join(L))
