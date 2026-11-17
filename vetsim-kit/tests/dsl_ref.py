"""Interpréteur de référence du DSL de formules (voir docs/DSL.md). ~40 lignes : à porter en Rust à l'identique."""
import math

def ev(e, env):
    if isinstance(e, bool):
        return e
    if isinstance(e, (int, float)):
        return float(e)
    if isinstance(e, str):
        return env[e]
    (op, a), = e.items()
    if op == 'if':
        return ev(a[1], env) if ev(a[0], env) else ev(a[2], env)
    if op == 'and':
        return all(ev(x, env) for x in a)
    if op == 'or':
        return any(ev(x, env) for x in a)
    v = [ev(x, env) for x in a] if isinstance(a, list) else [ev(a, env)]
    if op == 'add': return sum(v)
    if op == 'mul':
        r = 1.0
        for x in v: r *= x
        return r
    if op == 'sub': return v[0] - v[1]
    if op == 'div': return v[0] / v[1]
    if op == 'pow': return v[0] ** v[1]
    if op == 'exp': return math.exp(v[0])
    if op == 'log': return math.log(v[0])
    if op == 'neg': return -v[0]
    if op == 'abs': return abs(v[0])
    if op == 'min': return min(v)
    if op == 'max': return max(v)
    if op == 'clip': return min(max(v[0], v[1]), v[2])
    if op == 'lt': return v[0] < v[1]
    if op == 'le': return v[0] <= v[1]
    if op == 'gt': return v[0] > v[1]
    if op == 'ge': return v[0] >= v[1]
    if op == 'not': return not v[0]
    raise ValueError(op)

def disease_mods(mod, severity, draws=None):
    """Applique un module maladie JSON (sans bruit si draws=None : lognormal_mean1 -> 1, bernoulli -> 0). Renvoie [(param, op, valeur)]."""
    env = {'s': severity}
    for name, d in mod.get('draws', {}).items():
        if draws and name in draws:
            env[name] = draws[name]
        else:
            env[name] = 1.0 if d['dist'] == 'lognormal_mean1' else (0.0 if d['dist'] == 'bernoulli' else 0.5)
    for name, ex in mod.get('let', {}).items():
        env[name] = ev(ex, env)
    out = []
    for eff in mod['effects']:
        if 'when' in eff and not ev(eff['when'], env):
            continue
        out.append((eff['param'], eff['op'], ev(eff['expr'], env)))
    return out
