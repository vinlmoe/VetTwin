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
# --- furosémide : IV t1/2 1-1.5 h (BMC Vet Res 2021), Vss 0.52 L/kg (RCP), Cmax 2.126 mg/L à 5 mg/kg PO, 0.9 à 1.9 mg/kg, Tmax 0.8-1.1 h
def resf(x):
    vc,q,f,ka=x; vp=0.52-vc; cl=0.30
    t,c=pk(vc,vp,cl,q,ka,f,5.0,'po'); cm,tm,_,_=metrics(c,t)
    t,c2=pk(vc,vp,cl,q,ka,f,1.9,'po'); cm2=c2.max()
    t,c3=pk(vc,vp,cl,q,ka,f,2.0,'iv'); _,_,th,_=metrics(c3,t)
    return [(cm-2.126)/0.4,(cm2-0.9)/0.2,(tm-1.0)/0.3,(th-1.25)/0.3]
s=least_squares(resf,[0.25,0.25,0.4,2.0],bounds=([0.1,0.05,0.15,0.5],[0.45,2,0.9,6])); print('furo',s.x,resf(s.x))
# --- pimobendan PO 0.5 mg/kg : Cmax 49.1 ng/mL, Tmax 2.1 h, t1/2 1.8 h, AUC 148.4 ng h/mL (Her 2020)
def resp(x):
    vc,vp,cl,ka=x; q=1.0; f=0.5
    t,c=pk(vc,vp,cl,q,ka,f,0.5,'po',T=24); cm,tm,th,auc=metrics(c*1000,t)
    return [(cm-49.1)/10,(tm-2.1)/0.5,(th-1.8)/0.4,(auc-148.4)/30]
s2=least_squares(resp,[2.0,1.0,1.7,1.0],bounds=([0.3,0.1,0.3,0.2],[8,8,6,5])); print('pimo',s2.x,resp(s2.x))
