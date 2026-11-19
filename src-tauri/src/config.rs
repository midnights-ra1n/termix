//! Settings (config.toml, documented in docs/CONFIG.md), hosts (hosts.json) and secrets (OS keychain).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn dir() -> PathBuf {
    let d = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("rustterm");
    let _ = std::fs::create_dir_all(&d);
    d
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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Behavior {
    pub tray_icon: bool,
    pub run_in_background: bool,
    pub check_updates: bool,
    /// stable | beta | nightly
    pub update_channel: String,
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
        Self { language: "auto".into(), theme: "midnights".into(), ui_font: "Geist, system-ui, sans-serif".into(), ui_font_size: 13, sidebar: true, right_panel: true }
    }
}
impl Default for Terminal {
    fn default() -> Self {
        Self {
            font_family: "\"Geist Mono\", \"JetBrains Mono\", Menlo, Consolas, monospace".into(),
            font_size: 13,
            line_height: 1.2,
            cursor_style: "block".into(),
            cursor_blink: true,
            scrollback: 10000,
            copy_on_select: false,
            suggestions: true,
        }
    }
}
impl Default for Behavior {
    fn default() -> Self {
        Self { tray_icon: true, run_in_background: true, check_updates: true, update_channel: "stable".into() }
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

fn entry(id: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new("rustterm", id)
}
/// `id` is "<host_id>" for the password or "<host_id>:passphrase" for the key passphrase.
pub fn secret_get(id: &str) -> Option<String> {
    entry(id).ok()?.get_password().ok()
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
    fn partial_config_keeps_defaults() {
        let s: Settings = toml::from_str("[terminal]\nfont_size = 16\n").unwrap();
        assert_eq!(s.terminal.font_size, 16);
        assert_eq!(s.terminal.scrollback, 10000);
        assert_eq!(s.appearance.theme, "midnights");
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
