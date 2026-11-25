# VetTwins (application web)

Patient virtuel canin dans le navigateur. Le moteur vetsim (Rust) est compilé en WebAssembly : tout le calcul se fait localement, dans le navigateur, sans serveur de calcul, et aucune donnée n'est envoyée. **Usage pédagogique uniquement.**

## Construire et lancer
```
sh web/build.sh                          # depuis vetsim-kit/ : moteur WebAssembly, liaisons JS, modules, autotest
python3 -m http.server -d web 8000       # puis http://localhost:8000
```
Prérequis : Rust avec la cible `wasm32-unknown-unknown` et `cargo install wasm-bindgen-cli --version 0.2.100`. Le dossier `web/` construit est statique : il peut être déposé tel quel sur n'importe quel hébergement de fichiers (GitHub Pages, serveur de l'école, ressource Moodle « dossier »).

## Contenu
- `index.html`, `styles.css`, `app.js` : interface (onglets Patient, Respiration, Schéma du modèle, Auscultation, Cœur animé, Traitement PK/PD, Anesthésie), en JavaScript sans dépendance ni outil de construction.
- `anesthesia.js` : onglet Anesthésie (session en temps réel : moniteur, stéthoscope synchronisé sur l'ECG (apex ou base), actions, signes cliniques, feuille d'anesthésie ; `docs/ANESTHESIE.tex`).
- `engine.js` : pilote du moteur, équivalent du contrôleur Kotlin de `kmp-demo`. Chaque calcul crée un patient temporaire, libéré ensuite (`drop_patient`).
- `engine-worker.js` : calculs longs hors du fil de l'interface. La cinétique sous traitement (≈ 12 s pour 3 jours en MMVD) y tourne : l'interface reste utilisable, les anciens résultats restent affichés grisés, et un nouveau changement annule le calcul en cours.
- `mc-worker.js` : Monte-Carlo (variabilité), également dans un Web Worker.
- `heart.js` : cœur animé, entièrement piloté par le moteur (API `heart`).
  - Mécanique : contraction des quatre cavités (volumes) et parois épaissies selon l'activation mécanique ; les valves s'ouvrent ou fuient selon les débits du modèle.
  - Électrique : dépolarisation du myocarde pendant les ondes P, QRS et T de l'ECG, influx dans le tissu de conduction, et devenir de chaque influx au nœud AV.
  - Interface : ruban des phases du cœur gauche, tracés synchronisés, clavier (espace pour la lecture, ← → pour changer de battement).
- `echo.js` : échocardiographie virtuelle (prototype, `docs/ECHO.tex`). Géométrie des cavités tirée des volumes du modèle (API `echo` : VG à volume myocardique constant, OG et OD observées, VD en croissant, valves suivant les débits) ; image formée par une physique simplifiée (speckle attaché aux tissus, interfaces, atténuation, ombres costales, lignes A, faisceau focalisé, TGC) ; mode TM, Doppler couleur, pulsé et continu (vitesses tirées des débits et des pressions du modèle, jets turbulents, convergence, repliement, angle), calipers comparés aux mesures du modèle, coupes de référence, sonde pilotable par le gyroscope d'un téléphone.
- `charts.js` : tracés sur canvas (courbes, ECG, spectrogramme, curseur, zoom).
- `selftest.html` : recalcule dans le navigateur les cas `eval` du golden et les compare au bit près au binaire natif.
- Générés par `build.sh`, non versionnés : `pkg/` (moteur), `modules.json`, `selftest.json`.

## Publication (GitHub Pages)
Le workflow `.github/workflows/pages.yml` publie le site à chaque fusion dans `main` : il reconstruit le moteur, lance l'autotest bit à bit dans Chrome, puis déploie. Il faut régler une fois Settings → Pages → Source sur « GitHub Actions ». Il est aussi lançable à la main (onglet Actions, « Pages », Run workflow). Adresse : https://vinlmoe.github.io/VetTwin/

## Ergonomie
- L'adresse suit le cas affiché (patient, onglet, schéma de traitement) : un rechargement le restitue, et le bouton « Copier le lien du cas » la copie pour la distribuer.
- Clavier : Entrée dans le formulaire du patient = Évaluer ; flèches gauche/droite, Début, Fin sur les onglets ; espace = écouter/arrêter (Auscultation) ou lecture/pause (Cœur animé) ; blocs du schéma du modèle atteignables par Tab, ouverts par Entrée.
- Barre latérale : changer de maladie, de stade ou de poids évalue aussitôt le patient.
- Traitement :
  - Entrée ajoute le schéma, qui est simulé aussitôt ; le formulaire garde ses valeurs.
  - Chaque schéma se modifie sur place (dose ±10 %, voie, intervalle, nombre de prises, début).
  - Cliquer ou glisser sur une courbe déplace l'instant d'examen ; les constantes à cet instant s'affichent en direct, sans examen.
  - Une fois l'examen complet demandé, il suit l'instant choisi (curseur, creux, pic) sans quitter l'onglet.
- Anesthésie : le calcul de chaque seconde réelle est borné ; un patient coûteux (MMVD) avance moins vite que la vitesse demandée au lieu de geler l'interface, et la vitesse effective est affichée.

## Cas partageables par l'adresse
Les paramètres après `#` décrivent un cas, ce qui permet de distribuer un lien aux étudiants :
`bw`, `disease`, `stage`, `tab` (`patient`, `respiration`, `model`, `auscultation`, `heart`, `echo`, `treatment`, `anesthesia`), `conc=drogue:mg/L,...`, `plan=drogue:mg/kg:voie:intervalle_h:nb_prises[:début_h],...`, `days`, `t` (instant d'examen, h), `sim=1` (simulation immédiate), `window` (zoom de l'auscultation, s), `speed` (vitesse du cœur animé, ex. 0.25), `ht` (instant figé du cœur animé, s).

Exemple : `index.html#disease=mmvd&stage=C&tab=treatment&plan=furosemide:2:po:12:14,pimobendan:0.25:po:12:14&days=7&sim=1`

Vérifié (17/11/2026) : Chrome sans interface, 882/882 valeurs identiques au bit près au binaire natif (`selftest.html`) ; captures des cinq onglets relues.
