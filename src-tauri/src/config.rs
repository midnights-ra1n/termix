//! Settings (config.toml, documented in docs/CONFIG.md), hosts (hosts.json) and secrets (OS keychain).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const UI_FONT: &str = "\"IBM Plex Sans\", system-ui, sans-serif";
const TERM_FONT: &str = "\"IBM Plex Mono\", Menlo, Consolas, monospace";

/// Settings still on the former Geist defaults (fonts that were not bundled) move to IBM Plex.
pub fn migrate_fonts(s: &mut Settings) {
    // Exact former defaults only: Geist Sans / Geist Mono are bundled again and can be picked.
    if s.appearance.ui_font == "Geist, system-ui, sans-serif" {
        s.appearance.ui_font = UI_FONT.into();
    }
    if s.terminal.font_family == "\"Geist Mono\", \"JetBrains Mono\", Menlo, Consolas, monospace" {
        s.terminal.font_family = TERM_FONT.into();
    }
}

fn device_name() -> String {
    let h = std::process::Command::new("hostname").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    h.filter(|s| !s.is_empty()).unwrap_or_else(|| std::env::consts::OS.into())
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
    pub performance: Performance,
    pub window: WindowStyle,
    pub behavior: Behavior,
    pub ssh: Ssh,
    /// action -> accelerator ("CmdOrCtrl+Shift+D"). Missing actions fall back to defaults.
    pub keybindings: BTreeMap<String, String>,
    /// User themes: name -> { key: "#rrggbb" }, same keys as the built-in themes.
    pub themes: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    /// Device-specific: never synchronised itself.
    pub sync: Sync,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Sync {
    /// off | server | p2p
    pub mode: String,
    pub server_url: String,
    pub user: String,
    /// Stable id of this device (records, workspaces, pairing).
    pub device_id: String,
    pub device_name: String,
    /// Seconds between two syncs (5 = live). 0 = only at launch and after a local change.
    pub interval_secs: u32,
    /// synced (adapted to each system's keyboard) | local (this device keeps its own shortcuts)
    pub keybindings: String,
}

impl Default for Sync {
    fn default() -> Self {
        Self { mode: "off".into(), server_url: String::new(), user: String::new(), device_id: String::new(), device_name: String::new(), interval_secs: 30, keybindings: "synced".into() }
    }
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
    /// Widths in px, set by dragging the panels' inner edge.
    pub sidebar_width: u32,
    pub panel_width: u32,
    /// Overrides the theme accent (#RRGGBB), empty = theme accent. Superseded by `colors.accent`.
    pub accent: String,
    /// Overrides any theme key (accent, bg, scene, ink, term_bg, term_fg, cursor, selection…) with "#RRGGBB".
    pub colors: BTreeMap<String, String>,
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
    /// Clicking the sidebar or the side panel gives the keyboard back to the terminal.
    pub keep_focus: bool,
    /// normal | 500 | bold
    pub font_weight: String,
    pub letter_spacing: f32,
    /// Inner padding of terminals, in px.
    pub padding: u32,
    /// Bold text in the bright ANSI colours (classic xterm behaviour).
    pub bold_is_bright: bool,
    /// Show the focus border when the tab has a single pane (it always shows once split).
    pub focus_single: bool,
    /// Focus border width, in px.
    pub focus_width: u32,
    /// Background colour opacity, 0-100. Below 100 the background image, or the desktop, shows through.
    pub background_opacity: u32,
    /// Blur of the background image, in px (the desktop blur is the window material).
    pub background_blur: u32,
    /// Imported image ("<hash>.<ext>" in <config>/backgrounds, synced) or a path on this machine; empty = none.
    pub background_image: String,
    pub background_image_opacity: u32,
    /// cover | contain | tile | center | stretch
    pub background_image_fit: String,
    /// left | center | right
    pub background_image_x: String,
    /// top | center | bottom
    pub background_image_y: String,
}

/// Window material: the system's blur or glass behind the whole window. Device-specific (never
/// synchronised), since each system has its own materials.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct WindowStyle {
    /// none | macOS: vibrancy, liquid_glass | Windows: acrylic, mica | Linux (KDE Plasma): blur
    pub material: String,
    /// Opacity of the interface (side bars, title bar) over the material, 0-100.
    pub interface_opacity: u32,
}

impl Default for WindowStyle {
    fn default() -> Self {
        Self { material: "none".into(), interface_opacity: 75 }
    }
}

impl WindowStyle {
    /// The material, if it exists on this system.
    pub fn active(&self) -> Option<&str> {
        let ok: &[&str] = if cfg!(target_os = "macos") { &["vibrancy", "liquid_glass"] } else if cfg!(windows) { &["acrylic", "mica"] } else { &["blur"] };
        ok.contains(&self.material.as_str()).then_some(self.material.as_str())
    }
}

/// Device-specific (never synchronised): depends on the machine, not on taste.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Performance {
    /// WebGL terminal renderer (GPU); off = DOM renderer.
    pub gpu: bool,
    /// Interface transitions and animations.
    pub animations: bool,
    /// Seconds between two host status checks; 0 = only on demand (Hosts → Refresh).
    pub probe_secs: u32,
    /// Seconds between two refreshes of the Stats panel.
    pub stats_secs: u32,
}

impl Default for Performance {
    fn default() -> Self {
        Self { gpu: true, animations: true, probe_secs: 30, stats_secs: 3 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Behavior {
    pub tray_icon: bool,
    pub run_in_background: bool,
    /// Several Termix processes at once; when off, launching it again shows the running window. Read at startup.
    pub multi_instance: bool,
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
            performance: Performance::default(),
            window: WindowStyle::default(),
            behavior: Behavior::default(),
            ssh: Ssh { agent_socket: String::new(), keepalive_secs: 30 },
            keybindings: BTreeMap::new(),
            themes: BTreeMap::new(),
            sync: Sync::default(),
        }
    }
}
impl Default for Appearance {
    fn default() -> Self {
        Self { language: "auto".into(), theme: "termix".into(), ui_font: UI_FONT.into(), ui_font_size: 13, sidebar: true, right_panel: true, sidebar_width: 248, panel_width: 300, accent: String::new(), colors: BTreeMap::new(), density: "comfortable".into() }
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
            tmux: false,
            keep_focus: true,
            font_weight: "normal".into(),
            letter_spacing: 0.0,
            padding: 8,
            bold_is_bright: false,
            focus_single: false,
            focus_width: 2,
            background_opacity: 100,
            background_blur: 0,
            background_image: String::new(),
            background_image_opacity: 100,
            background_image_fit: "cover".into(),
            background_image_x: "center".into(),
            background_image_y: "center".into(),
        }
    }
}
impl Default for Behavior {
    fn default() -> Self {
        Self { tray_icon: true, run_in_background: true, multi_instance: false, check_updates: true, update_channel: "stable".into(), tray_hosts: "favorites".into(), tray_count: 5 }
    }
}

/// Defaults use Cmd on macOS and Ctrl+Shift elsewhere so plain Ctrl keys keep reaching the remote shell.
pub fn default_keybindings() -> BTreeMap<String, String> {
    [
        ("new_tab", "T"),
        ("quick_connect", "O"),
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
        ("clear_screen", "Alt+K"), // Cmd+Option+K like macOS Terminal
    ]
    .into_iter()
    .map(|(a, k)| (a.to_string(), crate::sync::native_key(&format!("Mod+{k}"), cfg!(target_os = "macos"))))
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
        if s.sync.mode.is_empty() {
            s.sync.mode = "off".into();
        }
        if s.sync.device_id.is_empty() {
            s.sync.device_id = uuid::Uuid::new_v4().to_string();
            let _ = s.save();
        }
        if s.sync.device_name.is_empty() {
            s.sync.device_name = device_name();
        }
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
    /// Key stored in the sync vault (`key:<id>` record), used when no key file is set.
    pub key_id: String,
    pub description: String,
    pub icon: String,
    pub tags: Vec<String>,
    pub folder: Option<String>,
    pub favorite: bool,
    /// tmux for this host: "" = the global setting, "on" = always, "off" = never.
    pub tmux: String,
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

// Each keychain read of an ad-hoc signed app asks the user again after every update, so secrets are
// read at most once per launch, and on macOS all live in one item ("secrets"): one prompt, not one per host.
// Elsewhere one item per secret stays (Windows caps an item at 2.5 KB).
const SINGLE: bool = cfg!(target_os = "macos");
static SECRETS: std::sync::Mutex<Option<std::collections::HashMap<String, Option<String>>>> = std::sync::Mutex::new(None);
static DENIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn entry(id: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new("termix", id)
}
fn read(e: keyring::Result<keyring::Entry>) -> Option<String> {
    if denied() {
        return None;
    }
    match e.and_then(|e| e.get_password()) {
        Ok(v) => Some(v),
        Err(keyring::Error::NoEntry) => None,
        Err(_) => {
            DENIED.store(true, std::sync::atomic::Ordering::Relaxed);
            None
        }
    }
}
/// The user refused keychain access this launch: secrets read as missing must not be taken as deleted.
pub fn denied() -> bool {
    DENIED.load(std::sync::atomic::Ordering::Relaxed)
}
fn with_secrets<T>(f: impl FnOnce(&mut std::collections::HashMap<String, Option<String>>) -> T) -> T {
    let mut g = SECRETS.lock().unwrap();
    let m = g.get_or_insert_with(|| {
        let all = if SINGLE { read(entry("secrets")) } else { None };
        let all: BTreeMap<String, String> = all.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        all.into_iter().map(|(k, v)| (k, Some(v))).collect()
    });
    f(m)
}
fn persist(m: &std::collections::HashMap<String, Option<String>>) -> anyhow::Result<()> {
    if denied() {
        anyhow::bail!("accès au trousseau refusé : relance Termix et autorise-le");
    }
    let all: BTreeMap<&String, &String> = m.iter().filter_map(|(k, v)| Some((k, v.as_ref()?))).collect();
    Ok(entry("secrets")?.set_password(&serde_json::to_string(&all)?)?)
}
/// `id` is "<host_id>" for the password or "<host_id>:passphrase" for the key passphrase.
pub fn secret_get(id: &str) -> Option<String> {
    with_secrets(|m| {
        if let Some(v) = m.get(id) {
            return v.clone();
        }
        // Per-secret items: the storage outside macOS, and the former layout there (Termix, then RustTerm).
        let own = read(entry(id));
        let old = if own.is_none() { read(keyring::Entry::new("rustterm", id)) } else { None };
        let v = own.clone().or(old.clone());
        m.insert(id.into(), v.clone());
        if v.is_some() && (SINGLE || old.is_some()) {
            let moved = if SINGLE { persist(m) } else { entry(id).and_then(|e| e.set_password(v.as_deref().unwrap())).map_err(Into::into) };
            if moved.is_ok() {
                let _ = keyring::Entry::new(if own.is_some() { "termix" } else { "rustterm" }, id).and_then(|e| e.delete_credential());
            }
        }
        v
    })
}
pub fn secret_set(id: &str, value: &str) -> anyhow::Result<()> {
    with_secrets(|m| {
        let v = (!value.is_empty()).then(|| value.to_string());
        if SINGLE {
            let before = m.insert(id.into(), v);
            let r = persist(m);
            if r.is_err() {
                match before {
                    Some(b) => m.insert(id.into(), b),
                    None => m.remove(id),
                };
            }
            return r;
        }
        match &v {
            Some(v) => entry(id)?.set_password(v)?,
            None => match entry(id)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(e.into()),
            },
        }
        m.insert(id.into(), v);
        Ok(())
    })
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
