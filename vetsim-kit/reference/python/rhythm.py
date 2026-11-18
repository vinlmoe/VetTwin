"""Couche de conduction : génération et propagation de l'influx, battement par battement.

Simulation par événements (implémentation secondaire ; référence normative : `crates/vetsim-core/src/rhythm.rs`).
Chaque trouble du rythme naît d'un mécanisme, rien n'est plaqué :
- nœud sinusal : décharges à la période sinusale ; bloc de sortie sino-atrial (pauses) avec la
  probabilité `sa_block` ;
- oreillettes : activées par le nœud sinusal, ou, en fibrillation (`af` = 1), par une suite
  d'activations rapides et irrégulières (cycle moyen `af_cycle`, coefficient de variation
  `af_cv`), sans contraction organisée. En hyperkaliémie (K ≥ `k_atrial_standstill`), le
  myocarde atrial est inexcitable : ni onde P ni contraction, mais l'influx sinusal gagne encore
  le nœud AV (rythme sino-ventriculaire) ;
- nœud AV : un influx arrivant moins de `ERP` après la dernière activation nodale est bloqué ;
  s'il arrive en réfractarité relative (seconde moitié de l'ERP), il pénètre partiellement le nœud
  sans le traverser (conduction cachée : la récupération recule de `av_conceal` × le temps écoulé) ;
  sinon délai `AH = AH0 + av_beta·exp(−(récupération − ERP)/av_tau)`. Le tonus vagal
  effectif et K⁺ allongent `AH0` et `ERP`. Wenckebach et filtrage de la FA en émergent ;
- His-Purkinje : conduction avec la probabilité `hp_conduction`, réduite au-delà de `k_hp_block`
  (bloc infranodal, Mobitz II, 3e degré) ;
- échappements : foyers jonctionnel (`junct_rate`) et ventriculaire (`vent_escape_rate`), remis à
  zéro par toute activation ventriculaire ; suppression par entraînement (overdrive) : le premier
  intervalle d'un foyer est allongé de `1 + esc_sup_gain·max(0, cycle·f_moy − 1)`, f_moy étant la
  fréquence ventriculaire moyennée sur `esc_sup_tau` (charge de la pompe Na/K) ; le jonctionnel est sous le bloc nodal mais au-dessus
  du bloc infranodal ;
- foyer ectopique ventriculaire : décharges de Poisson (`vpc_rate` /min, accrues en hypokaliémie
  par `vpc_k_gain`) ; efficace seulement hors de la période réfractaire ventriculaire (QT).
  La pause compensatrice émerge : l'onde P suivante trouve le nœud AV réfractaire.

Aléa : uniquement des tirages uniformes u ∈ [0, 1) (rejouables à l'identique en Rust).
Valeurs des paramètres : placeholders (evidence D) sauf mention dans docs/PROVENANCE.md.
"""
import math

ORG_SINUS, ORG_JUNCT, ORG_VESC, ORG_VPC = 'sinus', 'junctional', 'ventricular_escape', 'ectopic'
SQRT3 = math.sqrt(3.0)
_M64 = (1 << 64) - 1


class SplitMix64:
    """Générateur SplitMix64 (Steele, Lea, Flood 2014), écrit à l'identique en Rust
    (`rhythm::SplitMix64`) : le moteur tire l'aléa de la conduction avec lui, d'où des séquences
    identiques au bit près entre la référence et le port. u = (x >> 11)·2⁻⁵³ ∈ [0, 1)."""

    def __init__(self, seed):
        self.state = seed & _M64

    def next_u64(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & _M64
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & _M64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & _M64
        return z ^ (z >> 31)

    def random(self):
        return (self.next_u64() >> 11) * (1.0 / (1 << 53))


def may_be_trivial(p):
    """Condition nécessaire (paramètres) d'un rythme sinusal régulier : aucun processus aléatoire
    ni arrêt atrial."""
    return (p['af'] == 0.0 and p['sa_block'] == 0.0 and p['vpc_rate'] == 0.0 and p['hp_conduction'] >= 1.0
            and p['k'] < p['k_atrial_standstill'] and p['k'] < p['k_hp_block'] and p['k'] >= p['k_vpc'])


def is_trivial(p, v_eff, sinus_period, n_beats=40):
    """Vrai si la conduction produit un rythme sinusal conduit 1:1 à PR constant (vérifié sur la
    séquence simulée) : l'hémodynamique garde alors la simulation périodique d'origine."""
    if not may_be_trivial(p):
        return False
    ev = simulate_conduction(p, v_eff, sinus_period, n_beats * sinus_period, lambda: 0.0)
    v = ev['ventricular']
    if len(v) < n_beats or any(b['origin'] != ORG_SINUS for b in v):
        return False
    prs = [b['pr'] for b in v[-10:]]                       # PR stabilisé (convergence géométrique)
    return max(prs) - min(prs) < 1e-6


def steady_pr(p, v_eff, sinus_period, n_beats=40):
    """PR (s) du rythme sinusal régulier stabilisé : délai mécanique oreillette → ventricule."""
    ev = simulate_conduction(p, v_eff, sinus_period, n_beats * sinus_period, lambda: 0.0)
    return ev['ventricular'][-1]['pr']


def av_constants(p, v_eff):
    """Délai nodal minimal AH0 et période réfractaire nodale ERP (s), modulés par le vagal et K⁺.
    AH0 est tel que, récupéré, le PR vaut `pr` au tonus vagal de référence."""
    vag = 1.0 + p['av_vagal'] * (v_eff - 0.6)
    kf = 1.0 + p['av_k_gain'] * max(0.0, p['k'] - 5.5)
    ah0 = max(0.0, p['pr'] - p['hv']) * vag * kf
    erp = p['av_erp'] * vag * kf
    return ah0, erp


def simulate_conduction(p, v_eff, sinus_period, t_end, rand, sinus_times=None):
    """Événements de conduction sur [0, t_end].

    p : paramètres effectifs ; v_eff : tonus vagal effectif ; sinus_period : période sinusale (s)
    (ignorée si `sinus_times` est fourni, ex. instants IPFM du modèle autonome) ; rand() → u ∈ [0, 1).
    Renvoie dict(atrial=[(t, organisée)], ventricular=[dict(t, origin, t_a, pr, wide)]).
    """
    ah0, erp = av_constants(p, v_eff)
    k = p['k']
    standstill = k >= p['k_atrial_standstill']
    hp_p = p['hp_conduction'] * (1.0 if k < p['k_hp_block'] else max(0.0, 1.0 - p['hp_k_slope'] * (k - p['k_hp_block'])))
    vpc_rate = p['vpc_rate'] + p['vpc_k_gain'] * max(0.0, p['k_vpc'] - k)            # /min ; hypokaliémie

    vrp = p['qt']                                                                     # réfractarité ventriculaire
    kesc = max(0.0, 1.0 - p['esc_k_gain'] * max(0.0, k - 5.5))                         # automatisme d'échappement
    junct_cycle = 60.0 / (p['junct_rate'] * kesc) if p['junct_rate'] * kesc > 0 else math.inf
    vesc_cycle = 60.0 / (p['vent_escape_rate'] * kesc) if p['vent_escape_rate'] * kesc > 0 else math.inf
    # le foyer jonctionnel est au-dessus d'un bloc infranodal complet : il ne gagne pas les ventricules
    junct_ok = math.isfinite(junct_cycle) and hp_p > 0.0

    # --- impulsions supraventriculaires arrivant au nœud AV : (t, organisée pour l'oreillette)
    supra = []
    atrial = []
    if p['af'] >= 0.5:
        t = rand() * p['af_cycle']
        while t < t_end:
            atrial.append((t, False))
            supra.append(t)
            t += max(0.02, p['af_cycle'] * (1.0 + p['af_cv'] * SQRT3 * (2.0 * rand() - 1.0)))
    else:
        times = sinus_times if sinus_times is not None else [i * sinus_period for i in range(int(t_end / sinus_period) + 1)]
        for t in times:
            if t > t_end:
                break
            if p['sa_block'] > 0.0 and rand() < p['sa_block']:
                continue                                   # bloc de sortie : pas d'activation
            if not standstill:
                atrial.append((t, True))
            supra.append(t)                                # sino-ventriculaire si arrêt atrial
    # --- décharges ectopiques (Poisson)
    ect = []
    if vpc_rate > 0.0:
        lam = vpc_rate / 60.0
        t = -math.log(1.0 - rand()) / lam
        while t < t_end:
            ect.append(t)
            t += -math.log(1.0 - rand()) / lam

    # --- propagation chronologique
    vent = []
    last_v = -math.inf          # dernière activation ventriculaire (toute origine)
    # suppression par entraînement : fréquence ventriculaire moyennée sur esc_sup_tau (/s) ; un
    # patient qui conduit est supposé entraîné depuis longtemps à sa fréquence sinusale
    f_avg = 1.0 / sinus_period if (hp_p > 0.0 and p['sa_block'] < 1.0) else 0.0

    def suppression(cycle):
        return 1.0 + p['esc_sup_gain'] * max(0.0, cycle * f_avg - 1.0)

    def beat(t):
        nonlocal f_avg
        if math.isfinite(last_v):
            rr = t - last_v
            f_avg += (1.0 - math.exp(-rr / p['esc_sup_tau'])) * (1.0 / rr - f_avg)
    last_his = -math.inf        # fin de la dernière activation nodale (origine de la récupération)
    i_s, i_e = 0, 0
    pending = None              # influx conduit en cours de traversée : (t_arrivée_V, t_a, pr)
    while True:
        # prochain événement candidat
        cand = []
        if i_s < len(supra):
            cand.append((supra[i_s], 0))
        if i_e < len(ect):
            cand.append((ect[i_e], 1))
        if pending is not None:
            cand.append((pending[0], 2))
        base = last_v if math.isfinite(last_v) else 0.0
        if junct_ok:
            cand.append((base + junct_cycle * suppression(junct_cycle), 3))
        if math.isfinite(vesc_cycle):
            cand.append((base + vesc_cycle * suppression(vesc_cycle), 4))
        if not cand:
            break                                          # asystolie : plus aucun événement
        t, kind = min(cand)
        if t > t_end:
            break
        if kind == 0:                                      # influx supraventriculaire au nœud AV
            i_s += 1
            rec = t - last_his
            if rec < erp or pending is not None:
                # nœud réfractaire : bloqué. Conduction cachée : un influx arrivé en réfractarité
                # relative (seconde moitié de l'ERP) pénètre partiellement le nœud sans le traverser
                # et recule sa récupération d'une fraction av_conceal du temps écoulé.
                if pending is None and rec >= 0.5 * erp:
                    last_his += p['av_conceal'] * rec
                continue
            ah = ah0 + p['av_beta'] * math.exp(-(rec - erp) / p['av_tau'])
            last_his = t + ah                              # récupération nodale après la traversée
            if rand() >= hp_p:
                continue                                   # bloc infranodal
            pending = (t + ah + p['hv'], t, ah + p['hv'])
        elif kind == 2:                                    # arrivée ventriculaire de l'influx conduit
            tv, ta, pr = pending
            pending = None
            if tv - last_v < vrp:
                continue                                   # ventricule réfractaire
            vent.append(dict(t=tv, origin=ORG_SINUS, t_a=ta, pr=pr, wide=False))
            beat(tv)
            last_v = tv
        elif kind == 1:                                    # foyer ectopique
            i_e += 1
            if t - last_v < vrp:
                continue
            vent.append(dict(t=t, origin=ORG_VPC, t_a=None, pr=None, wide=True))
            beat(t)
            last_v = t
            last_his = t                                   # conduction rétrograde : nœud AV réfractaire
        elif kind == 3:                                    # échappement jonctionnel (QRS fin)
            vent.append(dict(t=t, origin=ORG_JUNCT, t_a=None, pr=None, wide=False))
            beat(t)
            last_v = t
            last_his = t
        else:                                              # échappement ventriculaire (QRS large)
            vent.append(dict(t=t, origin=ORG_VESC, t_a=None, pr=None, wide=True))
            beat(t)
            last_v = t
    return dict(atrial=atrial, ventricular=vent)


def summarize(ev, t0, t1):
    """Indicateurs sur [t0, t1] : FC ventriculaire, FC atriale, CV des RR, part d'origines."""
    v = [b for b in ev['ventricular'] if t0 <= b['t'] <= t1]
    a = [x for x in ev['atrial'] if t0 <= x[0] <= t1]
    rr = [v[i + 1]['t'] - v[i]['t'] for i in range(len(v) - 1)]
    mean_rr = sum(rr) / len(rr) if rr else math.nan
    sd = math.sqrt(sum((x - mean_rr) ** 2 for x in rr) / len(rr)) if rr else math.nan
    org = {}
    for b in v:
        org[b['origin']] = org.get(b['origin'], 0) + 1
    return dict(hr_v=60.0 / mean_rr if rr else 0.0, hr_a=len(a) / (t1 - t0) * 60.0,
                cv_rr=sd / mean_rr if rr else math.nan, max_rr=max(rr) if rr else math.nan, n=len(v), origins=org)
