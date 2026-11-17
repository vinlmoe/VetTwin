"""Calage de la PD du furosémide sur Koh 2021 (BMC Vet Res 17:295, texte intégral lu ; 5 beagles
≈ 11,7 kg ; 2 mg/kg IV et comprimé per os). Utilise la PK du module (fit_pk.py).

Diurèse induite (supplément) par intervalle, mL/kg = (débit horaire − débit de base IV 8,45 mL/h)
× durée / 11,7 kg (tableau 5 ; débit de base IV pris pour les deux voies, celui du per os ayant
un ET de 22 mL/h). Modèle : site d'effet dCe/dt = ke0·(C − Ce) (action depuis la lumière
tubulaire), u = urine_max·Ce^h/(EC50^h + Ce^h) mL/kg/min.

Pertes d'électrolytes : Δ[X] = −a_X·U, U = diurèse effective (dU/dt = 60·u − U/24 h), a_X ajusté
par moindres carrés sur les variations sériques moyennes à 1, 2, 4, 6, 8 h des deux voies
(tableau 4). Contrôle physique : composition de l'urine induite (tableau 6, IV, 0–2 h) Na 122,
K 19,5, Cl 79,5 mmol/L -> volumes apparents a_X⁻¹·C_urine : K 0,73 L/kg (tampon intracellulaire),
Cl 0,34 L/kg (extracellulaire) ; Na baisse peu car l'urine est hypotonique au plasma."""
import numpy as np
from scipy.integrate import solve_ivp
from scipy.optimize import least_squares

CL, VC, F, Q, KA = 0.7357, 2 / 9.65, 0.5498, 0.2006, 0.73
VP = 0.6675 - VC
t = np.linspace(0, 8, 8001)


def conc(route):
    def rhs(_, y):
        g, c, p = y
        return [-KA * g, KA * g - (CL + Q) / VC * c + Q / VP * p, Q / VC * c - Q / VP * p]
    y0 = [F * 2, 0, 0] if route == 'po' else [0, 2, 0]
    return solve_ivp(rhs, [0, 8], y0, t_eval=t, rtol=1e-9, atol=1e-12).y[1] / VC


def effect_site(c, ke0):
    out = np.zeros_like(c); dt = t[1] - t[0]
    for i in range(1, len(t)):
        out[i] = out[i - 1] + dt * ke0 * (c[i - 1] - out[i - 1])
    return out


EDGES = [0, 1, 2, 4, 6, 8]
OBS = {'iv': [11.86, 5.65, 0.21, -0.23, -0.39], 'po': [6.90, 10.34, 4.44, 0.87, -0.32]}
C = {r: conc(r) for r in OBS}


def urine(c, umax, ec, h, ke0):
    ce = effect_site(c, ke0)
    return umax * 60 * ce ** h / (ec ** h + ce ** h)        # mL/kg/h


def vols(u):
    return [np.trapezoid(u[(t >= a) & (t <= b)], t[(t >= a) & (t <= b)]) for a, b in zip(EDGES, EDGES[1:])]


def res(x):
    return np.concatenate([np.array(vols(urine(C[r], *x))) - OBS[r] for r in OBS])


s = least_squares(res, [0.25, 0.3, 2, 3], bounds=([0.05, 0.02, 0.5, 0.1], [2, 20, 8, 50]))
print('urine_max (mL/kg/min), EC50 (mg/L), Hill, ke0 (1/h) :', s.x)
for r in OBS:
    print(r, 'modèle', np.round(vols(urine(C[r], *s.x)), 2), 'publié', OBS[r])

SER = {'k': ([-0.29, -0.51, -0.36, -0.44, -0.33], [-0.08, -0.54, -0.46, -0.47, -0.46]),
       'cl': ([-4.20, -4.70, -3.02, -6.32, -4.30], [1.28, -2.76, -4.40, -2.52, -3.78]),
       'na': ([-3.12, -3.22, -0.66, -3.52, -2.74], [3.22, -0.20, -1.22, 1.58, -0.50])}
Ug = {}
for r in OBS:
    u = urine(C[r], *s.x); U = np.zeros_like(t); dt = t[1] - t[0]
    for i in range(1, len(t)):
        U[i] = U[i - 1] + dt * (u[i - 1] - U[i - 1] / 24.0)
    Ug[r] = np.array([U[np.argmin(abs(t - h))] for h in (1, 2, 4, 6, 8)])
x = np.concatenate([Ug['iv'], Ug['po']])
for k, (iv, po) in SER.items():
    y = np.array(iv + po); a = -(x @ y) / (x @ x)
    print(f'{k} : Δ = −{a:.4f}·U', 'modèle IV', np.round(-a * Ug['iv'], 2), 'PO', np.round(-a * Ug['po'], 2))
