# API par messages JSON pour Kotlin Multiplatform

Principe : peu de fonctions, des chaînes JSON en entrée et en sortie. Les structures évoluent dans le JSON sans casser la liaison. Pas d'état global : le moteur est un objet.

```
interface Engine {
  fun apiVersion(): String                                  // "1.0.0"
  fun loadModules(json: String): String                     // {"ok":true} ou {"ok":false,"errors":[{path,message}]}
  fun newPatient(optionsJson: String): String               // {"bw":20,"seed":42,"variability":true} -> {"patient_id":"p1"}
  fun addDisease(patientId: String, optionsJson: String): String   // {"id":"mmvd","stage":"C"|"severity":0.7} -> {case}
  fun state(patientId: String, optionsJson: String): String // {"conc":{...},"D":0,"U":0,"signals":false} -> variables + signes
  fun signals(patientId: String, optionsJson: String): String // {"duration_s":8,"fs":500} -> {ecg:[...],pcg:[...],r_times:[...]}
  fun timecourse(patientId: String, dosesJson: String): String // doses + horizon -> série d'états
  fun listModules(): String                                 // catalogue (id, nom, stades, evidence)
}
```
- `state` renvoie les mêmes clés plates que le CLI (`eval`) plus `case` (maladies actives et stades) et `emergent_signs` (liste de signes cliniques dérivés des variables : souffle et grade, dyspnée, hypotension, etc.).
- Les tableaux de signaux sont des `Float32`/`Float64` contigus; si le coût du JSON est trop élevé, exposer une variante binaire (`ByteArray`) pour `signals`.
- Toute entrée invalide renvoie `{"ok":false,"errors":[...]}` : aucune panique ne doit traverser la FFI.

## Liaison
1. Essayer **UniFFI** (génération Kotlin pour Android et JVM; Swift pour iOS).
2. Repli : C-ABI avec `cbindgen`, cinterop pour Kotlin/Native (iOS, desktop natif) et JNI pour Android/JVM.
3. Web : compilation `wasm32-unknown-unknown` et appel depuis Kotlin/JS (optionnel, jalon M7).

## Démo KMP minimale
Un écran : choix du poids, d'une maladie et d'un stade; boutons « évaluer » et « ajouter médicament »; tracés ECG et pressions; tableau des gaz du sang et de la biochimie; lecture du PCG.

## Implémentation (jalon M7)
- Crate `crates/vetsim-ffi` : `api.rs` (Rust pur, testé par `cargo test`) et `lib.rs` (objet UniFFI `Engine`, `catch_unwind` sur chaque appel). Liaisons générées par `cargo run -p uniffi-bindgen -- generate --library <lib> --language kotlin`.
- `loadModules` attend `{"patient": <healthy_dog.json>, "diseases": [<module>...], "drugs": [<module>...]}` ; les chemins d'erreur sont relatifs à ce paquet (ex. `/diseases/1/effects/0/op`).
- `newPatient` : `variability: false` → patient sans bruit (les maladies à sévérité imposée donnent alors exactement les valeurs du CLI `eval`).
- `addDisease` : `stage` ou `severity` (pas les deux) ; sans l'un ni l'autre, stade tiré selon `prior`.
- `state` : clés plates du CLI, `case` (avec `modifiers` : paramètre, opération, valeur, blocs), `emergent_signs`, `params` (paramètres d'entrée après maladies et médicaments) ; `"signals": true` ajoute la réponse de `signals`.
- `listModules` : en plus, `blocks` (blocs du modèle et leurs paramètres, `vetsim_core::blocks`) et, pour chaque module, `effects` (paramètre, opération, conditionnel, blocs).
- `signals` : `ecg` (500 Hz par défaut), `pcg` (4000 Hz), `r_times`, et en plus `ecg_features`, `pcg_features`, `pressures` (dernier battement : `ao`, `lv`, `la`, `pa`, `rv`, pas `dt`), `hrv`, `respiration` (20 Hz, même fenêtre que l'ECG : `volume_mL` au-dessus de la fin d'expiration = profil normalisé du modèle autonome × volume courant alvéolaire VA/FR, sans espace mort ; `flow_mL_s` dérivé ; `hr` et `map` instantanées ; `sap`, `dap` = `map` ± écart PAS − PAM et PAD − PAM du battement simulé, pression pulsée supposée constante). Les tableaux sont arrondis en `Float32` dans le JSON (≈ 300 Ko pour 8 s d'ECG et 4 battements de PCG) ; une variante binaire n'a pas été nécessaire.
- `emergent_signs` : uniquement des seuils déjà présents dans le projet, chacun avec sa source (souffle et grade, B3, POG > 20 mmHg, Na:K < 27, créatinine > 125 µmol/L, K > 5,5 mmol/L). Pas d'hypotension, de tachycardie ni d'hypoxémie : aucun seuil sourcé dans le kit.
- `timecourse` : `{"doses": [...], "t_end_h", "t_eval_h", "states"}` ; renvoie `t`, `conc_mg_L`, `D`, `U` et, si demandé, l'état plat à chaque instant.
