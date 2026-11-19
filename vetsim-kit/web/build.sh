#!/bin/sh
# Construit l'application web VetTwins : moteur WebAssembly + liaisons JS + paquet de modules.
#   sh web/build.sh            (depuis vetsim-kit/)
# Puis servir le dossier web/ en HTTP (le WebAssembly ne se charge pas depuis file://) :
#   python3 -m http.server -d web 8000   → http://localhost:8000
set -e
cd "$(dirname "$0")/.."
cargo build --release -p vetsim-wasm --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/release/vetsim_wasm.wasm
python3 - <<'PY'
import json, glob, os
m = lambda p: json.load(open(p, encoding='utf-8'))
bundle = {
    'patient': m('modules/healthy_dog.json'),
    'auscultation': m('modules/auscultation.json'),
    'variation': m('modules/variation.json'),
    'diseases': [m(p) for p in sorted(glob.glob('modules/diseases/*.json'))],
    'drugs': [m(p) for p in sorted(glob.glob('modules/drugs/*.json'))],
    'anesthesia': {
        'model': m('modules/anesthesia/anesthesia_model.json'),
        'agents': [m(p) for p in sorted(glob.glob('modules/anesthesia/agents/*.json'))],
        'inhalants': [m(p) for p in sorted(glob.glob('modules/anesthesia/inhalants/*.json'))],
        'procedures': [m(p) for p in sorted(glob.glob('modules/anesthesia/procedures/*.json'))],
    },
}
json.dump(bundle, open('web/modules.json', 'w', encoding='utf-8'), ensure_ascii=False)
print('web/modules.json :', os.path.getsize('web/modules.json'), 'octets')
PY
# Données d'autotest : sorties du binaire natif sur les cas eval du golden (selftest.html)
cargo build --release -p vetsim-cli
python3 - <<'PY'
import json, subprocess
G = json.load(open('tests/golden/cases.json'))
out = []
for c in G['cases']:
    a = c['args']
    if a[0] != 'eval':
        continue
    opt = lambda f, d: a[a.index(f) + 1] if f in a else d
    diseases = [[x.split(':')[0], float(x.split(':')[1])] for i, x in enumerate(a) if i > 0 and a[i - 1] == '--disease']
    conc = {x.split('=')[0]: float(x.split('=')[1]) for i, x in enumerate(a) if i > 0 and a[i - 1] == '--conc'}
    native = json.loads(subprocess.check_output(['target/release/vetsim-cli'] + a, text=True))
    out.append(dict(id=c['id'], bw=float(opt('--bw', 20)), diseases=diseases, conc=conc,
                    D=float(opt('--D', 0)), U=float(opt('--U', 0)), expected=native))
json.dump(out, open('web/selftest.json', 'w'))
print('web/selftest.json :', len(out), 'cas')
PY
ls -la web/pkg
