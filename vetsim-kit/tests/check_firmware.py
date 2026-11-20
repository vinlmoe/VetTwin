#!/usr/bin/env python3
"""Contrôle des micrologiciels ESP32 de la salle (salle/firmware) sans la chaîne Arduino.

1. Compile chaque croquis (.ino) sur l'hôte avec g++ en C++11, contre des en-têtes factices
   (Arduino, WiFi, HTTPClient, MFRC522, LiquidCrystal_I2C) : syntaxe, types, prototypes.
   Ce n'est pas une compilation ESP32 : les broches, le Wi-Fi et les bibliothèques réelles ne
   sont vérifiés qu'en téléversant (docs/INTEGRATION_MATERIEL.tex).
2. Exécute les fonctions de la bibliothèque VetTwinRelay (lecture des réponses JSON du relais,
   échappement, translittération pour l'écran LCD) sur des réponses réelles du relais.

    python tests/check_firmware.py
"""
import glob
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.join(HERE, "..", "salle", "firmware")
LIB = os.path.join(FW, "libraries", "VetTwinRelay", "src")

ARDUINO_H = r'''
#pragma once
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <algorithm>
typedef uint8_t byte;
enum { LOW = 0, HIGH = 1, INPUT = 0, OUTPUT = 1, INPUT_PULLUP = 2 };
class String {
 public:
  std::string s;
  String() {}
  String(const char* c) : s(c ? c : "") {}
  String(const std::string& x) : s(x) {}
  String(char c) : s(1, c) {}
  String(int v) : s(std::to_string(v)) {}
  String(unsigned v) : s(std::to_string(v)) {}
  String(long v) : s(std::to_string(v)) {}
  String(unsigned long v) : s(std::to_string(v)) {}
  String(float v) { char b[32]; snprintf(b, sizeof b, "%.2f", v); s = b; }
  String(double v) { char b[32]; snprintf(b, sizeof b, "%.2f", v); s = b; }
  size_t length() const { return s.size(); }
  void reserve(size_t n) { s.reserve(n); }
  const char* c_str() const { return s.c_str(); }
  char operator[](size_t i) const { return i < s.size() ? s[i] : 0; }
  String& operator+=(const String& o) { s += o.s; return *this; }
  String& operator+=(const char* o) { s += o; return *this; }
  String& operator+=(char c) { s += c; return *this; }
  friend String operator+(const String& a, const String& b) { return String(a.s + b.s); }
  friend String operator+(const String& a, const char* b) { return String(a.s + b); }
  friend String operator+(const char* a, const String& b) { return String(std::string(a) + b.s); }
  bool operator==(const String& o) const { return s == o.s; }
  bool operator==(const char* o) const { return s == o; }
  bool operator!=(const String& o) const { return s != o.s; }
  int indexOf(const String& f, int from = 0) const { auto p = s.find(f.s, from < 0 ? 0 : from); return p == std::string::npos ? -1 : (int)p; }
  int indexOf(char c, int from = 0) const { auto p = s.find(c, from < 0 ? 0 : from); return p == std::string::npos ? -1 : (int)p; }
  String substring(int a, int b = -1) const { if (a > (int)s.size()) return String(); return String(s.substr(a, b < 0 ? std::string::npos : b - a)); }
  void replace(const String& f, const String& r) { if (f.s.empty()) return; size_t p = 0; while ((p = s.find(f.s, p)) != std::string::npos) { s.replace(p, f.s.size(), r.s); p += r.s.size(); } }
  void toUpperCase() { for (auto& c : s) c = (char)toupper((unsigned char)c); }
};
struct SerialMock { void begin(long) {} template <class... A> void printf(const char* f, A... a) { std::printf(f, a...); } void println(const String& x) { std::puts(x.c_str()); } void println(const char* x) { std::puts(x); } };
extern SerialMock Serial;
uint32_t millis();
void delay(uint32_t);
int digitalRead(int);
void digitalWrite(int, int);
void pinMode(int, int);
int analogRead(int);
void analogReadResolution(int);
template <class T, class L, class H> T constrain(T x, L lo, H hi) { return x < lo ? (T)lo : x > hi ? (T)hi : x; }
using std::max; using std::min;
void ledcWrite(int, int);
void ledcAttach(int, int, int);
typedef void (*TaskFn)(void*);
int xTaskCreatePinnedToCore(TaskFn, const char*, int, void*, int, void*, int);
void vTaskDelay(uint32_t);
#define pdMS_TO_TICKS(x) (x)
'''
STUBS = {
    "esp_arduino_version.h": "#pragma once\n#define ESP_ARDUINO_VERSION_MAJOR 3\n",
    "WiFi.h": r'''#pragma once
#include <Arduino.h>
enum { WIFI_STA = 1, WL_CONNECTED = 3 };
struct IP { String toString() const { return "0.0.0.0"; } };
struct WiFiMock { void mode(int) {} void setHostname(const char*) {} void begin(const char*, const char*) {} int status() { return WL_CONNECTED; } void reconnect() {} IP localIP() { return IP(); } };
extern WiFiMock WiFi;
''',
    "HTTPClient.h": r'''#pragma once
#include <Arduino.h>
struct HTTPClient { void setTimeout(int) {} bool begin(const String&) { return true; } void addHeader(const char*, const char*) {} int POST(const String&) { return 200; } int GET() { return 200; } String getString() { return ""; } void end() {} };
''',
    "SPI.h": "#pragma once\nstruct SPIMock { void begin() {} };\nextern SPIMock SPI;\n",
    "MFRC522.h": r'''#pragma once
#include <Arduino.h>
struct MFRC522 { struct { byte size; byte uidByte[10]; } uid; MFRC522(int, int) {} void PCD_Init() {} bool PICC_IsNewCardPresent() { return false; } bool PICC_ReadCardSerial() { return false; } void PICC_HaltA() {} };
''',
    "LiquidCrystal_I2C.h": r'''#pragma once
#include <Arduino.h>
struct LiquidCrystal_I2C { LiquidCrystal_I2C(int, int, int) {} void init() {} void backlight() {} void clear() {} void setCursor(int, int) {} void print(const String&) {} };
''',
}
UNIT = r'''
#include <VetTwinRelay.h>
#include <cassert>
SerialMock Serial; WiFiMock WiFi;
uint32_t millis() { return 0; } void delay(uint32_t) {} int digitalRead(int) { return 1; } void digitalWrite(int, int) {} void pinMode(int, int) {}
int analogRead(int) { return 0; } void analogReadResolution(int) {} void ledcWrite(int, int) {} void ledcAttach(int, int, int) {}
int xTaskCreatePinnedToCore(TaskFn, const char*, int, void*, int, void*, int) { return 1; } void vTaskDelay(uint32_t) {}
int fails = 0;
#define CHECK(c, what) do { if (c) std::printf("ok   %s\n", what); else { std::printf("ÉCHEC %s\n", what); fails++; } } while (0)
int main() {
  // réponses telles que les écrit relay.py (json.dumps, séparateurs « , » et « : », UTF-8)
  String acc = "{\"ok\": true, \"text\": \"propofol 3 mg/kg IV (injection 30 s)\"}";
  String ref = "{\"ok\": false, \"text\": \"intubation : refusé (intubation impossible : déglutition présente (plan trop léger))\"}";
  String err = "{\"ok\": false, \"error\": \"régie non connectée\"}";
  String q = "{\"ok\": true, \"text\": \"vaporisateur \\\"iso\\\" 2 %\"}";
  CHECK(vettwin::jsonTrue(acc, "ok"), "verdict accepté lu");
  CHECK(!vettwin::jsonTrue(ref, "ok"), "verdict refusé lu");
  CHECK(vettwin::jsonString(acc, "text") == "propofol 3 mg/kg IV (injection 30 s)", "texte du verdict");
  CHECK(vettwin::jsonString(err, "text") == "", "texte absent");
  CHECK(vettwin::jsonString(err, "error") == "régie non connectée", "motif d'erreur du relais");
  CHECK(vettwin::jsonString(q, "text") == "vaporisateur \"iso\" 2 %", "guillemets échappés");
  CHECK(vettwin::esc("a\"b\\c\nd") == "a\\\"b\\\\c d", "échappement JSON");
  String lcd = vettwin::ascii(vettwin::jsonString(ref, "text"));
  CHECK(lcd == "intubation : refuse (intubation impossible : deglutition presente (plan trop leger))", "translittération LCD");
  CHECK(vettwin::ascii("SpO₂ 98 %, 38,4 °C, cœur") == "SpO2 98 %, 38,4 oC, coeur", "translittération : indices, symboles, ligatures");
  return fails ? 1 : 0;
}
'''
FAIL = []


def check(cond, what, detail=""):
    print(("ok   " if cond else "ÉCHEC ") + what)
    if not cond:
        FAIL.append(what)
        if detail:
            print(detail)


def main():
    gxx = shutil.which("g++")
    if not gxx:
        print("g++ absent : contrôle non exécuté")
        sys.exit(2)
    with tempfile.TemporaryDirectory() as tmp:
        inc = os.path.join(tmp, "inc")
        os.makedirs(inc)
        open(os.path.join(inc, "Arduino.h"), "w").write(ARDUINO_H)
        for name, text in STUBS.items():
            open(os.path.join(inc, name), "w").write(text)
        # VetTwinConfig.h tiré de l'exemple (le vrai, avec le mot de passe Wi-Fi, n'est pas versionné)
        shutil.copy(os.path.join(LIB, "VetTwinConfig.example.h"), os.path.join(inc, "VetTwinConfig.h"))
        lib = os.path.join(tmp, "lib")
        shutil.copytree(LIB, lib, ignore=shutil.ignore_patterns("VetTwinConfig.h"))
        shutil.copy(os.path.join(inc, "VetTwinConfig.h"), os.path.join(lib, "VetTwinConfig.h"))
        flags = ["-std=gnu++11", "-Wall", "-Wextra", "-Wno-unused-parameter", "-I", inc, "-I", lib]
        sketches = sorted(glob.glob(os.path.join(FW, "*", "*.ino")))
        check(len(sketches) == 6, f"{len(sketches)} croquis trouvés (6 attendus)")
        for ino in sketches:
            name = os.path.basename(ino)
            r = subprocess.run([gxx, *flags, "-fsyntax-only", "-x", "c++", "-include", "Arduino.h", ino], capture_output=True, text=True)
            warn = [ln for ln in r.stderr.splitlines() if "warning" in ln]
            check(r.returncode == 0 and not warn, f"compilation hôte (C++11, -Wall -Wextra) : {name}", r.stderr[-3000:])
        src = os.path.join(tmp, "unit.cpp")
        open(src, "w").write(UNIT)
        exe = os.path.join(tmp, "unit")
        r = subprocess.run([gxx, *flags, "-include", "Arduino.h", src, "-o", exe], capture_output=True, text=True)
        check(r.returncode == 0, "compilation des tests de la bibliothèque", r.stderr[-3000:])
        if r.returncode == 0:
            r = subprocess.run([exe], capture_output=True, text=True)
            print(r.stdout, end="")
            check(r.returncode == 0, "tests de la bibliothèque VetTwinRelay")
    print(f"\n{len(FAIL)} échec(s)" if FAIL else "\nmicrologiciels : tous les contrôles passent")
    sys.exit(1 if FAIL else 0)


if __name__ == "__main__":
    main()
