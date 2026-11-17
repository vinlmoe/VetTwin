import numpy as np
from scipy.integrate import solve_ivp
from scipy.optimize import least_squares
def pk(vc,vp,cl,q,ka,f,dose,route,T=24):
    # unités : mg/kg, L/kg, L/h/kg ; conc en mg/L
    def rhs(t,y):
        g,c,pp=y
        return [-ka*g, ka*g-(cl+q)/vc*c+q/vp*pp, q/vc*c-q/vp*pp]
    y0=[f*dose,0,0] if route=='po' else [0,dose,0]
    t=np.linspace(0,T,2401); s=solve_ivp(rhs,[0,T],y0,t_eval=t,rtol=1e-8,atol=1e-10)
    return t,s.y[1]/vc*(1 if route=='po' else 1)
def metrics(c,t):
    i=c.argmax(); late=(t>t[i]*1+1)&(c>1e-6*c.max()+1e-12)
    # t1/2 terminal : pente de log c sur 3e-8 de Cmax... 
    m=c>c.max()*0.02; tt=t[(t>t[i]+0.5)&m][-200:] if True else None
    cc=c[(t>t[i]+0.5)&m][-200:]
    k=-np.polyfit(tt[len(tt)//2:],np.log(cc[len(cc)//2:]),1)[0]
    return c.max(),t[i],np.log(2)/k,np.trapezoid(c,t)
# --- furosémide : Koh 2021 (BMC Vet Res 17:295, texte intégral lu, tableau 2), 5 beagles ≈ 11,7 kg, 2 mg/kg.
# Fixés par l'article : CL 735,7 mL/h/kg ; Vss 0,6675 L/kg ; Vc = dose/C0 = 2/9,65 ; F 0,5498.
# Ajustés : Q sur t1/2 IV 2,07 h (pente de ln C sur ]2, 8[ h, comme calibrate.py) ; ka sur Cmax PO 0,61 mg/L.
# Écarts résiduels : Tmax PO 0,5 h (publié 1, 0,5-1,5) ; t1/2 apparente PO 1,5 h (publié 3,44 ± 1,79).
# Dans le module, cl est rapportée à 20 kg (allométrie BW^0,75 du moteur) : 12,26·(11,7/20)^0,25 = 10,72 mL/min/kg.
CL_F, VC_F, F_F = 0.7357, 2 / 9.65, 0.5498
VP_F = 0.6675 - VC_F
def th_iv(c, t):
    m = (t > 2) & (t < 8); return np.log(2) / (-np.polyfit(t[m], np.log(c[m]), 1)[0])
def resf(x):
    q, ka = x
    t, c = pk(VC_F, VP_F, CL_F, q, ka, F_F, 2.0, 'iv', T=24)
    t, cp = pk(VC_F, VP_F, CL_F, q, ka, F_F, 2.0, 'po', T=24)
    return [(th_iv(c, t) - 2.07) / 0.3, (cp.max() - 0.61) / 0.1]
s = least_squares(resf, [0.3, 1.0], bounds=([0.01, 0.1], [5, 10]))
print('furo Q (L/h/kg), ka (1/h)', s.x, resf(s.x), '-> q mL/min/kg', s.x[0] * 1000 / 60, ', cl à 20 kg', CL_F * 1000 / 60 * (11.7 / 20) ** 0.25)
# --- pimobendan PO 0.5 mg/kg : Cmax 49.1 ng/mL, Tmax 2.1 h, t1/2 1.8 h, AUC 148.4 ng h/mL (Her 2020)
def resp(x):
    vc,vp,cl,ka=x; q=1.0; f=0.5
    t,c=pk(vc,vp,cl,q,ka,f,0.5,'po',T=24); cm,tm,th,auc=metrics(c*1000,t)
    return [(cm-49.1)/10,(tm-2.1)/0.5,(th-1.8)/0.4,(auc-148.4)/30]
s2=least_squares(resp,[2.0,1.0,1.7,1.0],bounds=([0.3,0.1,0.3,0.2],[8,8,6,5])); print('pimo',s2.x,resp(s2.x))
