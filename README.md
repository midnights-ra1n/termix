# RustTerm

Terminal SSH léger et personnalisable : Rust (Tauri v2, russh) + xterm.js.

- Hôtes en arborescence (dossiers et sous-dossiers, glisser-déposer), tags, recherche, favoris, icônes intégrées, statut en direct (SSH / ping / injoignable), import/export JSON
- Interface en français, anglais, espagnol et allemand
- Auth : agent SSH (Bitwarden, 1Password, KeePassXC, Proton Pass, Secretive… détecté via `~/.ssh/config` et votre shell), clé, mot de passe (trousseau du système)
- Onglets et splits droite/gauche/haut/bas, raccourcis personnalisables par plateforme
- Panneau latéral : SFTP (parcourir, envoyer, télécharger, créer, supprimer) et stats (CPU, mémoire, disque, réseau, load average)
- Thèmes : Midnights (nuit/aube), Nord, Dracula, Solarized, Gruvbox, Système + thèmes perso
- Barre des menus macOS native, icône tray native (macOS, GNOME, KDE, Windows), exécution en arrière-plan (optionnelles)
- Mises à jour auto/manuelles depuis les releases GitHub (canaux stable, beta, nightly)

Configuration : [docs/CONFIG.md](docs/CONFIG.md).

## Développement

```sh
pnpm install
pnpm tauri dev
cargo test --manifest-path src-tauri/Cargo.toml
```

Linux : `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`. L'icône tray sous GNOME nécessite l'extension AppIndicator.

## Docker (accès navigateur)

RustTerm tourne dans un bureau virtuel servi par noVNC : ouvrez `http://<serveur>:5800`.

```sh
docker compose up -d          # image ghcr.io/midnights-ra1n/rustterm:stable (amd64, arm64)
docker compose up -d --build  # ou construire depuis les sources
```

- Réglages, hôtes et `known_hosts` sont dans le volume `/config`.
- Agent SSH : montez le socket de l'hôte (`${SSH_AUTH_SOCK}:/ssh-agent`) et définissez `SSH_AUTH_SOCK=/ssh-agent` (voir `docker-compose.yml`). Le socket doit être lisible par `USER_ID`.
- Pas de trousseau système dans le conteneur : les mots de passe ne sont pas mémorisés, utilisez l'agent ou une clé dans `/config`.
- Exposé sur un réseau, activez `WEB_AUTHENTICATION` ou placez-le derrière un reverse proxy authentifié : la page donne accès à vos serveurs.
- Les tags `stable`, `beta`, `nightly` suivent les branches ; mettez à jour avec `docker compose pull`.

## Branches et releases

| Branche | Rôle | Release |
|---|---|---|
| `dev` | développement (par défaut) | CI seulement |
| `nightly` | expérimental | pré-release `nightly`, chaque jour s'il y a des commits |
| `beta` | instable | pré-release `beta` à chaque push |
| `stable` | stable | release `v<version>` à chaque push |

Secret GitHub requis : `TAURI_SIGNING_PRIVATE_KEY` (clé de signature de l'updater, la publique est dans `src-tauri/tauri.conf.json`).
