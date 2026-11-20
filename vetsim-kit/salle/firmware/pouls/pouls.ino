// Pouls fémoral (salle de simulation VetTwins) : boîtier de SORTIE.
// Un petit vibreur (moteur à masselotte ERM, ou mini-solénoïde) sous la peau de la face interne
// de la cuisse du mannequin bat à la FC du patient virtuel ; la force suit la pression pulsée
// (PAS - PAD). Pas de battement si le patient est décédé, si la régie est absente ou si l'état
// a plus de 5 s. La lecture du relais (GET /api/pulse, chaque seconde) tourne dans une tâche à
// part pour ne pas décaler les battements.
// PP_FULL et MOTOR_START sont des réglages d'usage du boîtier (sensation au toucher, démarrage
// du moteur), pas des seuils cliniques : aucun seuil de « pouls non palpable » n'est sourcé dans
// le projet. Câblage : docs/INTEGRATION_MATERIEL.tex, § Pouls. Usage pédagogique uniquement.
#include <VetTwinRelay.h>
#include <esp_arduino_version.h>

const int PIN_MOTOR = 25;     // grille du MOSFET (IRLZ44N, AO3400...) ; diode de roue libre !
const int PIN_LED = 2;        // LED témoin : clignote avec le pouls
const float PP_FULL = 60.0;   // pression pulsée (mmHg) donnant la force maximale (réglage d'usage)
const int MOTOR_START = 90;   // rapport cyclique minimal qui fait tourner le moteur (sur 255)
const int BEAT_MS = 110;      // durée d'une impulsion (ms)

volatile float gHr = 0, gPp = 0;
volatile bool gOn = false;

void pwm(int duty) {
#if ESP_ARDUINO_VERSION_MAJOR >= 3
  ledcWrite(PIN_MOTOR, duty);
#else
  ledcWrite(0, duty);
#endif
}

void pollTask(void*) {
  for (;;) {
    vettwin::Pulse p;
    bool ok = vettwin::pulse(p);
    gOn = ok && p.valid && !p.dead && p.hr > 0;
    gHr = p.hr;
    gPp = max(0.0f, p.sap - p.dap);
    vTaskDelay(pdMS_TO_TICKS(1000));
  }
}

void setup() {
  Serial.begin(115200);
  pinMode(PIN_LED, OUTPUT);
#if ESP_ARDUINO_VERSION_MAJOR >= 3
  ledcAttach(PIN_MOTOR, 5000, 8);
#else
  ledcSetup(0, 5000, 8);
  ledcAttachPin(PIN_MOTOR, 0);
#endif
  pwm(0);
  vettwin::begin("pouls");
  xTaskCreatePinnedToCore(pollTask, "poll", 6144, nullptr, 1, nullptr, 0);
}

void loop() {
  static uint32_t next = 0;
  uint32_t now = millis();
  if (!gOn) {
    pwm(0);
    digitalWrite(PIN_LED, LOW);
    next = now;
    delay(20);
    return;
  }
  if ((int32_t)(now - next) >= 0) {
    float hr = gHr, pp = gPp;
    int duty = (int)(255.0f * min(1.0f, pp / PP_FULL));
    if (duty < MOTOR_START) duty = 0;  // trop faible pour faire tourner ce moteur
    pwm(duty);
    digitalWrite(PIN_LED, HIGH);
    delay(BEAT_MS);
    pwm(0);
    digitalWrite(PIN_LED, LOW);
    next = now + (uint32_t)(60000.0f / max(hr, 1.0f));
  }
  delay(2);
}
