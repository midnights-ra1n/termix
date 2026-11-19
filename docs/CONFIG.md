# Configuration de RustTerm

Tout ce qui est réglable dans **Réglages** (⌘, sur macOS, Ctrl+Shift+, ailleurs) l'est aussi à la main.

| Fichier | Contenu |
|---|---|
| `config.toml` | apparence, terminal, comportement, agent SSH, raccourcis, thèmes perso |
| `hosts.json` | dossiers et hôtes (sans aucun secret) |
| `keys/` | clés privées importées (droits 0600) |

Emplacement du dossier :

- macOS : `~/Library/Application Support/rustterm/`
- Linux : `~/.config/rustterm/`
- Windows : `%APPDATA%\rustterm\`

Les **mots de passe** et **phrases de passe** des clés ne sont jamais écrits sur disque par RustTerm : ils vont dans le trousseau du système (Trousseau macOS, Secret Service/GNOME Keyring/KWallet, Gestionnaire d'identifiants Windows).

Les clés absentes du fichier gardent leur valeur par défaut ; un fichier invalide est ignoré (message sur la sortie d'erreur) et les valeurs par défaut sont utilisées. Le fichier est réécrit quand on enregistre depuis les Réglages : les commentaires ne sont donc pas conservés. Relancez l'app après une modification manuelle.

## Exemple complet

```toml
[appearance]
theme = "midnights"          # midnights, aube, nord, dracula, solarized, gruvbox, system, ou un thème perso
ui_font = "Geist, system-ui, sans-serif"
ui_font_size = 13
sidebar = true               # barre latérale des hôtes
right_panel = true           # panneau SFTP + stats

[terminal]
font_family = "\"Geist Mono\", \"JetBrains Mono\", Menlo, Consolas, monospace"
font_size = 13
line_height = 1.2
cursor_style = "block"       # block, bar, underline
cursor_blink = true
scrollback = 10000
copy_on_select = false

[behavior]
tray_icon = true             # icône barre des menus (macOS) / zone de notification (GNOME, KDE, Windows)
run_in_background = true     # fermer la fenêtre ne quitte pas l'app
check_updates = true         # au lancement puis toutes les 24 h
update_channel = "stable"    # stable, beta, nightly

[ssh]
agent_socket = ""            # vide = détection auto, sinon chemin du socket / named pipe
keepalive_secs = 30          # 0 = désactivé

[keybindings]
split_right = "Cmd+D"
split_down = "Cmd+Shift+D"
# ...

[themes.ma-nuit]
label = "Ma nuit"
base = "midnights"           # hérite des couleurs non précisées
bg = "#101020"
accent = "#87D7F7"
cursor = "#87D7F7"
```

## Agent SSH et gestionnaires de mots de passe

RustTerm essaie, dans l'ordre : **agent SSH → fichier de clé → mot de passe** (enregistré ou demandé).

Avec `agent_socket` vide, l'agent est trouvé ainsi :

1. `IdentityAgent` de la section globale de `~/.ssh/config` ;
2. `SSH_AUTH_SOCK` de l'environnement, sinon celui de votre shell de connexion (`$SHELL -l -i`), ce qui récupère ce que vos `.zshrc`, `.bashrc` ou `config.fish` exportent ;
3. Windows : `\\.\pipe\openssh-ssh-agent`.

Ça couvre Bitwarden, 1Password, KeePassXC, Proton Pass, Secretive, GNOME Keyring ou `ssh-agent`, sans rien installer. Les Réglages listent les sockets détectés pour en choisir un explicitement.

Les clés d'hôte sont vérifiées dans `~/.ssh/known_hosts` (même politique que `StrictHostKeyChecking=accept-new` : un hôte inconnu est appris, une clé modifiée est refusée).

## Raccourcis

Format : modificateurs + touche, séparés par `+`.
Modificateurs : `Cmd` (macOS), `Ctrl`, `Alt` (= Option sur macOS), `Shift`, `Super`, `CmdOrCtrl` (Cmd sur macOS, Ctrl ailleurs).
Touches : lettres, chiffres, `[ ] = - , . /`, `Left Right Up Down`, `F1`…`F12`, `Enter`, `Tab`.

Par défaut, macOS utilise `Cmd+…` et Linux/Windows `Ctrl+Shift+…`, pour que les `Ctrl+lettre` restent envoyés au shell distant. Les lettres suivent la disposition du clavier (AZERTY, QWERTZ…).
Copier/coller : `Cmd+C`/`Cmd+V` sur macOS, `Ctrl+Shift+C`/`Ctrl+Shift+V` ailleurs.

| Action | macOS | Linux / Windows |
|---|---|---|
| `new_host` | ⌘N | Ctrl+Shift+N |
| `close_pane` | ⌘W | Ctrl+Shift+W |
| `split_right` / `split_left` | ⌘D / ⌘L | Ctrl+Shift+D / L |
| `split_down` / `split_up` | ⌘E / ⌘U | Ctrl+Shift+E / U |
| `focus_next` / `focus_prev` | ⌘] / ⌘[ | Ctrl+Shift+] / [ |
| `next_tab` / `prev_tab` | ⌘→ / ⌘← | Ctrl+Shift+→ / ← |
| `search_hosts` | ⌘K | Ctrl+Shift+K |
| `toggle_sidebar` / `toggle_panel` | ⌘B / ⌘I | Ctrl+Shift+B / I |
| `settings` | ⌘, | Ctrl+Shift+, |
| `zoom_in` / `zoom_out` / `zoom_reset` | ⌘= / ⌘- / ⌘0 | Ctrl+Shift+= / - / 0 |

## Thèmes

Clés d'un thème : `bg scene vide indigo brume ink mute faint accent accent2 success error warn onAccent cursor selection`, plus la palette ANSI `black red green yellow blue magenta cyan white` et leurs variantes `brightBlack` … `brightWhite`. `dark = false` pour un thème clair.
Le thème `system` suit le mode clair/sombre et la couleur d'accent du système.
