"""Modules maladie : chaque module tire (probabilistiquement) une sévérité puis des
modificateurs de PARAMÈTRES du patient sain. Les signes cliniques/biologiques ne sont
pas codés ici : ils émergent du moteur (couplages hémodynamique -> rein -> lactate -> gaz...).

Ajouter une maladie = écrire une classe avec sample() qui renvoie une liste de modificateurs.
"""
from dataclasses import dataclass, field
import numpy as np
from params import copy_params


@dataclass
class Case:
    name: str
    severity: float
    mods: list                       # [(clé, 'mul'|'add', valeur)]
    meta: dict = field(default_factory=dict)


def apply(p, case):
    q = copy_params(p)
    for key, op, val in case.mods:
        q[key] = q[key] * val if op == 'mul' else q[key] + val
    return q


def _lnz(rng, sd=0.25):
    return float(rng.lognormal(-0.5 * sd ** 2, sd))     # moyenne 1


class Disease:
    name = 'base'
    stages = {}
    stage_probs = {}

    def draw_severity(self, rng, severity=None, stage=None):
        if severity is not None:
            return float(severity), self._stage_of(severity)
        if stage is None:
            names = list(self.stage_probs)
            stage = names[rng.choice(len(names), p=[self.stage_probs[n] for n in names])]
        lo, hi = self.stages[stage]
        return float(rng.uniform(lo, hi)), stage

    def _stage_of(self, s):
        for n, (lo, hi) in self.stages.items():
            if lo <= s <= hi:
                return n
        return None

    def sample(self, rng, severity=None, stage=None):
        raise NotImplementedError


# coefficients du module MMVD (calés par fit_mmvd.py sur : fraction de régurgitation par stade, POG, PAM ; voir rapport)
KM = dict(g=6.07, gexp=2.617, cpv=0.116, v0=1.115, b=0.2, ret=0.149, ret0=0.05, aut=0.50, zsd=0.25)


class MMVD(Disease):
    """Maladie valvulaire mitrale dégénérative (stades ACVIM B1, B2, C, D)."""
    name = 'MMVD'
    stages = {'B1': (0.05, 0.30), 'B2': (0.30, 0.60), 'C': (0.60, 0.85), 'D': (0.85, 1.0)}
    stage_probs = {'B1': 0.35, 'B2': 0.30, 'C': 0.25, 'D': 0.10}

    def sample(self, rng, severity=None, stage=None):
        s, st = self.draw_severity(rng, severity, stage)
        z = lambda: _lnz(rng, 0.25 * KM['zsd'] / 0.25) if KM['zsd'] > 0 else 1.0
        late = max(0.0, (s - 0.8) / 0.2)
        mods = [('g_mr', 'add', KM['g'] * s ** KM['gexp'] * z()),
                ('c_pv', 'mul', 1 + KM['cpv'] * s * z()),
                ('v0_lv', 'mul', 1 + KM['v0'] * s),
                ('b_lv', 'mul', 1 - KM['b'] * s),
                ('ees_lv', 'mul', 1 - 0.35 * late),
                ('bvf', 'mul', 1 + KM['ret0'] * s + KM['ret'] * max(0.0, (s - 0.55) / 0.45) * z()),   # rétention hydrosodée tardive
                # déséquilibre autonome de l'insuffisance cardiaque : sympathique ↑, vagal ↓, baroréflexe ↓
                ('s0', 'add', KM['aut'] * max(0.0, (s - 0.4) / 0.6)),
                ('v0', 'add', -KM['aut'] * max(0.0, (s - 0.4) / 0.6)),
                ('baro_sens', 'mul', 1 - 0.5 * late)]
        return Case(self.name, s, mods, dict(stage=st))


class CKD(Disease):
    """Maladie rénale chronique (stades IRIS 1-4 approximatifs)."""
    name = 'CKD'
    stages = {'IRIS1': (0.05, 0.25), 'IRIS2': (0.25, 0.50), 'IRIS3': (0.50, 0.75), 'IRIS4': (0.75, 0.95)}
    stage_probs = {'IRIS1': 0.2, 'IRIS2': 0.35, 'IRIS3': 0.3, 'IRIS4': 0.15}

    def sample(self, rng, severity=None, stage=None):
        s, st = self.draw_severity(rng, severity, stage)
        z = lambda: _lnz(rng)
        hyper = rng.random() < (0.2 + 0.6 * s)
        mods = [('nephron_mass', 'mul', max(0.05, float(np.exp(-2.4 * s * z())))),      # calé sur les bornes de créatininémie IRIS
                ('xa', 'add', 6.0 * s ** 2 * z()),
                ('phos', 'add', 1.6 * s ** 2 * z()),
                ('hb', 'mul', 1 - 0.35 * s ** 1.5 * z()),
                ('alb', 'mul', 1 - 0.10 * s),
                ('k', 'add', 2.0 * max(0.0, (s - 0.85) / 0.15))]
        if hyper:
            mods.append(('r_sys', 'mul', 1 + 0.30 * s * z()))
        return Case(self.name, s, mods, dict(stage=st, hypertension=bool(hyper)))


class Hypoadrenocorticism(Disease):
    """Hypoadrénocorticisme (Addison), de la forme discrète à la crise addisonienne."""
    name = 'Addison'
    stages = {'discret': (0.1, 0.4), 'modere': (0.4, 0.7), 'crise': (0.7, 1.0)}
    stage_probs = {'discret': 0.3, 'modere': 0.4, 'crise': 0.3}

    def sample(self, rng, severity=None, stage=None):
        s, st = self.draw_severity(rng, severity, stage)
        z = lambda: _lnz(rng, 0.2)
        zc = z()                                          # Na et Cl varient ensemble (même mécanisme)
        mods = [('na', 'add', -22.0 * s * zc),
                ('cl', 'add', -17.0 * s * zc),
                ('k', 'add', 3.4 * s * z()),
                ('bvf', 'mul', 1 - 0.22 * s * z()),
                ('r_sys', 'mul', 1 - 0.25 * s),
                ('glucose', 'add', -2.0 * s * (rng.random() < 0.5))]
        return Case(self.name, s, mods, dict(stage=st))


class Pneumonia(Disease):
    """Pneumonie : altération des échanges (gradient alvéolo-artériel), léger hypermétabolisme."""
    name = 'Pneumonie'
    stages = {'legere': (0.1, 0.4), 'moderee': (0.4, 0.7), 'severe': (0.7, 1.0)}
    stage_probs = {'legere': 0.35, 'moderee': 0.4, 'severe': 0.25}

    def sample(self, rng, severity=None, stage=None):
        s, st = self.draw_severity(rng, severity, stage)
        z = lambda: _lnz(rng)
        mods = [('aagrad', 'add', 70.0 * s ** 1.3 * z()),
                ('vco2', 'mul', 1 + 0.10 * s),
                ('vo2', 'mul', 1 + 0.10 * s),
                ('lact0', 'add', 0.6 * s)]
        return Case(self.name, s, mods, dict(stage=st))


REGISTRY = {c.name: c for c in (MMVD(), CKD(), Hypoadrenocorticism(), Pneumonia())}
