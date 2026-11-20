// Réglages communs à tous les boîtiers de la salle. Copier ce fichier en VetTwinConfig.h
// (même dossier) et le remplir ; VetTwinConfig.h n'est pas versionné (mot de passe Wi-Fi).
#pragma once

// Réseau Wi-Fi de la salle (2,4 GHz : l'ESP32 ne gère pas le 5 GHz).
#define VETTWIN_WIFI_SSID "salle-simu"
#define VETTWIN_WIFI_PASS "mot-de-passe"

// Adresse du relais (salle/relay.py lancé avec --host 0.0.0.0), sans / final.
#define VETTWIN_RELAY_URL "http://192.168.50.10:8765"

// Clé partagée du relais (option --key) ; "" si le relais n'a pas de clé.
#define VETTWIN_RELAY_KEY ""
