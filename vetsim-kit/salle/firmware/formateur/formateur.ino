// Boîtier du formateur (salle de simulation VetTwins) : gros boutons à portée de main en salle,
// pour déclencher les temps du scénario sans retourner à la régie.
// Valeurs : celles proposées par défaut dans l'onglet Anesthésie (hémorragie 5 mL/kg, bolus
// 10 mL/kg, pince 30 s) ; ce sont des réglages du scénario, à adapter.
// LED verte / rouge : verdict du moteur (ex. « tous les temps opératoires sont réalisés »).
// Câblage : docs/INTEGRATION_MATERIEL.tex, § Boîtier du formateur. Usage pédagogique uniquement.
#include <VetTwinRelay.h>

using vettwin::Debounced;

struct Button {
  Debounced in;
  const char* input;
  const char* label;
};
Button buttons[] = {
    {Debounced(13), "{\"type\":\"procedure_next\"}", "temps operatoire suivant"},
    {Debounced(14), "{\"type\":\"hemorrhage\",\"ml_kg\":5}", "hemorragie 5 mL/kg"},
    {Debounced(27), "{\"type\":\"stimulus\",\"intensity\":1,\"duration_s\":30}", "stimulus (pince 30 s)"},
    {Debounced(26), "{\"type\":\"fluid_bolus\",\"ml_kg\":10}", "bolus 10 mL/kg"},
    {Debounced(25), "{\"type\":\"end_surgery\"}", "fin de chirurgie"},
};
const int LED_OK = 18, LED_ERR = 19;

void setup() {
  Serial.begin(115200);
  pinMode(LED_OK, OUTPUT);
  pinMode(LED_ERR, OUTPUT);
  for (auto& b : buttons) b.in.begin();
  vettwin::begin("formateur");
}

void loop() {
  for (auto& b : buttons) {
    if (b.in.update() && b.in.active()) {
      Serial.println(b.label);
      vettwin::Result r = vettwin::send(b.input);
      digitalWrite(LED_OK, r.ok ? HIGH : LOW);
      digitalWrite(LED_ERR, r.ok ? LOW : HIGH);
    }
  }
  delay(5);
}
