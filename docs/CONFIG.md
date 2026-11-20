# Configuration de Termix

Tout ce qui est réglable dans **Réglages** (⌘, sur macOS, Ctrl+Shift+, ailleurs) l'est aussi à la main.

| Fichier | Contenu |
|---|---|
| `config.toml` | apparence, terminal, comportement, agent SSH, raccourcis, thèmes perso |
| `hosts.json` | dossiers et hôtes (sans aucun secret) |
| `keys/` | clés privées importées (droits 0600) |

Emplacement du dossier :

- macOS : `~/Library/Application Support/termix/`
- Linux : `~/.config/termix/`
- Windows : `%APPDATA%\termix\`

Les **mots de passe** et **phrases de passe** des clés ne sont jamais écrits sur disque par Termix : ils vont dans le trousseau du système (Trousseau macOS, Secret Service/GNOME Keyring/KWallet, Gestionnaire d'identifiants Windows).

Les clés absentes du fichier gardent leur valeur par défaut ; un fichier invalide est ignoré (message sur la sortie d'erreur) et les valeurs par défaut sont utilisées. Le fichier est réécrit quand on enregistre depuis les Réglages : les commentaires ne sont donc pas conservés. Relancez l'app après une modification manuelle.

## Exemple complet

```toml
[appearance]
language = "auto"            # auto (langue du système), fr, en, es, de
theme = "termix"             # termix, midnights, aube, nord, dracula, solarized, solarizedlight, gruvbox, gruvboxlight,
                             # catppuccin, latte, tokyonight, onedark, rosepine, everforest, kanagawa, github, monokai,
                             # ayu, nightowl, tomorrow, palenight, poimandres, system, ou un thème perso
# Polices intégrées : IBM Plex Sans, Inter, Geist Sans, Source Sans 3, Atkinson Hyperlegible (interface) ;
# IBM Plex Mono, JetBrains Mono, Fira Code, Geist Mono, Source Code Pro, Cascadia Code (terminal).
# Toute police installée sur le système marche aussi (pile CSS).
ui_font = "\"IBM Plex Sans\", system-ui, sans-serif"
ui_font_size = 13
sidebar = true               # barre latérale des hôtes
right_panel = true           # panneau Stats, SFTP, Docker, tmux
accent = ""                  # ancien réglage, remplacé par [appearance.colors] accent
density = "comfortable"      # comfortable, compact

[appearance.colors]          # remplace des couleurs du thème actif ("#RRGGBB"), clé absente = couleur du thème
# accent = "#C3BAFE"         # boutons, focus, jauges
# bg = "#1A2030"             # fond de l'interface
# scene = "#20283A"          # panneaux et barres
# ink = "#EEEEEE"            # texte de l'interface
# term_bg = "#101420"        # fond du terminal (par défaut = bg)
# term_fg = "#E0E0E0"        # texte du terminal (par défaut = ink)
# cursor = "#9CCBA9"
# selection = "#48536C"

[terminal]
font_family = "\"IBM Plex Mono\", Menlo, Consolas, monospace"
font_size = 13
line_height = 1.2
cursor_style = "block"       # block, bar, underline
cursor_blink = true
scrollback = 10000
copy_on_select = false
suggestions = true           # prédiction des commandes (texte grisé)
tmux = false                 # shell dans tmux (si installé sur le serveur) : survit à la déconnexion
                             # par hôte, "tmux": "on" / "off" dans hosts.json l'emporte ; vide = ce réglage
keep_focus = true            # un clic dans la liste des hôtes ou le panneau de droite garde la saisie dans le terminal
font_weight = "normal"       # normal, 500, bold
letter_spacing = 0           # en px, peut être négatif
padding = 8                  # marge intérieure des terminaux, en px
bold_is_bright = false       # texte gras dans les couleurs ANSI vives
focus_single = false         # bordure de focus aussi quand l'onglet n'a qu'une session (toujours visible une fois divisé)
focus_width = 2              # épaisseur de la bordure de focus, en px (1 à 6)

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

Termix essaie, dans l'ordre : **agent SSH → fichier de clé → mot de passe** (enregistré ou demandé).

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

Clés d'un thème : `bg scene vide indigo brume ink mute faint accent accent2 success error warn onAccent cursor selection term_bg term_fg`, plus la palette ANSI `black red green yellow blue magenta cyan white` et leurs variantes `brightBlack` … `brightWhite`. `dark = false` pour un thème clair.
Le thème `system` suit le mode clair/sombre et la couleur d'accent du système.

## Statut des hôtes

Toutes les 30 s (et à chaque modification des hôtes), Termix vérifie chaque hôte :

- **vert** : le port SSH répond avec une bannière `SSH-` ;
- **jaune** : l'hôte répond au ping mais pas le SSH ;
- **rouge** : ni ping ni SSH.

Le statut s'affiche dans la liste des hôtes et dans le menu de la barre des menus / zone de notification. Le ping utilise la commande `ping` du système : un hôte qui bloque l'ICMP mais dont le SSH répond reste vert.

## Suggestions de commandes

Pendant la saisie, Termix affiche en grisé la suite la plus probable de la commande, tirée de l'historique du serveur (`~/.bash_history`, `~/.zsh_history`, historique fish, lus à la connexion) et de vos commandes passées sur cet hôte (stockées localement).

- `→` ou `Fin` : accepter la suggestion
- `⌥→` / `Alt+→` : accepter le mot suivant
- `⇧Tab` : liste des suggestions (`↑` `↓` pour choisir, `Entrée` pour valider, `Échap` pour fermer)

`Tab` reste envoyé au shell distant pour sa propre complétion. Les suggestions sont désactivées dans les applications plein écran (vim, htop…) et quand la ligne n'a pas de prompt reconnu (`$ # % > ❯ ➜` ou séparateur Powerline).

## Synchronisation

Se règle dans **Réglages → Synchronisation** (voir le [README](../README.md#sync) pour le fonctionnement). Section propre à chaque appareil, jamais synchronisée :

```toml
[sync]
mode = "off"            # off | server | p2p (réseau local)
server_url = ""         # ex. https://sync.example.com
user = ""
device_id = "…"         # généré au premier lancement, ne pas modifier
device_name = "MacBook" # nom affiché sur les autres appareils
interval_secs = 30      # 5 = en direct ; 0 = seulement au lancement et après une modification
```

Une synchro a lieu à chaque lancement, après chaque modification locale, puis toutes les `interval_secs` secondes. L'icône nuage de la barre du haut en donne l'état (à jour, en cours, erreur, conflits) ; un clic synchronise, ou ouvre la page Synchronisation en cas de problème.

**Conflits** : quand un hôte, un dossier ou les réglages ont été modifiés sur deux appareils entre deux synchros, la version la plus récente est appliquée et le conflit apparaît dans Réglages → Synchronisation avec les différences. « Garder la mienne » republie la version de cet appareil sur tous les autres.

L'état de la synchro (enregistrements, appareils appairés) est dans `sync.json`. Le jeton du serveur, la clé du coffre, les clés d'appairage et les clés SSH synchronisées sont dans le trousseau du système. Supprimer `sync.json` repart d'un état vierge : les hôtes locaux sont renvoyés à la synchro suivante.
