# VetTwins (application web)

Patient virtuel canin dans le navigateur. Le moteur vetsim (Rust) est compilé en WebAssembly : tout le calcul se fait sur la machine de l'utilisateur, sans serveur de calcul, et aucune donnée n'est envoyée. **Usage pédagogique uniquement.**

## Construire et lancer
```
sh web/build.sh                          # depuis vetsim-kit/ : moteur WebAssembly, liaisons JS, modules, autotest
python3 -m http.server -d web 8000       # puis http://localhost:8000
```
Prérequis : Rust avec la cible `wasm32-unknown-unknown` et `cargo install wasm-bindgen-cli --version 0.2.100`. Le dossier `web/` construit est statique : il peut être déposé tel quel sur n'importe quel hébergement de fichiers (GitHub Pages, serveur de l'école, ressource Moodle « dossier »).

## Contenu
- `index.html`, `styles.css`, `app.js` : interface (onglets Patient, Respiration, Schéma du modèle, Auscultation, Traitement PK/PD, Anesthésie), en JavaScript sans dépendance ni outil de construction.
- `anesthesia.js` : onglet Anesthésie (session en temps réel : moniteur, actions, signes cliniques, feuille d'anesthésie ; `docs/ANESTHESIE.tex`).
- `engine.js` : pilote du moteur, équivalent du contrôleur Kotlin de `kmp-demo`.
- `charts.js` : tracés sur canvas (courbes, ECG, spectrogramme, curseur, zoom).
- `selftest.html` : recalcule dans le navigateur les cas `eval` du golden et les compare au bit près au binaire natif.
- Générés par `build.sh`, non versionnés : `pkg/` (moteur), `modules.json`, `selftest.json`.

## Cas partageables par l'adresse
Les paramètres après `#` décrivent un cas, ce qui permet de distribuer un lien aux étudiants :
`bw`, `disease`, `stage`, `tab` (`patient`, `respiration`, `model`, `auscultation`, `treatment`), `conc=drogue:mg/L,...`, `plan=drogue:mg/kg:voie:intervalle_h:nb_prises[:début_h],...`, `days`, `t` (instant d'examen, h), `sim=1` (simulation immédiate), `window` (zoom de l'auscultation, s).

Exemple : `index.html#disease=mmvd&stage=C&tab=treatment&plan=furosemide:2:po:12:14,pimobendan:0.25:po:12:14&days=7&sim=1`

Vérifié (17/11/2026) : Chrome sans interface, 882/882 valeurs identiques au bit près au binaire natif (`selftest.html`) ; captures des cinq onglets relues.
