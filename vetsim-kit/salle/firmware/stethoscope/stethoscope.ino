// Boîtier stéthoscope (salle de simulation VetTwins).
// Deux capteurs à effet Hall omnipolaires alimentés en 3,3 V (sortie collecteur ouvert active à
// l'état bas, ex. SS451A ; l'A3144 demande 4,5 V au moins et ne convient pas) sous
// la peau du thorax du mannequin : foyer de la base et foyer de l'apex (côté gauche). Un aimant
// néodyme collé dans le pavillon du stéthoscope déclenche le capteur du foyer où il est posé.
// Envoie le site au relais (entrée "stethoscope") ; le lecteur d'auscultation (ausc.html) joue
// alors les bruits du cœur de ce site dans le haut-parleur du thorax.
// Câblage : docs/INTEGRATION_MATERIEL.tex, § Auscultation. Usage pédagogique uniquement.
#include <VetTwinRelay.h>

using vettwin::Debounced;

Debounced base(32, 150), apex(33, 150);  // anti-rebond long : le pavillon glisse sur la peau
const uint32_t RELEASE_MS = 600;         // délai avant « retiré » (évite les coupures en déplaçant)
const char* sent = "?";
uint32_t lostAt = 0, lastTry = 0;

const char* site() {
  if (apex.active()) return "apex";  // si les deux : l'apex l'emporte (foyer du souffle mitral)
  if (base.active()) return "base";
  return nullptr;
}

void push(const char* s) {
  String in = String("{\"type\":\"stethoscope\",\"site\":") + (s ? String("\"") + s + "\"" : String("null")) + "}";
  if (vettwin::send(in).code == 200) sent = s ? s : "";
}

void setup() {
  Serial.begin(115200);
  base.begin();
  apex.begin();
  vettwin::begin("stethoscope");
  push(site());
}

void loop() {
  base.update();
  apex.update();
  const char* s = site();
  // site voulu : le capteur actif, ou « retiré » après RELEASE_MS sans capteur
  const char* want = s;
  if (s) {
    lostAt = 0;
  } else if (*sent) {
    if (!lostAt) lostAt = millis();
    if (millis() - lostAt <= RELEASE_MS) want = sent;
  }
  const char* w = want ? want : "";
  // nouvel essai au plus une fois par seconde si le relais ou la régie ne répond pas
  if (strcmp(w, sent) != 0 && millis() - lastTry > 1000) {
    lastTry = millis();
    push(want);
  }
  delay(5);
}
