<p align="center"><img src="assets/icon-rounded.svg" width="96" alt=""></p>

<h1 align="center">Termix</h1>

<p align="center">A light, customizable SSH terminal for macOS, Linux and Windows.<br>
Rust (Tauri v2, russh) and xterm.js. No telemetry, no account required.</p>

---

## Contents

- [Features](#features)
- [Install](#install)
- [Getting started](#getting-started)
- [Authentication and SSH agents](#authentication-and-ssh-agents)
- [Tabs, splits and tmux](#tabs-splits-and-tmux)
- [Side panel](#side-panel)
- [Customization](#customization)
- [Sync](#sync)
- [Sync server (Docker)](#sync-server-docker)
- [Termix in a browser (Docker)](#termix-in-a-browser-docker)
- [Security and privacy](#security-and-privacy)
- [Configuration files](#configuration-files)
- [Development](#development)
- [Branches and releases](#branches-and-releases)

## Features

- **Hosts**: folders and subfolders with drag and drop, tags, search, favorites, built-in icons, live status (SSH, ping only, down), JSON import and export
- **Authentication**: SSH agent (Bitwarden, 1Password, KeePassXC, Proton Pass, Secretive… found through `~/.ssh/config` and your shell), key files, passwords in the system keychain, and an SSH key manager where each key stays on the device or syncs end-to-end encrypted
- **Terminal**: tabs, splits right, left, down and up, keyboard shortcuts per platform, command suggestions, copy on select, zoom
- **Persistent sessions** (optional): shells can run inside tmux when the server has it, so jobs keep running after you disconnect and you resume them from the side panel
- **Side panel**: server stats (CPU, memory, disk, network, load average), SFTP browser, Docker containers and their state, tmux sessions
- **Sync**: hosts, folders, settings, passwords, SSH keys and open tabs between your devices, through a self-hosted server or directly on the local network. Passwords and keys are end-to-end encrypted
- **Customization**: 24 built-in themes plus your own, 11 bundled fonts (5 sans for the interface, 6 mono for the terminal) or any installed one, accent color, density, cursor, padding, letter spacing, everything from the GUI or a TOML file
- **Desktop integration**: native macOS menu bar (same menus in the custom title bar on Windows and Linux, with Windows-style window buttons and the system's rounded corners), tray icon that turns white or black to match the panel, native tray menu on macOS, GNOME, KDE and Windows with your hosts one click away, optional background mode for instant reopening, native Wayland
- **iPhone and iPad**: a native SwiftUI app on the same Rust core: host tree with search, tags, favorites and live status, quick connect and recent sessions, terminal with the desktop themes, a full key bar, pinch to zoom and automatic reconnection, tmux sessions that survive iOS suspending the app, the side panel (stats, SFTP, Docker, tmux), the desktop's open tabs to carry on, SSH keys, server sync with the encrypted vault, keyboard shortcuts on iPad
- **Updates**: automatic and manual, from GitHub releases, on the stable, beta or nightly channel
- **Languages**: English, French, Spanish, German

## Install

Download the build for your system from the [releases page](https://github.com/midnights-ra1n/termix/releases).

| System | Files | Architectures |
|---|---|---|
| macOS 11+ | `.dmg` | Apple silicon, Intel |
| Windows 10+ | `-setup.exe` | x64, arm64 |
| Linux | `.deb`, `.rpm`, `.AppImage` | x86_64, aarch64 |
| iOS / iPadOS 26+ | `_iOS.ipa` (unsigned, to sideload) | arm64 |

**macOS**: the app is not notarized. The first time, right-click Termix in Applications and choose Open, or run:

```sh
xattr -dr com.apple.quarantine /Applications/Termix.app
```

**iPhone and iPad**: the `.ipa` is not signed. Install it with [Sideloadly](https://sideloadly.io), [AltStore](https://altstore.io) or [SideStore](https://sidestore.io), which sign it with your Apple ID (with a free account, the app must be refreshed every 7 days). To build it yourself, see [iPhone and iPad](#iphone-and-ipad).

**Linux**: install the package with your package manager (the `.rpm` is not GPG-signed; updates are checked with Termix's own signature instead):

```sh
sudo apt install ./Termix_*_amd64.deb                              # Debian, Ubuntu
sudo dnf install ./Termix-*.x86_64.rpm                             # Fedora, RHEL, Alma, Rocky
sudo zypper install --allow-unsigned-rpm ./Termix-*.x86_64.rpm     # openSUSE Tumbleweed, Leap 15.6+
```

Updates of a `.deb` or `.rpm` install ask for your password through polkit (`pkexec`, or `systemd-run` where `pkexec` is not installed, as on openSUSE).

The tray icon on GNOME needs the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/). Termix runs natively on Wayland.

Termix checks for updates at launch and once a day, then asks before installing. Choose the channel in Settings → General.

## Getting started

1. Add a host with **+** in the sidebar (⌘N on macOS, Ctrl+Shift+N elsewhere): name, address, port, user, and optionally a key file, a password, tags, a folder and an icon.
2. Double-click it, press Enter on it after a search (⌘K / Ctrl+Shift+K), or drop it on a pane.
3. Right-click a pane or use the toolbar to split it.

Already using Termix under its former name? Settings, hosts and keychain secrets move over automatically on first launch.

## Authentication and SSH agents

For each connection Termix tries, in order:

1. the SSH agent
2. the host's key file, or its saved key (see [SSH keys](#ssh-keys))
3. the password saved in the system keychain, otherwise it asks

The agent is found without setup. Termix reads `IdentityAgent` from `~/.ssh/config` and `SSH_AUTH_SOCK` as set by your `.zshrc`, `.bashrc` or fish config, so the agent of Bitwarden, 1Password, KeePassXC, Proton Pass or Secretive works as it does in your usual terminal. Settings → SSH lists the agents it found and lets you force one. A forced socket that is not running falls back to automatic detection.

### SSH keys

Settings → SSH keys lists the private keys Termix keeps in the system keychain. Add one from a file or paste its text (on iOS: Settings → SSH keys, or **Add a key** in the host editor). Pasting a key into the host's **Private key** field imports it too. Then pick it in the host's **Saved key** field; a key file set on the host still takes precedence.

Each key is either:

- **on this device only**: it never leaves it, or
- **synced**: sealed in the [vault](#the-vault) (end-to-end encrypted) and written to the keychain of your other devices.

Flip one key with its switch (a swipe on iOS), or all of them at once with **Sync all** / **Keep all on this device**. A key keeps its id when it moves, so the hosts using it don't change. Keeping a synced key on one device removes it from the server and from the other devices. New keys sync when the vault is unlocked, unless you turn the switch off when adding them.

You can also point the **Private key** field to a key file anywhere on disk.

## Tabs, splits and tmux

| Action | macOS | Linux / Windows |
|---|---|---|
| New host | ⌘N | Ctrl+Shift+N |
| Close pane | ⌘W | Ctrl+Shift+W |
| Split right / left | ⌘D / ⌘L | Ctrl+Shift+D / L |
| Split down / up | ⌘E / ⌘U | Ctrl+Shift+E / U |
| Next / previous pane | ⌘] / ⌘[ | Ctrl+Shift+] / [ |
| Next / previous tab | ⌘→ / ⌘← | Ctrl+Shift+→ / ← |
| Search hosts | ⌘K | Ctrl+Shift+K |
| Toggle sidebar / panel | ⌘B / ⌘I | Ctrl+Shift+B / I |
| Settings | ⌘, | Ctrl+Shift+, |
| Zoom in / out / reset | ⌘= / ⌘- / ⌘0 | Ctrl+Shift+= / - / 0 |

Every shortcut can be changed in Settings → Shortcuts. Linux and Windows use Ctrl+Shift so that Ctrl+letter still reaches the remote shell. Letters follow your keyboard layout (AZERTY, QWERTZ…).

**tmux**: when enabled and the server has tmux, each pane runs in its own tmux session (`tx-…`) with mouse support and no status bar. Closing the tab or losing the network leaves it running. The tmux tab of the side panel lists the sessions of the server, resumes one in a new tab, or ends it. It is off by default: turn it on for every host in Settings → Terminal, per host in the host editor (general setting / always / never), or for a single connection from the host's right-click menu (**Connect in tmux** / **Connect without tmux**). Hold ⌥ (macOS) or Shift to select text while tmux has the mouse.

## Side panel

Toggle it with ⌘I / Ctrl+Shift+I. It follows the focused pane.

- **Stats**: CPU, memory, disk, network and load average, refreshed every few seconds (Linux servers)
- **SFTP**: browse, upload, download, create folders, delete
- **Docker**: containers with their image and state, read only, without sudo. Shown when Docker is installed and your user can reach it
- **tmux**: sessions, their windows and last activity

## Customization

Settings → Appearance and Settings → Terminal cover:

- **Themes**: Termix (default), Midnights, Aube, Nord, Dracula, Solarized, Gruvbox, Catppuccin Mocha and Latte, Tokyo Night, One Dark, Rosé Pine, Everforest, Kanagawa, GitHub, System (follows light or dark mode), plus your own themes in `config.toml`
- **Fonts**: interface font (default IBM Plex Sans) and terminal font (default IBM Plex Mono), size, weight, line height, letter spacing. Both IBM Plex families ship inside the app: no font is fetched from the network
- **Accent color**, comfortable or compact density, terminal padding, cursor style and blink, bold as bright, scrollback

Everything is also in `config.toml`, documented in [docs/CONFIG.md](docs/CONFIG.md).

## Sync

Settings → Sync keeps these in sync between your devices:

| Data | How |
|---|---|
| Hosts and folders | as is |
| Settings (appearance, terminal, shortcuts, custom themes) | as is |
| Passwords and key passphrases | end-to-end encrypted |
| SSH keys stored in the vault | end-to-end encrypted |
| Open tabs, splits and each pane's tmux session | as is, per device |

Device-specific settings (tray, background mode, update channel, SSH agent) stay local. Each item is merged on its own: the most recent change wins, and deletions propagate.

Choose one of two modes:

- **Termix server**: a small server you host yourself, with a web interface and several users. Works across networks. See [Sync server](#sync-server-docker).
- **Local network**: devices find each other with mDNS and pair with a 6-digit code. No server, no third party. Devices sync whenever they are on the same network.

### The vault

Passwords and SSH keys are encrypted on your device before they leave it, with a key derived from a **passphrase** that only you know. The server stores ciphertext it cannot read. Set the passphrase on the first device, then enter the same one on every other device. It cannot be recovered: if you lose it, the encrypted items are lost, while hosts and settings stay readable.

The vault key is kept in the system keychain of each device, so you only enter the passphrase once per device. Lock it from Settings → Sync to remove it.

### Synced SSH keys

Keys marked as synced in Settings → SSH keys are sealed in the vault, and every device writes them to its keychain. See [SSH keys](#ssh-keys) to keep one on a single device instead.

### Resuming a workspace

Each device publishes its open tabs, its splits and the tmux session of each pane. The welcome screen of your other devices then offers **Resume tabs from \<device\>**, which reopens the same layout and reattaches the same tmux sessions, so you continue exactly where you left off.

### Pairing on the local network

1. On both devices, set Settings → Sync → Mode to **Local network**.
2. On device A, click **Show a pairing code**. The code is valid for 2 minutes.
3. On device B, click **Pair** next to device A and type the code.
4. Unlock the vault with the same passphrase on both.

Pairing uses SPAKE2: the code is never sent, a wrong code reveals nothing, and three wrong attempts cancel it. Every sync then uses a fresh AES-256-GCM session key derived from the pairing key.

## Sync server (Docker)

`termix-sync` is a single small binary (about 50 MB image, SQLite inside) that stores each user's records and serves a light web interface.

```yaml
# docker-compose.yml (also in sync-server/)
services:
  termix-sync:
    image: ghcr.io/midnights-ra1n/termix-sync:latest
    restart: unless-stopped
    ports:
      - "8080:8080"
    environment:
      TERMIX_ADMIN_USER: admin
      TERMIX_ADMIN_PASSWORD: ""   # empty: a random password is printed once in the logs
    volumes:
      - termix-sync:/data
volumes:
  termix-sync:
```

```sh
docker compose up -d
docker compose logs termix-sync   # first start: the admin password, if you did not set one
```

| Variable | Default | |
|---|---|---|
| `TERMIX_LISTEN` | `0.0.0.0:8080` | listen address |
| `TERMIX_DB` | `/data/termix-sync.db` | SQLite database |
| `TERMIX_ADMIN_USER` | `admin` | first administrator, created when there is no user yet |
| `TERMIX_ADMIN_PASSWORD` | random | its password, 8 characters or more |

Then in Termix: Settings → Sync → Termix server, enter the server address, your user and password, and unlock the vault.

**Web interface** (open the server address in a browser):

- **Hosts**: add, edit and delete hosts, set their password
- **SSH keys**: add, delete, copy the public key
- **Account**: change your sign-in password, see and sign out your devices
- **Users** (administrators): add users, reset passwords, grant or remove admin rights, delete users

Passwords and keys are decrypted in the browser with WebCrypto after you enter your vault passphrase. The page loads nothing from outside the server.

**Put the server behind HTTPS** (Caddy, Traefik, nginx…) before you expose it beyond your local network. Sign-in passwords and session tokens travel in the clear over plain HTTP, even though vault contents stay encrypted.

Backups: copy the `/data` volume. Images are tagged `stable` (and `latest`), `beta` and `nightly`, for `linux/amd64` and `linux/arm64`.

Build it yourself:

```sh
docker build -f sync-server/Dockerfile -t termix-sync .
# or without Docker
cargo run --release --manifest-path sync-server/Cargo.toml
```

### API

All endpoints take and return JSON. Authenticated ones need `Authorization: Bearer <token>`.

| Method | Path | |
|---|---|---|
| `POST` | `/api/login` | `{user, password, device}` → `{token, salt, admin}` |
| `POST` | `/api/logout` | revoke the current token |
| `GET` | `/api/me` | user, vault salt, signed-in devices |
| `POST` | `/api/me/password` | `{old, new}` |
| `DELETE` | `/api/devices/{id}` | sign a device out |
| `GET` | `/api/records?after=<seq>` | records changed since `seq` → `{records, seq}` |
| `POST` | `/api/records` | `{records: [{key, v, ts, dev}]}`, newest `(ts, dev)` wins, `v: null` deletes |
| `GET` `POST` | `/api/users` | list, create (admin) |
| `PATCH` `DELETE` | `/api/users/{id}` | `{password?, admin?}`, delete (admin) |
| `GET` | `/healthz` | `ok` |

Record keys: `host:<id>`, `folder:<id>`, `settings`, `secret:<host id>`, `secret:<host id>:passphrase`, `key:<id>`, `workspace:<device id>`, `vault:check`. Encrypted values are `{"enc": base64(nonce ‖ ciphertext ‖ tag)}`, AES-256-GCM with a key from PBKDF2-HMAC-SHA256 (600,000 rounds, per-account salt).

## Termix in a browser (Docker)

The desktop app can also run in a virtual desktop served by noVNC, to reach your servers from any browser:

```sh
docker compose up -d          # ghcr.io/midnights-ra1n/termix:stable (amd64, arm64)
docker compose up -d --build  # or build from source
```

Open `http://<server>:5800`.

- Settings, hosts and `known_hosts` live in the `/config` volume.
- SSH agent: mount the host's socket (`${SSH_AUTH_SOCK}:/ssh-agent`) and set `SSH_AUTH_SOCK=/ssh-agent` (see `docker-compose.yml`). The socket must be readable by `USER_ID`.
- There is no system keychain in the container, so passwords are not remembered: use the agent or a key in `/config`.
- On a network, turn on `WEB_AUTHENTICATION` or put it behind an authenticating reverse proxy: the page gives access to your servers.
- The `stable`, `beta` and `nightly` tags follow the branches. Update with `docker compose pull`.

## Security and privacy

- **No telemetry, no analytics, no tracking.** Termix only connects to your SSH servers, your sync server or paired devices if you set them up, and GitHub to check for updates (which you can turn off). Fonts and icons are bundled.
- Passwords and key passphrases go to the system keychain (macOS Keychain, Secret Service / GNOME Keyring / KWallet, Windows Credential Manager), never to a file.
- Host keys are checked against `known_hosts`. A changed key blocks the connection.
- Synced secrets are end-to-end encrypted, see [The vault](#the-vault). The server stores sign-in passwords as Argon2 hashes and session tokens as SHA-256 hashes.
- The sync web interface sends a strict Content Security Policy and loads nothing from third parties.

Found a vulnerability? Please open a private security advisory on GitHub rather than a public issue.

## Configuration files

| File | Contents |
|---|---|
| `config.toml` | appearance, terminal, behavior, SSH agent, shortcuts, custom themes, sync mode |
| `hosts.json` | folders and hosts, without any secret |
| `keys/` | imported private keys (mode 0600) |
| `sync.json` | sync state: records, paired devices |

Location: `~/Library/Application Support/termix/` on macOS, `~/.config/termix/` on Linux, `%APPDATA%\termix\` on Windows. Every option is documented in [docs/CONFIG.md](docs/CONFIG.md) (French).

## Development

Requirements: Rust (stable), Node.js 20+, pnpm. On Linux: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`.

```sh
pnpm install
pnpm tauri dev                                         # run the app
pnpm tauri build                                       # build installers
cargo test --manifest-path src-tauri/Cargo.toml        # app tests
cargo test --manifest-path sync-server/Cargo.toml      # server tests
node scripts/check-i18n.mjs                            # every language has every string
```

| Path | |
|---|---|
| `src/` | interface (vanilla JS, xterm.js) |
| `src-tauri/src/` | app backend: SSH (`ssh.rs`), settings (`config.rs`), sync (`sync.rs`, `syncd.rs`, `p2p.rs`) |
| `sync-server/` | `termix-sync` server and its web interface |
| `docs/` | configuration reference |
| `ios/` | native iOS and iPadOS app: SwiftUI interface (`Termix/`), Rust core as a static library (`core/`) |

### iPhone and iPad

A native app in `ios/` (iOS 26 or later), separate from the Tauri project. The interface is SwiftUI with [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) for the terminal; the logic is the desktop's Rust: `ios/core` compiles `config.rs`, `ssh.rs`, `sync.rs`, `p2p.rs` and `syncd.rs` unchanged into a static library behind a three-function C API (`ios/Termix/Bridging.h`), with `ios/core/tauri-shim` standing in for the few Tauri items `syncd.rs` uses. Same hosts file, keychain secrets and sync format, so a phone joins your sync account like another computer.

Requirements: Xcode, `brew install xcodegen`, `rustup target add aarch64-apple-ios`. Xcode runs `cargo` itself during the build (`ios/core/build.sh`).

```sh
cd ios && xcodegen                                      # Termix.xcodeproj from project.yml
open Termix.xcodeproj                                   # choose your team under Signing & Capabilities, then Run on the iPhone
xcodebuild -project Termix.xcodeproj -scheme Termix -destination generic/platform=iOS \
  -skipPackagePluginValidation CODE_SIGNING_ALLOWED=NO build    # compile check (CI)
node scripts/ios-themes.mjs                             # after a change to src/themes.js
```

The first build in Xcode asks you to trust SwiftTerm's build plugin. The theme syncs; the terminal font and size belong to each device. SFTP downloads land in Files › Termix. Not on iOS: SSH agents, splits, peer-to-peer sync on the local network (it needs Apple's multicast entitlement), and shells running in the background: iOS suspends the app about 30 seconds after you leave it. Lost connections reconnect when you come back, and with tmux (Settings → Terminal, or per host) the shell keeps running on the server and is resumed as it was.

In the terminal, tap the title to switch sessions, pinch to change the text size, and use the key bar (Esc, Ctrl and Alt for the next key, Tab, arrows that repeat when held, ^C, ^D, ^Z, symbols, PgUp/PgDn, F1-F12, paste). The panel button (top right) opens the server panel: a column on iPad, a sheet on iPhone. The host list shows each host's status, your recent sessions and the sessions open on your computers (**Resume the sessions of …**, tmux included); your iPhone's sessions show up on the computer the same way.

iPad keyboard shortcuts: ⌘N new host, ⌘K quick connect, ⌘T new session on the same host, ⌘⇧W close the session, ⌘] / ⌘[ next / previous session, ⌘I server panel, ⌘= / ⌘- / ⌘0 text size.

## Branches and releases

| Branch | Purpose | Release |
|---|---|---|
| `dev` | development (default) | CI only |
| `nightly` | experimental, very unstable | `nightly` pre-release, daily when there are commits |
| `beta` | unstable | `beta` pre-release on every push |
| `stable` | stable | `v<version>` release on every push |

Each push to `nightly`, `beta` or `stable` also publishes the Docker images (`termix`, `termix-sync`) with the branch name as tag.

Required GitHub secret: `TAURI_SIGNING_PRIVATE_KEY` (updater signing key, its public half is in `src-tauri/tauri.conf.json`).
