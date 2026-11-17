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
