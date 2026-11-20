// Machine d'anesthésie factice (salle de simulation VetTwins).
// - Vaporisateur : potentiomètre rotatif sous le cadran (impression 3D ou vaporisateur réformé) ;
//   le bas de course (cran « 0 ») ferme le vaporisateur.
// - Sélecteur d'agent : interrupteur (ouvert = isoflurane, fermé = sévoflurane).
// - Débitmètre O2 : potentiomètre sous le bouton du débitmètre.
// - Détecteur d'intubation : interrupteur à lame souple (ILS) dans la trachée du mannequin,
//   aimant annulaire sur la sonde d'intubation : sonde en place = "intubate", retirée = "extubate".
// Un changement n'est envoyé qu'après 400 ms de stabilité (on tourne le bouton sans inonder
// le relais). LED verte/rouge : verdict du moteur (ex. intubation refusée, plan trop léger).
// Câblage : docs/INTEGRATION_MATERIEL.tex, § Machine d'anesthésie. Usage pédagogique uniquement.
#include <VetTwinRelay.h>

using vettwin::Debounced;

// Butées des cadrans = vaporizer_max_pct des modules (modules/anesthesia/inhalants/*.json).
struct Agent {
  const char* id;
  float max_pct;
};
const Agent AGENTS[2] = {{"isoflurane", 5.0}, {"sevoflurane", 8.0}};
const float VAP_STEP = 0.25;  // graduation du cadran (%), comme l'onglet Anesthésie
const float FGF_MAX = 4.0;    // pleine échelle du débitmètre (L/min) : réglage d'usage du boîtier
const float FGF_STEP = 0.25;
const float DETENT = 0.03;    // 3 % bas de course = cran fermé

const int PIN_VAP = 34, PIN_FGF = 35;  // entrées analogiques (ADC1, utilisables avec le Wi-Fi)
Debounced agentSw(16), tube(17, 300);
const int LED_OK = 18, LED_ERR = 19;
const uint32_t SETTLE_MS = 400;

struct Knob {
  explicit Knob(int p) : pin(p) {}
  int pin;
  float sent = -1, cand = -1;
  uint32_t since = 0;
};
Knob vap{PIN_VAP}, fgf{PIN_FGF};
int agentSent = -1;
uint32_t lastFail = 0;

float readPos(int pin) {  // 0..1, moyenne de 16 lectures
  uint32_t s = 0;
  for (int i = 0; i < 16; i++) s += analogRead(pin);
  return (s / 16.0f) / 4095.0f;
}

float quant(float v, float step) { return roundf(v / step) * step; }

void verdict(const vettwin::Result& r) {
  digitalWrite(LED_OK, r.ok ? HIGH : LOW);
  digitalWrite(LED_ERR, r.ok ? LOW : HIGH);
  if (r.code != 200) lastFail = millis();
}

/** Valeur stable depuis SETTLE_MS et différente de la dernière envoyée ? */
bool settled(Knob& k, float v) {
  if (v != k.cand) {
    k.cand = v;
    k.since = millis();
  }
  return k.cand != k.sent && millis() - k.since >= SETTLE_MS && millis() - lastFail > 2000;
}

void setup() {
  Serial.begin(115200);
  analogReadResolution(12);
  pinMode(LED_OK, OUTPUT);
  pinMode(LED_ERR, OUTPUT);
  agentSw.begin();
  tube.begin();
  vettwin::begin("machine");
}

void loop() {
  agentSw.update();
  int a = agentSw.active() ? 1 : 0;
  if (tube.update()) verdict(vettwin::send(tube.active() ? "{\"type\":\"intubate\"}" : "{\"type\":\"extubate\"}"));

  float p = readPos(PIN_VAP);
  float pct = p < DETENT ? 0 : quant((p - DETENT) / (1 - DETENT) * AGENTS[a].max_pct, VAP_STEP);
  if (a != agentSent) vap.sent = -1;  // changement d'agent : renvoyer le réglage
  if (settled(vap, pct)) {
    char in[96];
    snprintf(in, sizeof in, "{\"type\":\"vaporizer\",\"agent\":\"%s\",\"pct\":%.2f}", AGENTS[a].id, pct);
    vettwin::Result r = vettwin::send(in);
    verdict(r);
    if (r.code == 200) {
      vap.sent = pct;
      agentSent = a;
    }
  }

  float l = quant(readPos(PIN_FGF) * FGF_MAX, FGF_STEP);
  if (settled(fgf, l)) {
    char in[64];
    snprintf(in, sizeof in, "{\"type\":\"fresh_gas\",\"l_min\":%.2f}", l);
    vettwin::Result r = vettwin::send(in);
    verdict(r);
    if (r.code == 200) fgf.sent = l;
  }
  delay(20);
}
