# VetTwins : démo Kotlin Multiplatform du moteur vetsim

Application de démonstration (Android + bureau JVM) : choix du poids, d'une maladie et d'un stade, ajout d'un médicament par sa concentration plasmatique. Trois onglets :
- **Patient** : tracés ECG et pressions (aorte, VG, OG), gaz du sang, hémodynamique, biochimie, signes émergents sourcés, écoute du PCG.
- **Respiration** : volume pulmonaire au-dessus de la fin d'expiration (profil du modèle autonome × volume courant alvéolaire VA/FR ; espace mort non modélisé), débit, FC et PAM instantanées (arythmie sinusale respiratoire, variation respiratoire de la PA), sur la même fenêtre que l'ECG.
- **Schéma du modèle** : blocs du moteur et couplages codés ; blocs touchés par la maladie (rouge) et les médicaments (bleu), d'après les paramètres qui diffèrent du chien sain de même poids ; un clic sur un bloc montre ses paramètres (sain → patient, module source) et ses couplages. Le rattachement paramètre → bloc vient du moteur (`vetsim_core::blocks`).

**Usage pédagogique uniquement : ne jamais utiliser pour décider d'une dose ou d'un traitement réel.**

## Architecture
- `crates/vetsim-ffi` (Rust) : API par messages JSON (`docs/KMP.md`) exposée par UniFFI ; aucune panique ne traverse la FFI.
- `composeApp/src/commonMain` : interface `VetsimEngine`, contrôleur, écran Compose (code commun).
- `composeApp/src/jvmSharedMain` : `FfiEngine`, branché sur les liaisons Kotlin générées par UniFFI (JNA), commun à Android et au bureau.
- `androidMain` / `desktopMain` : point d'entrée et lecture audio du PCG.
- Les modules `../modules/*.json` sont embarqués à la compilation (tâche `modulesBundle`).

Tout le natif est construit par Gradle (tâches `cargoBuildHost`, `cargoBuildAndroid`, `uniffiBindings`) ; rien de généré n'est versionné.

## Prérequis
- Rust (rustup) avec les cibles `aarch64-linux-android` et `x86_64-linux-android`, et `cargo install cargo-ndk`.
- JDK 17 ou plus ; SDK Android (`local.properties` : `sdk.dir=...`, ou `ANDROID_HOME`) avec un NDK (r27 testé).

## Commandes
```
./gradlew :composeApp:run                 # application de bureau
./gradlew :composeApp:desktopTest         # tests JVM via FFI + capture d'écran (build/screenshots/)
./gradlew :composeApp:assembleDebug       # APK Android (arm64-v8a, x86_64)
```

## Vérifications faites (jalon M7)
- `EngineFfiTest` : session complète par la FFI ; sorties de `state` identiques **au bit près** à `vetsim-cli eval` (MMVD 0,8, Addison 0,9, MRC 0,6).
- `ScreenshotTest` : rendu hors écran des trois onglets (MMVD stade C, et + pimobendane pour le schéma) dans `composeApp/build/screenshots/`.
- APK : `lib/arm64-v8a` et `lib/x86_64` contiennent `libvetsim_ffi.so` et `libjnidispatch.so`. Non exécuté sur appareil ni émulateur (aucune image système installée).
