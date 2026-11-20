// Pupitre d'injection RFID (salle de simulation VetTwins).
// Chaque seringue d'entraînement porte une étiquette NFC (NTAG213/215, MIFARE Classic 1K) collée
// sur le corps. L'étudiant présente la seringue au lecteur, règle le volume poussé avec le
// bouton rotatif, puis appuie sur INJECTER : le pupitre envoie {agent, mg/mL, mL, voie} au relais ;
// la régie convertit en mg/kg avec le poids du patient et renvoie le verdict du moteur, affiché
// sur l'écran LCD.
//
// Table des seringues : SYRINGES ci-dessous. Une étiquette inconnue affiche son identifiant
// (UID) sur l'écran et le port série : le reporter dans la table puis téléverser.
// Les concentrations proposées sont celles des présentations commerciales de
// web/anesthesia.js (docs/ANESTHESIE_SOURCES.md) ; ce sont des étiquettes, pas des paramètres
// du modèle.
//
// Bibliothèques : MFRC522 (GithubCommunity), LiquidCrystal I2C (Frank de Brabander).
// Câblage : docs/INTEGRATION_MATERIEL.tex, § Pupitre d'injection. Usage pédagogique uniquement.
#include <LiquidCrystal_I2C.h>
#include <MFRC522.h>
#include <SPI.h>
#include <VetTwinRelay.h>

using vettwin::Debounced;

struct Syringe {
  const char* uid;    // UID de l'étiquette, hexadécimal majuscule sans séparateur
  const char* agent;  // identifiant du module (modules/anesthesia/agents/<agent>.json)
  float conc;         // mg/mL, tel qu'étiqueté
  const char* route;  // "iv", "im" ou "sc" (voies disponibles : onglet Anesthésie)
  float push_s;       // durée de l'injection IV (s) ; 30 s comme le défaut de l'onglet Anesthésie
  const char* label;  // 16 caractères au plus (écran)
};

// À compléter avec les UID de vos étiquettes.
const Syringe SYRINGES[] = {
    {"04A1B2C3D4E580", "propofol", 10.0, "iv", 30, "Propofol 10"},
    {"04A1B2C3D4E581", "alfaxalone", 10.0, "iv", 30, "Alfaxalone 10"},
    {"04A1B2C3D4E582", "dexmedetomidine", 0.5, "im", 0, "Dexdomitor 0,5"},
    {"04A1B2C3D4E583", "methadone", 10.0, "im", 0, "Methadone 10"},
    {"04A1B2C3D4E584", "ketamine", 100.0, "iv", 30, "Ketamine 100"},
    {"04A1B2C3D4E585", "midazolam", 5.0, "iv", 30, "Midazolam 5"},
    {"04A1B2C3D4E586", "atipamezole", 5.0, "im", 0, "Antisedan 5"},
    {"04A1B2C3D4E587", "naloxone", 0.4, "iv", 30, "Naloxone 0,4"},
};

// Broches (ESP32 DevKitC) : lecteur RC522 sur le bus SPI matériel, écran sur I2C (21/22).
const int RC522_SS = 5, RC522_RST = 4;  // SCK 18, MISO 19, MOSI 23
const int ENC_A = 32, ENC_B = 33;       // bouton rotatif (KY-040 : CLK, DT)
Debounced encSw(25);                    // poussoir du bouton rotatif : pas 0,1 / 1 mL
Debounced injectBtn(26);                // gros bouton INJECTER
Debounced clearBtn(27);                 // ANNULER (oublie la seringue présentée)
const float ML_MAX = 60.0;              // volume maximal réglable (taille de seringue)
const uint32_t SYRINGE_TIMEOUT_MS = 120000;

MFRC522 rfid(RC522_SS, RC522_RST);
LiquidCrystal_I2C lcd(0x27, 16, 2);  // adresse 0x27 ou 0x3F selon le module

const Syringe* current = nullptr;
float ml = 0, step = 0.1;
uint32_t seenAt = 0;
int encLast = 0;

void show(const String& l1, const String& l2) {
  lcd.clear();
  lcd.setCursor(0, 0);
  lcd.print(l1.substring(0, 16));
  lcd.setCursor(0, 1);
  lcd.print(l2.substring(0, 16));
}

String fmtMl(float v) {
  char b[12];
  snprintf(b, sizeof b, "%.1f mL", v);
  return b;
}

void showCurrent() {
  if (!current) {
    show("Presenter une", "seringue");
    return;
  }
  String route = current->route;
  route.toUpperCase();
  show(current->label, fmtMl(ml) + " " + route + (step >= 1 ? " x1" : ""));
}

String uidHex() {
  String s;
  for (byte i = 0; i < rfid.uid.size; i++) {
    char b[3];
    snprintf(b, sizeof b, "%02X", rfid.uid.uidByte[i]);
    s += b;
  }
  return s;
}

/** Décodage du bouton rotatif en quadrature (scruté ; une graduation par cran). */
int readEncoder() {
  int a = digitalRead(ENC_A);
  int d = 0;
  if (a != encLast && a == LOW) d = digitalRead(ENC_B) != a ? 1 : -1;
  encLast = a;
  return d;
}

void inject() {
  if (!current) {
    show("Aucune seringue", "presentee");
    return;
  }
  if (ml <= 0) {
    show("Regler le volume", "avant d'injecter");
    return;
  }
  char in[200];
  snprintf(in, sizeof in, "{\"type\":\"inject\",\"agent\":\"%s\",\"conc_mg_ml\":%.4f,\"ml\":%.2f,\"route\":\"%s\",\"duration_s\":%.0f}",
           current->agent, current->conc, ml, current->route, current->push_s);
  show("Injection...", fmtMl(ml));
  vettwin::Result r = vettwin::send(in);
  // verdict du moteur : accepté (libellé de l'action) ou motif du refus
  show(r.ok ? "OK" : "REFUSE", vettwin::ascii(r.text));
  Serial.println(r.text);
  delay(r.ok ? 1500 : 4000);
  if (r.ok) ml = 0;
  showCurrent();
}

void setup() {
  Serial.begin(115200);
  pinMode(ENC_A, INPUT_PULLUP);
  pinMode(ENC_B, INPUT_PULLUP);
  encLast = digitalRead(ENC_A);
  encSw.begin();
  injectBtn.begin();
  clearBtn.begin();
  lcd.init();
  lcd.backlight();
  show("VetTwins", "Wi-Fi...");
  SPI.begin();
  rfid.PCD_Init();
  vettwin::begin("pupitre");
  showCurrent();
}

void loop() {
  if (rfid.PICC_IsNewCardPresent() && rfid.PICC_ReadCardSerial()) {
    String uid = uidHex();
    const Syringe* found = nullptr;
    for (const auto& s : SYRINGES)
      if (uid == s.uid) found = &s;
    if (found) {
      if (found != current) ml = 0;
      current = found;
      seenAt = millis();
      showCurrent();
    } else {
      Serial.printf("Etiquette inconnue : %s\n", uid.c_str());
      show("Inconnue, UID :", uid);  // UID de 7 octets : 14 caracteres
    }
    rfid.PICC_HaltA();
  }
  int d = readEncoder();
  if (d && current) {
    ml = constrain(ml + d * step, 0.0f, ML_MAX);
    ml = roundf(ml * 10) / 10;
    seenAt = millis();
    showCurrent();
  }
  if (encSw.update() && encSw.active()) {
    step = step < 1 ? 1.0 : 0.1;
    showCurrent();
  }
  if (injectBtn.update() && injectBtn.active()) inject();
  if (clearBtn.update() && clearBtn.active()) {
    current = nullptr;
    ml = 0;
    showCurrent();
  }
  if (current && millis() - seenAt > SYRINGE_TIMEOUT_MS) {
    current = nullptr;
    ml = 0;
    showCurrent();
  }
}
