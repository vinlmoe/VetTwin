// Boîtier des capteurs posés (salle de simulation VetTwins).
// Six contacts : un par capteur du moniteur. Contact fermé = capteur posé sur le mannequin.
// À chaque changement, l'état complet est envoyé au relais (entrée "probes") ; le moniteur de
// salle n'affiche que les voies dont le capteur est posé. Renvoi toutes les 30 s (resynchronise
// une régie redémarrée). Câblage : docs/INTEGRATION_MATERIEL.tex, § Boîtier des capteurs.
// Usage pédagogique uniquement.
#include <VetTwinRelay.h>

using vettwin::Debounced;

// clé du moteur (monitor-view.js, PROBES), broche, libellé
struct Probe {
  const char* key;
  Debounced in;
  const char* label;
};
Probe probes[] = {
    {"ecg", Debounced(13), "ECG"},     {"spo2", Debounced(14), "SpO2"},  {"nibp", Debounced(27), "PNI"},
    {"art", Debounced(26), "PA inv."}, {"capno", Debounced(25), "capno"}, {"temp", Debounced(33), "T°"},
};
const int LED_OK = 2;  // LED de la carte : allumée si le dernier envoi est accepté
uint32_t lastSend = 0;
bool lastOk = false;

String state() {
  String s = "{\"type\":\"probes\"";
  for (auto& p : probes) s += String(",\"") + p.key + "\":" + (p.in.active() ? "true" : "false");
  return s + "}";
}

void push() {
  vettwin::Result r = vettwin::send(state());
  lastOk = r.ok;
  digitalWrite(LED_OK, r.ok ? HIGH : LOW);
  lastSend = millis();
}

void setup() {
  Serial.begin(115200);
  pinMode(LED_OK, OUTPUT);
  for (auto& p : probes) p.in.begin();
  vettwin::begin("capteurs");
  push();
}

void loop() {
  bool changed = false;
  for (auto& p : probes)
    if (p.in.update()) {
      changed = true;
      Serial.printf("%s %s\n", p.label, p.in.active() ? "posé" : "retiré");
    }
  // renvoi toutes les 30 s, ou 2 s après un échec (relais ou régie absents)
  if (changed || millis() - lastSend > (lastOk ? 30000u : 2000u)) push();
  delay(5);
}
