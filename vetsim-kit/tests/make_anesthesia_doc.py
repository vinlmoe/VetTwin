"""Tableau de provenance du module d'anesthésie (docs/anesthesie_provenance.tex), tiré des modules JSON.
python tests/make_anesthesia_doc.py"""
import json, glob

def tex(s):
    for a, b in [('\\', '\\textbackslash{}'), ('&', '\\&'), ('%', '\\%'), ('_', '\\_'), ('#', '\\#'), ('$', '\\$'), ('~', '\\textasciitilde{}'), ('^', '\\textasciicircum{}')]:
        s = s.replace(a, b)
    return s.replace('µ', '\\textmu{}').replace('⁻¹', '$^{-1}$').replace('≈', '$\\approx$').replace('→', '$\\to$').replace('≥', '$\\ge$').replace('≤', '$\\le$').replace('α', '$\\alpha$').replace('γ', '$\\gamma$').replace('×', '$\\times$').replace('−', '$-$').replace('Δ', '$\\Delta$').replace('β', '$\\beta$').replace('½', '$\\frac12$').replace('±', '$\\pm$').replace('·', '$\\cdot$').replace('¹', '$^1$').replace('§', '\\S{}').replace('–', '--').replace('ν', '$\\nu$').replace('₂', '$_2$')

ev = lambda e: {'C': '\\evC', 'D': '\\evD'}.get(e, e)
out = ['% Généré par tests/make_anesthesia_doc.py — ne pas éditer à la main.',
       '\\subsection{Agents IV, IM et locorégionaux}',
       '\\begin{tabularx}{\\linewidth}{P{2.4cm}P{0.6cm}L}', '\\toprule', 'Agent & Niv. & Provenance (bloc \\vp{anesthesia}) et notes \\\\', '\\midrule']
for p in sorted(glob.glob('modules/anesthesia/agents/*.json')):
    m = json.load(open(p))
    a = m['anesthesia']
    out.append(f"{tex(m['name']['fr'])} & {ev(a['evidence'])} & {tex(a.get('source', ''))} {tex(m.get('notes', ''))} \\\\")
out += ['\\bottomrule', '\\end{tabularx}', '', '\\subsection{Halogénés}',
        '\\begin{tabularx}{\\linewidth}{P{2.4cm}P{0.6cm}L}', '\\toprule', 'Agent & Niv. & Valeurs et provenance \\\\', '\\midrule']
for p in sorted(glob.glob('modules/anesthesia/inhalants/*.json')):
    m = json.load(open(p))
    t = m['tissue_blood']
    out.append(f"{tex(m['name']['fr'])} & {ev(m['evidence'])} & MAC {m['mac_pct']} \\%, sang/gaz {m['blood_gas']}, tissu/sang {t['vrg']} / {t['muscle']} / {t['fat']}. {tex(m['notes'])} \\\\")
out += ['\\bottomrule', '\\end{tabularx}', '', '\\subsection{Constantes du modèle}',
        '\\begin{tabularx}{\\linewidth}{P{3.6cm}P{1.2cm}P{0.6cm}L}', '\\toprule', 'Constante & Valeur & Niv. & Source \\\\', '\\midrule']
m = json.load(open('modules/anesthesia/anesthesia_model.json'))
for k, v in m['values'].items():
    out.append(f"\\vp{{{k}}} & {v['value']} & {ev(v['evidence'])} & {tex(v['source'])} \\\\")
out += ['\\bottomrule', '\\end{tabularx}', '', '\\subsection{Scénarios chirurgicaux}']
for p in sorted(glob.glob('modules/anesthesia/procedures/*.json')):
    pr = json.load(open(p))
    steps = ' ; '.join(f"{tex(s['label']['fr'])} ({s['intensity']}, {s['duration_s']:.0f} s)" for s in pr['steps'])
    out.append(f"\\textbf{{{tex(pr['name']['fr'])}}} ({ev(pr['evidence'])}) : {steps}. Douleur postopératoire {pr['postop']['intensity']}, constante {pr['postop']['tau_h']} h. {tex(pr['notes'])}\n")
open('docs/anesthesie_provenance.tex', 'w').write('\n'.join(out) + '\n')
print('docs/anesthesie_provenance.tex')
