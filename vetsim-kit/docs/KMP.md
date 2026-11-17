# API par messages JSON pour Kotlin Multiplatform

Principe : peu de fonctions, des chaînes JSON en entrée et en sortie. Les structures évoluent dans le JSON sans casser la liaison. Pas d'état global : le moteur est un objet.

```
interface Engine {
  fun apiVersion(): String                                  // "1.1.0"
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
- **Issue (API 1.1.0)** : `state` renvoie `outcome` = `{"state": "stable"|"decompensated"|"dead", "mechanisms": [{id, label_fr, value, threshold, factor, evidence}], "f_contract", "f_rsys", "f_vent", "iterations"}` ; en cas de décès `{"state": "dead", "cause", "label_fr"}`, les variables plates étant celles de la dernière passe viable (absentes s'il n'y en a pas), `emergent_signs` vide et `signals` à `null`. `timecourse` ajoute à chaque entrée de `states_at`/`states` les clés `outcome`, `mechanisms` (identifiants) et `cause`, et renvoie `death_t_h` (premier instant de décès, ou `null`). `pcg` et `signals` renvoient une erreur pour un patient décédé. Seuils et pentes : placeholders D.
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
- `timecourse` : `{"doses": [...], "t_end_h", "t_eval_h", "states", "state_times_h"}` ; renvoie `t`, `conc_mg_L`, `effect` (effet Emax E(t) par médicament), `D`, `U` ; `states` : état plat à chaque instant de `t_eval_h` ; `state_times_h` (≤ 400 instants) : `state_t` et `states_at`, l'état aux instants choisis (moins coûteux pour un traitement de plusieurs jours).
- Examen sous traitement : `state`, `signals` et `pcg` acceptent `{"doses": [...], "at_h": t}` à la place de `conc`/`D`/`U`. Le moteur simule la PK du patient jusqu'à `t`, avec sa fonction rénale et hépatique, puis applique les concentrations et l'état de volume à cet instant.
- `pcg(patientId, options)` (module auscultation, `docs/AUSCULTATION.md`) : options `site` (`base` ou `apex`), `hr` (FC de repos imposée, réglage d'usage), `fs`, `duration_s`, `seed`, `conc`/`D`/`U`, `with_ecg`, `with_pressures`. Renvoie `wav_base64` (WAV PCM 16 bits mono), `fs`, `grade`, `murmur`, `s3`, plus `s1`, `s2`, `beats`, `t_mc`, `t_ac`, `hr`. Avec `with_ecg`, l'ECG a les mêmes battements que le son. Avec `with_pressures`, les pressions aorte, VG et OG à 250 Hz sont alignées sur le son : le battement simulé est posé à chaque instant de battement, sans être recalculé battement par battement. `loadModules` accepte la clé optionnelle `auscultation` (contenu de `modules/auscultation.json`).
- `message(json)` : `{"op": "pcg"|"state"|"signals"|"timecourse"|"add_disease"|"new_patient"|"list_modules"|"api_version", "patient": id, "options": {...}}`. Même réponse que la méthode correspondante. `hr` (FC imposée) est accepté aussi par `state` et `signals`.

### Diurétique : définition de `D` et `U`
- Débit urinaire induit : `u(t) = urine_max · E(C(t))` (mL/kg/min, `urine_max` = 0,25). C'est le supplément dû au médicament ; la diurèse de base n'est pas modélisée.
- `U` (mL/kg), « diurèse effective cumulée » : `dU/dt = 60 u − U/τ_elect`, avec τ_elect = 24 h. Soit `U(t) = ∫ 60 u(s) e^{−(t−s)/24 h} ds` : volume induit cumulé avec oubli exponentiel. Ce n'est pas le volume d'urine émis, mais la variable des pertes non encore récupérées. Elle fixe les électrolytes : K⁺ − 0,04 U, Cl⁻ − 0,5 U, Na⁺ − 0,3 U (mmol/L).
- `D` (mL), déficit de volume : `dD/dt = f_plasma · u · bw · 60 − D/τ_refill`, avec f_plasma = 0,3 et τ_refill = 4 h. C'est la part de l'urine prélevée sur la volémie, nette du retour ; elle est soustraite de `vol_shift`.
- Paramètres PD du module furosémide : placeholders (evidence D).
