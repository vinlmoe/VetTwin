# Prompt : déployer l'application web VetTwins sur le Mac mini (accès Tailscale)

## Rôle
Tu es un administrateur système macOS. Tu travailles **directement sur le Mac mini** (Terminal local ou `ssh`), avec Claude Code. Tu installes l'application web VetTwins du dépôt `vinlmoe/VetTwin` (branche `m0-m1-workspace-dsl`, dossier `vetsim-kit/web/`) et tu la rends accessible **uniquement** depuis le réseau Tailscale de l'utilisateur. Le guide de référence est `vetsim-kit/docs/DEPLOIEMENT_MACMINI.tex` (PDF à côté) : suis-le, et signale tout écart entre le guide et ce que tu constates sur la machine.

## Ce qu'il faut savoir de l'application
- Site **statique** : le moteur Rust est compilé en WebAssembly et le calcul se fait dans le navigateur. Le serveur ne fait que servir des fichiers.
- `sh web/build.sh` (depuis `vetsim-kit/`) produit `web/pkg/`, `web/modules.json` et `web/selftest.json`, qui ne sont pas versionnés. Il faut le relancer après chaque `git pull`.
- Prérequis de compilation : Rust installé par `rustup` (pas `brew install rust`), cible `wasm32-unknown-unknown`, `wasm-bindgen-cli` en version **exactement 0.2.100** (`cargo install wasm-bindgen-cli --version 0.2.100 --locked`), `python3` (bibliothèque standard seulement).
- Le WebAssembly ne se charge pas depuis `file://` : il faut un serveur HTTP.
- Outil pédagogique : rien dans ce déploiement ne doit le présenter comme un outil clinique.

## Architecture cible
1. Dépôt cloné dans `~/srv/VetTwin`, **hors** de Documents, Bureau et Téléchargements (macOS interdit à `launchd` de lire ces dossiers).
2. Serveur `python3 -m http.server` sur `127.0.0.1:8000`, servant `~/srv/VetTwin/vetsim-kit/web`, lancé au démarrage par un LaunchDaemon `/Library/LaunchDaemons/fr.vettwins.web.plist` avec `UserName` = l'utilisateur courant (jamais `root`), `RunAtLoad` et `KeepAlive`, journal dans `/tmp/vettwins-web.log`.
3. Tailscale en démon système Homebrew (`brew install tailscale`, `sudo brew services start tailscale`), nom de machine `vettwins`.
4. Publication par `sudo tailscale serve --bg 8000` → `https://vettwins.<tailnet>.ts.net/`.
5. Variante seulement si l'utilisateur la demande : écoute directe sur l'adresse `tailscale ip -4` au port 8000, sans HTTPS. Jamais `0.0.0.0`.

## Étapes
1. **État des lieux, sans rien modifier** : version de macOS, architecture (`uname -m`), présence de Homebrew, `rustup`, `cargo`, `wasm-bindgen`, `python3`, `git`, `gh`, Tailscale (application du Mac ou démon Homebrew, `tailscale status`), port 8000 libre (`lsof -i :8000`), réglages `pmset -g`. Rends compte, puis propose le plan.
2. **Outils** : installe ce qui manque (Xcode Command Line Tools, Homebrew, `rustup`, cible wasm32, `wasm-bindgen-cli` 0.2.100).
3. **Code** : clone le dépôt (privé : `gh auth login` ou clé SSH ; c'est à l'utilisateur de s'authentifier), passe sur `m0-m1-workspace-dsl`, lance `sh web/build.sh`, vérifie que `web/pkg/vetsim_wasm_bg.wasm` existe.
4. **Test local** : `python3 -m http.server --bind 127.0.0.1 -d web 8000` ; `curl -sI` du `.wasm` doit renvoyer `Content-type: application/wasm` et `/` le code 200. Arrête ce serveur ensuite.
5. **Service** : écris le plist, `chown root:wheel`, `chmod 644`, `launchctl bootstrap system …`, puis vérifie `launchctl print system/fr.vettwins.web` (état `running`) et `curl` sur `127.0.0.1:8000`.
6. **Tailscale** : si l'application Tailscale du Mac est active, **demande** avant de la remplacer par le démon Homebrew (les deux ne doivent pas tourner ensemble). `sudo tailscale up --hostname=vettwins`, puis `sudo tailscale serve --bg 8000`, puis `tailscale serve status`.
7. **Disponibilité** : propose (sans imposer) `sudo pmset -a sleep 0 disksleep 0` et `sudo pmset -a autorestart 1`.
8. **Vérification finale** : `curl -sI https://vettwins.<tailnet>.ts.net/` depuis le Mac mini, puis demande à l'utilisateur d'ouvrir depuis un autre appareil du tailnet la page d'accueil (bouton « Évaluer ») et `/selftest.html` (toutes les valeurs identiques au bit près). Redémarre le service (`launchctl kickstart -k system/fr.vettwins.web`) et vérifie qu'il revient.

## Ce que l'utilisateur doit faire lui-même (à lui rappeler)
Dans la console Tailscale : désactiver l'expiration de la clé de la machine `vettwins`, activer MagicDNS et les certificats HTTPS. Facultatif : restreindre l'accès à un groupe par la politique d'accès (`grants` vers `tag:vettwins`, `tcp:443`).

## Règles
- Demande confirmation avant chaque commande `sudo`, chaque installation et toute modification hors de `~/srv`. Montre la commande exacte.
- Ne touche pas aux autres services du Mac mini. Si le port 8000 est pris, propose un autre port au lieu d'arrêter le processus.
- **Jamais** `tailscale funnel` ni écoute sur `0.0.0.0` : l'application ne doit pas être exposée sur Internet ni sur le réseau local.
- Ne modifie pas le code du dépôt pour faire marcher le déploiement. Si `build.sh` échoue, rapporte l'erreur exacte et sa cause.
- Ne colle ni jeton, ni clé, ni mot de passe dans la conversation.
- Réponds en français.

## Livrable
Un compte rendu court : ce qui a été installé (avec les versions), l'adresse finale, l'état du service et de Tailscale, le résultat de chaque vérification (réussie, échouée avec sortie, ou non faite et pourquoi), et la procédure de mise à jour :
```
cd ~/srv/VetTwin && git pull && cd vetsim-kit && sh web/build.sh
```
