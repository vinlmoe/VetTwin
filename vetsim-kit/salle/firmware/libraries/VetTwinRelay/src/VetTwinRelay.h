// Client du relais de la salle de simulation VetTwins (salle/relay.py) pour ESP32.
// - send()  : POST /api/device {"device": ..., "input": {...}} ; renvoie le verdict du moteur ;
// - pulse() : GET /api/pulse, une ligne "fc pas pad décès valide âge_s".
// Le boîtier ne connaît ni le poids du patient ni la physiologie : il envoie ce qu'il mesure
// (volume poussé, graduation d'un bouton, capteur posé) et la régie traduit.
// Usage pédagogique uniquement.
#pragma once
#include <Arduino.h>
#include <HTTPClient.h>
#include <WiFi.h>

#if __has_include("VetTwinConfig.h")
#include "VetTwinConfig.h"
#else
#error "Copier VetTwinConfig.example.h en VetTwinConfig.h (dossier de la bibliothèque VetTwinRelay) et le remplir."
#endif

namespace vettwin {

struct Result {
  int code;     // code HTTP (200 : verdict du moteur ; 503 : régie absente ; <= 0 : réseau)
  bool ok;      // événement accepté par le moteur
  String text;  // libellé de l'action ou motif du refus (UTF-8)
};

struct Pulse {
  float hr = 0, sap = 0, dap = 0;
  bool dead = false;
  bool valid = false;  // régie connectée et état de moins de 5 s
};

/** Nom du boîtier dans le journal de la régie (fixé par begin()). */
inline const char*& deviceName() {
  static const char* name = "boitier";
  return name;
}

/** Connexion Wi-Fi (bloquante au plus `timeoutMs`) ; à rappeler dans loop() via ensureWifi(). */
inline bool begin(const char* name, uint32_t timeoutMs = 15000) {
  deviceName() = name;
  WiFi.mode(WIFI_STA);
  WiFi.setHostname(name);
  WiFi.begin(VETTWIN_WIFI_SSID, VETTWIN_WIFI_PASS);
  uint32_t t0 = millis();
  while (WiFi.status() != WL_CONNECTED && millis() - t0 < timeoutMs) delay(200);
  Serial.printf("[vettwin] %s : Wi-Fi %s, IP %s\n", name, WiFi.status() == WL_CONNECTED ? "connecté" : "absent",
                WiFi.localIP().toString().c_str());
  return WiFi.status() == WL_CONNECTED;
}

inline bool ensureWifi() {
  if (WiFi.status() == WL_CONNECTED) return true;
  static uint32_t last = 0;
  if (millis() - last > 5000) {
    last = millis();
    WiFi.reconnect();
  }
  return false;
}

/** Échappement JSON d'une chaîne (guillemets, barre oblique inverse, contrôles). */
inline String esc(const String& s) {
  String o;
  o.reserve(s.length() + 2);
  for (size_t i = 0; i < s.length(); i++) {
    char c = s[i];
    if (c == '"' || c == '\\') {
      o += '\\';
      o += c;
    } else if ((unsigned char)c < 0x20) {
      o += ' ';
    } else {
      o += c;
    }
  }
  return o;
}

/** Valeur texte d'une clé dans une réponse JSON plate du relais (sans dépendance). */
inline String jsonString(const String& body, const char* key) {
  String k = String("\"") + key + "\"";
  int i = body.indexOf(k);
  if (i < 0) return "";
  i = body.indexOf(':', i + k.length());
  if (i < 0) return "";
  i = body.indexOf('"', i);
  if (i < 0) return "";
  String out;
  for (int j = i + 1; j < (int)body.length(); j++) {
    char c = body[j];
    if (c == '\\' && j + 1 < (int)body.length()) {
      out += body[++j];
      continue;
    }
    if (c == '"') break;
    out += c;
  }
  return out;
}

inline bool jsonTrue(const String& body, const char* key) {
  String k = String("\"") + key + "\"";
  int i = body.indexOf(k);
  if (i < 0) return false;
  i = body.indexOf(':', i + k.length());
  if (i < 0) return false;
  while (++i < (int)body.length() && body[i] == ' ') {
  }
  return body.substring(i, i + 4) == "true";
}

/** Texte UTF-8 → ASCII (écrans LCD HD44780 sans accents) : é→e, ç→c, œ→oe, ₂→2... */
inline String ascii(const String& s) {
  static const char* const map[][2] = {
      {"\xC3\xA0", "a"}, {"\xC3\xA2", "a"}, {"\xC3\xA4", "a"}, {"\xC3\xA7", "c"}, {"\xC3\xA8", "e"}, {"\xC3\xA9", "e"},
      {"\xC3\xAA", "e"}, {"\xC3\xAB", "e"}, {"\xC3\xAE", "i"}, {"\xC3\xAF", "i"}, {"\xC3\xB4", "o"}, {"\xC3\xB6", "o"},
      {"\xC3\xB9", "u"}, {"\xC3\xBB", "u"}, {"\xC3\xBC", "u"}, {"\xC3\x89", "E"}, {"\xC3\x88", "E"}, {"\xC3\x80", "A"},
      {"\xC3\x87", "C"}, {"\xC5\x93", "oe"}, {"\xE2\x82\x82", "2"}, {"\xC2\xB0", "o"}, {"\xC2\xB5", "u"},
      {"\xE2\x80\x99", "'"}, {"\xC2\xAB", "\""}, {"\xC2\xBB", "\""}, {"\xE2\x86\x92", ">"}, {"\xE2\x80\xAF", " "},
      {"\xC2\xA0", " "},
  };
  String o = s;
  for (const auto& m : map) o.replace(m[0], m[1]);
  String r;
  for (size_t i = 0; i < o.length(); i++)
    if ((unsigned char)o[i] < 0x80) r += o[i];  // autres caractères non ASCII : retirés
  return r;
}

/** Envoie une entrée (`inputJson` : objet JSON complet, ex. {"type":"probes","ecg":true}). */
inline Result send(const String& inputJson) {
  Result r{0, false, ""};
  if (!ensureWifi()) {
    r.code = -1;
    r.text = "Wi-Fi absent";
    return r;
  }
  HTTPClient http;
  http.setTimeout(5000);  // le relais attend la régie au plus 3 s
  if (!http.begin(String(VETTWIN_RELAY_URL) + "/api/device")) {
    r.code = -2;
    r.text = "adresse du relais invalide";
    return r;
  }
  http.addHeader("Content-Type", "application/json");
  if (strlen(VETTWIN_RELAY_KEY)) http.addHeader("X-VetTwin-Key", VETTWIN_RELAY_KEY);
  String body = String("{\"device\":\"") + esc(deviceName()) + "\",\"input\":" + inputJson + "}";
  r.code = http.POST(body);
  String resp = r.code > 0 ? http.getString() : String("");
  http.end();
  if (r.code <= 0) {
    r.text = "relais injoignable";
    return r;
  }
  r.ok = r.code == 200 && jsonTrue(resp, "ok");
  r.text = jsonString(resp, "text");
  if (r.text.length() == 0) r.text = jsonString(resp, "error");
  Serial.printf("[vettwin] %s -> %d %s %s\n", inputJson.c_str(), r.code, r.ok ? "accepté" : "refusé", r.text.c_str());
  return r;
}

/** Lit l'état du pouls ; false si le relais est injoignable. */
inline bool pulse(Pulse& p) {
  p.valid = false;
  if (!ensureWifi()) return false;
  HTTPClient http;
  http.setTimeout(1500);
  if (!http.begin(String(VETTWIN_RELAY_URL) + "/api/pulse")) return false;
  if (strlen(VETTWIN_RELAY_KEY)) http.addHeader("X-VetTwin-Key", VETTWIN_RELAY_KEY);
  int code = http.GET();
  String line = code == 200 ? http.getString() : String("");
  http.end();
  if (code != 200) return false;
  int dead = 0, valid = 0;
  float age = 999;
  if (sscanf(line.c_str(), "%f %f %f %d %d %f", &p.hr, &p.sap, &p.dap, &dead, &valid, &age) != 6) return false;
  p.dead = dead != 0;
  p.valid = valid != 0;
  return true;
}

/** Bouton ou contact anti-rebond (entrée à résistance de tirage, actif à l'état bas). */
class Debounced {
 public:
  explicit Debounced(uint8_t pin, uint16_t ms = 40) : pin_(pin), ms_(ms) {}
  void begin() {
    pinMode(pin_, INPUT_PULLUP);
    state_ = raw_ = digitalRead(pin_) == LOW;
  }
  /** true si l'état stable vient de changer. */
  bool update() {
    bool r = digitalRead(pin_) == LOW;
    if (r != raw_) {
      raw_ = r;
      t_ = millis();
    }
    if (raw_ != state_ && millis() - t_ >= ms_) {
      state_ = raw_;
      return true;
    }
    return false;
  }
  bool active() const { return state_; }

 private:
  uint8_t pin_;
  uint16_t ms_;
  bool state_ = false, raw_ = false;
  uint32_t t_ = 0;
};

}  // namespace vettwin
