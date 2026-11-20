//! Settings (config.toml, documented in docs/CONFIG.md), hosts (hosts.json) and secrets (OS keychain).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const UI_FONT: &str = "\"IBM Plex Sans\", system-ui, sans-serif";
const TERM_FONT: &str = "\"IBM Plex Mono\", Menlo, Consolas, monospace";

/// Geist is no longer bundled: settings still on its former defaults move to IBM Plex.
pub fn migrate_fonts(s: &mut Settings) {
    if s.appearance.ui_font.starts_with("Geist") {
        s.appearance.ui_font = UI_FONT.into();
    }
    if s.terminal.font_family.starts_with("\"Geist Mono\"") {
        s.terminal.font_family = TERM_FONT.into();
    }
}

pub fn dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let d = base.join("termix");
    migrate_dir(&base.join("rustterm"), &d);
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The app was called RustTerm: take its settings and hosts over once, the first time Termix starts.
fn migrate_dir(old: &std::path::Path, new: &std::path::Path) {
    if new.exists() || !old.is_dir() {
        return;
    }
    if std::fs::rename(old, new).is_err() {
        // Different volume: copy the (flat) directory instead.
        if std::fs::create_dir_all(new).is_ok() {
            for e in std::fs::read_dir(old).into_iter().flatten().flatten() {
                let _ = std::fs::copy(e.path(), new.join(e.file_name()));
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    pub terminal: Terminal,
    pub behavior: Behavior,
    pub ssh: Ssh,
    /// action -> accelerator ("CmdOrCtrl+Shift+D"). Missing actions fall back to defaults.
    pub keybindings: BTreeMap<String, String>,
    /// User themes: name -> { key: "#rrggbb" }, same keys as the built-in themes.
    pub themes: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Appearance {
    /// auto | fr | en | es | de
    pub language: String,
    pub theme: String,
    pub ui_font: String,
    pub ui_font_size: u32,
    pub sidebar: bool,
    pub right_panel: bool,
    /// Overrides the theme accent (#RRGGBB), empty = theme accent.
    pub accent: String,
    /// comfortable | compact
    pub density: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Terminal {
    pub font_family: String,
    pub font_size: u32,
    pub line_height: f32,
    pub cursor_style: String,
    pub cursor_blink: bool,
    pub scrollback: u32,
    pub copy_on_select: bool,
    /// Inline command predictions from history (→ to accept).
    pub suggestions: bool,
    /// Open shells inside tmux (when installed on the server) so jobs survive a disconnect.
    pub tmux: bool,
    /// normal | 500 | bold
    pub font_weight: String,
    pub letter_spacing: f32,
    /// Inner padding of terminals, in px.
    pub padding: u32,
    /// Bold text in the bright ANSI colours (classic xterm behaviour).
    pub bold_is_bright: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Behavior {
    pub tray_icon: bool,
    pub run_in_background: bool,
    pub check_updates: bool,
    /// stable | beta | nightly
    pub update_channel: String,
    /// Hosts listed in the tray menu: favorites | recent | all
    pub tray_hosts: String,
    /// How many hosts the tray menu shows.
    pub tray_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Ssh {
    /// Empty = auto-detect (ssh config IdentityAgent, then $SSH_AUTH_SOCK from the login shell).
    pub agent_socket: String,
    pub keepalive_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            terminal: Terminal::default(),
            behavior: Behavior::default(),
            ssh: Ssh { agent_socket: String::new(), keepalive_secs: 30 },
            keybindings: BTreeMap::new(),
            themes: BTreeMap::new(),
        }
    }
}
impl Default for Appearance {
    fn default() -> Self {
        Self { language: "auto".into(), theme: "termix".into(), ui_font: UI_FONT.into(), ui_font_size: 13, sidebar: true, right_panel: true, accent: String::new(), density: "comfortable".into() }
    }
}
impl Default for Terminal {
    fn default() -> Self {
        Self {
            font_family: TERM_FONT.into(),
            font_size: 13,
            line_height: 1.2,
            cursor_style: "block".into(),
            cursor_blink: true,
            scrollback: 10000,
            copy_on_select: false,
            suggestions: true,
            tmux: true,
            font_weight: "normal".into(),
            letter_spacing: 0.0,
            padding: 8,
            bold_is_bright: false,
        }
    }
}
impl Default for Behavior {
    fn default() -> Self {
        Self { tray_icon: true, run_in_background: true, check_updates: true, update_channel: "stable".into(), tray_hosts: "favorites".into(), tray_count: 5 }
    }
}

/// Defaults use Cmd on macOS and Ctrl+Shift elsewhere so plain Ctrl keys keep reaching the remote shell.
pub fn default_keybindings() -> BTreeMap<String, String> {
    let m = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl+Shift" };
    [
        ("new_host", "N"),
        ("close_pane", "W"),
        ("split_right", "D"),
        ("split_down", "E"),
        ("split_left", "L"),
        ("split_up", "U"),
        ("focus_next", "]"),
        ("focus_prev", "["),
        ("next_tab", "Right"),
        ("prev_tab", "Left"),
        ("search_hosts", "K"),
        ("toggle_sidebar", "B"),
        ("toggle_panel", "I"),
        ("settings", ","),
        ("zoom_in", "="),
        ("zoom_out", "-"),
        ("zoom_reset", "0"),
    ]
    .into_iter()
    .map(|(a, k)| (a.to_string(), format!("{m}+{k}")))
    .collect()
}

impl Settings {
    fn path() -> PathBuf {
        dir().join("config.toml")
    }
    pub fn load() -> Self {
        let mut s: Settings = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| match toml::from_str(&t) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("config.toml invalide, valeurs par défaut utilisées : {e}");
                    None
                }
            })
            .unwrap_or_default();
        migrate_fonts(&mut s);
        for (a, k) in default_keybindings() {
            s.keybindings.entry(a).or_insert(k);
        }
        s
    }
    pub fn save(&self) -> anyhow::Result<()> {
        write_atomic(&Self::path(), toml::to_string_pretty(self)?.as_bytes())
    }
}

/// Write via temp file + rename so a crash never leaves a half-written config.
pub fn write_atomic(path: &PathBuf, data: &[u8]) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Optional private key file; agent and stored password are tried as well.
    pub key_path: String,
    pub description: String,
    pub icon: String,
    pub tags: Vec<String>,
    pub folder: Option<String>,
    pub favorite: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct HostsDoc {
    pub folders: Vec<Folder>,
    pub hosts: Vec<Host>,
}

impl HostsDoc {
    fn path() -> PathBuf {
        dir().join("hosts.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self) -> anyhow::Result<()> {
        write_atomic(&Self::path(), serde_json::to_string_pretty(self)?.as_bytes())
    }
}

/// Most recently connected host ids, newest first (recent.json).
pub fn recent_load() -> Vec<String> {
    std::fs::read_to_string(dir().join("recent.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}
pub fn recent_push(id: &str) {
    let mut v = recent_load();
    v.retain(|x| x != id);
    v.insert(0, id.to_string());
    v.truncate(30);
    if let Ok(json) = serde_json::to_vec(&v) {
        let _ = write_atomic(&dir().join("recent.json"), &json);
    }
}

/// Hosts for the tray menu according to the user's choice; favorites fall back to recent, then to all.
pub fn tray_hosts<'a>(doc: &'a HostsDoc, mode: &str, count: usize, recent: &[String]) -> Vec<&'a Host> {
    let by_recent = || recent.iter().filter_map(|id| doc.hosts.iter().find(|h| &h.id == id)).collect::<Vec<_>>();
    let mut v: Vec<&Host> = match mode {
        "recent" => by_recent(),
        "all" => doc.hosts.iter().collect(),
        _ => doc.hosts.iter().filter(|h| h.favorite).collect(),
    };
    if v.is_empty() && mode != "all" {
        v = by_recent();
    }
    if v.is_empty() {
        v = doc.hosts.iter().collect();
    }
    v.truncate(count.max(1));
    v
}

fn entry(id: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new("termix", id)
}
/// `id` is "<host_id>" for the password or "<host_id>:passphrase" for the key passphrase.
pub fn secret_get(id: &str) -> Option<String> {
    if let Some(v) = entry(id).ok()?.get_password().ok() {
        return Some(v);
    }
    // Secrets saved under the former name (RustTerm) move to the new keychain service on first use.
    let v = keyring::Entry::new("rustterm", id).ok()?.get_password().ok()?;
    let _ = secret_set(id, &v);
    Some(v)
}
pub fn secret_set(id: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty() {
        match entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    } else {
        Ok(entry(id)?.set_password(value)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geist_defaults_move_to_plex() {
        let mut s = Settings::default();
        s.appearance.ui_font = "Geist, system-ui, sans-serif".into();
        s.terminal.font_family = "\"Geist Mono\", \"JetBrains Mono\", Menlo, Consolas, monospace".into();
        migrate_fonts(&mut s);
        assert_eq!((s.appearance.ui_font.as_str(), s.terminal.font_family.as_str()), (UI_FONT, TERM_FONT));
        s.terminal.font_family = "Iosevka".into();
        migrate_fonts(&mut s);
        assert_eq!(s.terminal.font_family, "Iosevka");
    }

    #[test]
    fn rustterm_dir_is_taken_over_once() {
        let base = std::env::temp_dir().join(format!("tx-migrate-{}", std::process::id()));
        let (old, new) = (base.join("rustterm"), base.join("termix"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("hosts.json"), "{}").unwrap();
        migrate_dir(&old, &new);
        assert_eq!(std::fs::read_to_string(new.join("hosts.json")).unwrap(), "{}");
        // Never overwrites an existing Termix directory.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("hosts.json"), "old").unwrap();
        migrate_dir(&old, &new);
        assert_eq!(std::fs::read_to_string(new.join("hosts.json")).unwrap(), "{}");
        let _ = std::fs::remove_dir_all(&base);
    }
    #[test]
    fn partial_config_keeps_defaults() {
        let s: Settings = toml::from_str("[terminal]\nfont_size = 16\n").unwrap();
        assert_eq!(s.terminal.font_size, 16);
        assert_eq!(s.terminal.scrollback, 10000);
        assert_eq!(s.appearance.theme, "termix");
    }
}

#[cfg(test)]
mod tray_tests {
    use super::*;
    fn h(id: &str, fav: bool) -> Host {
        Host { id: id.into(), name: id.into(), favorite: fav, ..Default::default() }
    }
    #[test]
    fn tray_selection() {
        let doc = HostsDoc { folders: vec![], hosts: vec![h("a", false), h("b", true), h("c", false)] };
        let ids = |v: Vec<&Host>| v.iter().map(|h| h.id.clone()).collect::<Vec<_>>();
        let recent = vec!["c".to_string(), "a".to_string()];
        assert_eq!(ids(tray_hosts(&doc, "favorites", 5, &recent)), ["b"]);
        assert_eq!(ids(tray_hosts(&doc, "recent", 1, &recent)), ["c"]);
        assert_eq!(ids(tray_hosts(&doc, "all", 2, &recent)), ["a", "b"]);
        let nofav = HostsDoc { folders: vec![], hosts: vec![h("a", false)] };
        assert_eq!(ids(tray_hosts(&nofav, "favorites", 5, &[])), ["a"]);
    }
}

#[cfg(test)]
mod theme_tests {
    #[test]
    fn user_theme_accepts_bools() {
        let s: super::Settings = toml::from_str("[themes.x]\nbase = \"aube\"\ndark = false\n").unwrap();
        assert_eq!(s.themes["x"]["dark"], serde_json::json!(false));
    }
}
