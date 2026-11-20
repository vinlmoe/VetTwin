# Salle de simulation VetTwins : relais et boîtiers

Guide complet (matériel, câblage, code, essais) : `docs/INTEGRATION_MATERIEL.pdf`.

- `relay.py` : relais de salle (Python 3.8+, bibliothèque standard). Sert `web/`, WebSocket `/ws`, `POST /api/device`, `GET /api/state`, `GET /api/pulse`, `GET /api/status`.
- `vettwins-relay.service` (systemd) et `fr.vettwins.relay.plist` (launchd) : lancement au démarrage.
- `firmware/` : micrologiciels ESP32 (Arduino) des six boîtiers et bibliothèque `VetTwinRelay`.

| Page | Rôle |
|---|---|
| `index.html?room=1` | régie : onglet Anesthésie, seule à exécuter le moteur |
| `monitor.html?room=1` | écran moniteur de la salle (voies masquées sans capteur posé) |
| `ausc.html?room=1` | lecteur d'auscultation (haut-parleur du mannequin) |

Essais : `python tests/check_relay.py`, `python tests/check_firmware.py`, `node tests/room_e2e.mjs` (après `sh web/build.sh`, Playwright requis).

Usage pédagogique uniquement : aucun résultat ne sert à décider d'un traitement réel.
