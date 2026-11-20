mod config;
mod probe;
mod ssh;
mod sync;
mod p2p;
mod syncd;

use config::{HostsDoc, Settings};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::menu::{IconMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_updater::UpdaterExt;

const REPO: &str = "midnights-ra1n/termix";

struct AppState {
    settings: Mutex<Settings>,
    sessions: ssh::Sessions,
    /// Translated menu labels pushed by the UI (key -> text); French built-ins until then.
    labels: Mutex<HashMap<String, String>>,
    /// host id -> up | ping | down
    status: Mutex<HashMap<String, String>>,
    probe_now: tokio::sync::Notify,
    shown: std::sync::atomic::AtomicBool,
}

fn lbl(app: &AppHandle, key: &str, fallback: &str) -> String {
    app.state::<AppState>().labels.lock().unwrap().get(key).cloned().unwrap_or_else(|| fallback.to_string())
}

type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

// ---------- settings / hosts ----------

#[derive(serde::Serialize)]
struct Boot {
    settings: Settings,
    defaults: std::collections::BTreeMap<String, String>,
    platform: &'static str,
    arch: &'static str,
    version: String,
    config_dir: String,
    /// XDG_CURRENT_DESKTOP (Linux), for the window corner radius.
    desktop: String,
}

#[tauri::command]
fn boot(app: AppHandle, st: State<AppState>) -> Boot {
    Boot {
        settings: st.settings.lock().unwrap().clone(),
        defaults: config::default_keybindings(),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        version: app.package_info().version.to_string(),
        config_dir: config::dir().to_string_lossy().into(),
        desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, st: State<AppState>, mut settings: Settings) -> R<()> {
    // The sync section only changes through the sync commands (the UI copy may be stale).
    settings.sync = st.settings.lock().unwrap().sync.clone();
    settings.save().map_err(e)?;
    *st.settings.lock().unwrap() = settings.clone();
    if let Some(t) = app.tray_by_id("main") {
        let _ = t.set_visible(settings.behavior.tray_icon);
    }
    refresh_menus(&app);
    syncd::kick(&app);
    Ok(())
}

#[tauri::command]
fn get_hosts() -> HostsDoc {
    HostsDoc::load()
}

#[tauri::command]
fn save_hosts(app: AppHandle, doc: HostsDoc) -> R<()> {
    doc.save().map_err(e)?;
    refresh_menus(&app);
    app.state::<AppState>().probe_now.notify_one();
    syncd::kick(&app);
    Ok(())
}

/// Called by the UI once the first frame is painted: the window starts hidden so it never flashes white.
#[tauri::command]
fn app_ready(app: AppHandle) {
    show_main(&app);
}

#[tauri::command]
fn set_labels(app: AppHandle, st: State<AppState>, labels: HashMap<String, String>) -> R<()> {
    *st.labels.lock().unwrap() = labels;
    refresh_menus(&app);
    Ok(())
}

#[tauri::command]
fn host_status(st: State<AppState>) -> HashMap<String, String> {
    st.status.lock().unwrap().clone()
}

#[tauri::command]
fn refresh_status(st: State<AppState>) {
    st.probe_now.notify_one();
}

/// Backup: the hosts file only. Passwords stay in the OS keychain.
#[tauri::command]
fn export_hosts(path: String) -> R<()> {
    let doc = serde_json::to_string_pretty(&HostsDoc::load()).map_err(e)?;
    std::fs::write(path, doc).map_err(e)
}

#[tauri::command]
fn read_hosts_file(path: String) -> R<HostsDoc> {
    serde_json::from_str(&std::fs::read_to_string(path).map_err(e)?).map_err(|err| format!("fichier invalide : {err}"))
}

#[tauri::command]
fn set_secret(app: AppHandle, id: String, value: String) -> R<()> {
    config::secret_set(&id, &value).map_err(e)?;
    syncd::kick(&app);
    Ok(())
}

#[tauri::command]
fn has_secret(id: String) -> bool {
    config::secret_get(&id).is_some()
}

#[tauri::command]
async fn detect_agents() -> Vec<(String, String)> {
    tokio::task::spawn_blocking(ssh::known_agents).await.unwrap_or_default()
}

/// Copies a private key into the app's key store (0600) and returns the new path.
#[tauri::command]
fn import_key(path: String) -> R<String> {
    let src = std::path::PathBuf::from(&path);
    let dir = config::dir().join("keys");
    std::fs::create_dir_all(&dir).map_err(e)?;
    let dst = dir.join(src.file_name().ok_or("chemin invalide")?);
    std::fs::copy(&src, &dst).map_err(e)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o600)).map_err(e)?;
    }
    Ok(dst.to_string_lossy().into())
}

// ---------- ssh ----------

#[tauri::command]
async fn ssh_connect(
    app: AppHandle,
    st: State<'_, AppState>,
    host_id: String,
    // Quick connect: an unsaved host (id "quick:user@host:port") passed whole instead of looked up.
    host: Option<config::Host>,
    cols: u32,
    rows: u32,
    password: Option<String>,
    tmux: Option<String>,
    on_data: Channel<InvokeResponseBody>,
) -> R<String> {
    let host = match host.filter(|h| h.id == host_id && h.id.starts_with("quick:")) {
        Some(h) => h,
        None => HostsDoc::load().hosts.into_iter().find(|h| h.id == host_id).ok_or("hôte introuvable")?,
    };
    let (sock, keepalive) = {
        let s = st.settings.lock().unwrap();
        (s.ssh.agent_socket.clone(), s.ssh.keepalive_secs)
    };
    let sock = tokio::task::spawn_blocking(move || ssh::resolve_agent(&sock)).await.map_err(e)?;
    let handle = ssh::connect(&host, sock, password, keepalive).await.map_err(e)?;
    let id = uuid::Uuid::new_v4().to_string();
    let (id2, app2) = (id.clone(), app.clone());
    let sess = ssh::open_shell(
        handle,
        cols,
        rows,
        tmux.as_deref(),
        move |d| {
            let _ = on_data.send(InvokeResponseBody::Raw(d));
        },
        move || {
            let _ = app2.emit("ssh-closed", &id2);
            let app3 = app2.clone();
            let id3 = id2.clone();
            tauri::async_runtime::spawn(async move {
                app3.state::<AppState>().sessions.lock().await.remove(&id3);
            });
        },
    )
    .await
    .map_err(e)?;
    st.sessions.lock().await.insert(id.clone(), sess);
    config::recent_push(&host_id);
    refresh_menus(&app);
    Ok(id)
}

/// Host ids (or "quick:…" specs) of the latest connections, newest first.
#[tauri::command]
fn recent_sessions() -> Vec<String> {
    config::recent_load()
}

async fn sess(st: &State<'_, AppState>, id: &str) -> R<Arc<ssh::Session>> {
    st.sessions.lock().await.get(id).cloned().ok_or_else(|| "session fermée".into())
}

#[tauri::command]
async fn ssh_write(st: State<'_, AppState>, id: String, data: String) -> R<()> {
    sess(&st, &id).await?.tx.send(ssh::Cmd::Data(data.into_bytes())).map_err(e)
}

#[tauri::command]
async fn ssh_resize(st: State<'_, AppState>, id: String, cols: u32, rows: u32) -> R<()> {
    sess(&st, &id).await?.tx.send(ssh::Cmd::Resize(cols, rows)).map_err(e)
}

#[tauri::command]
async fn ssh_close(st: State<'_, AppState>, id: String) -> R<()> {
    if let Some(s) = st.sessions.lock().await.remove(&id) {
        let _ = s.tx.send(ssh::Cmd::Close);
    }
    Ok(())
}

#[tauri::command]
async fn ssh_stats(st: State<'_, AppState>, id: String) -> R<ssh::Stats> {
    let s = sess(&st, &id).await?;
    Ok(ssh::parse_stats(&ssh::exec(&s.handle, ssh::STATS_CMD).await.map_err(e)?))
}

#[tauri::command]
async fn ssh_history(st: State<'_, AppState>, id: String) -> R<Vec<String>> {
    let s = sess(&st, &id).await?;
    Ok(ssh::parse_history(&ssh::exec(&s.handle, ssh::HISTORY_CMD).await.map_err(e)?))
}

#[tauri::command]
async fn docker_ps(st: State<'_, AppState>, id: String) -> R<ssh::Docker> {
    let s = sess(&st, &id).await?;
    Ok(ssh::parse_docker(&ssh::exec(&s.handle, ssh::DOCKER_PS_CMD).await.map_err(e)?))
}

#[tauri::command]
async fn tmux_list(st: State<'_, AppState>, id: String) -> R<Option<Vec<ssh::TmuxSession>>> {
    let s = sess(&st, &id).await?;
    Ok(ssh::parse_tmux(&ssh::exec(&s.handle, ssh::TMUX_LS_CMD).await.map_err(e)?))
}

#[tauri::command]
async fn tmux_kill(st: State<'_, AppState>, id: String, name: String) -> R<()> {
    let s = sess(&st, &id).await?;
    let out = ssh::exec(&s.handle, &ssh::tmux_kill_cmd(&name).map_err(e)?).await.map_err(e)?;
    if out.trim().is_empty() { Ok(()) } else { Err(out.trim().to_string()) }
}

#[tauri::command]
async fn sftp_list(st: State<'_, AppState>, id: String, path: String) -> R<(String, Vec<ssh::Entry>)> {
    let s = sess(&st, &id).await?;
    ssh::sftp_list(s.sftp().await.map_err(e)?, &path).await.map_err(e)
}

#[tauri::command]
async fn sftp_download(st: State<'_, AppState>, id: String, remote: String, local: String) -> R<()> {
    let s = sess(&st, &id).await?;
    ssh::sftp_download(s.sftp().await.map_err(e)?, &remote, &local).await.map_err(e)
}

#[tauri::command]
async fn sftp_upload(st: State<'_, AppState>, id: String, local: String, remote: String) -> R<()> {
    let s = sess(&st, &id).await?;
    ssh::sftp_upload(s.sftp().await.map_err(e)?, &local, &remote).await.map_err(e)
}

#[tauri::command]
async fn sftp_mkdir(st: State<'_, AppState>, id: String, path: String) -> R<()> {
    let s = sess(&st, &id).await?;
    s.sftp().await.map_err(e)?.create_dir(path).await.map_err(e)
}

#[tauri::command]
async fn sftp_remove(st: State<'_, AppState>, id: String, path: String, dir: bool) -> R<()> {
    let s = sess(&st, &id).await?;
    let sftp = s.sftp().await.map_err(e)?;
    if dir { sftp.remove_dir(path).await } else { sftp.remove_file(path).await }.map_err(e)
}

// ---------- updates ----------

fn endpoint(channel: &str) -> String {
    match channel {
        "beta" | "nightly" => format!("https://github.com/{REPO}/releases/download/{channel}/latest.json"),
        _ => format!("https://github.com/{REPO}/releases/latest/download/latest.json"),
    }
}

async fn find_update(app: &AppHandle, channel: &str) -> anyhow::Result<Option<tauri_plugin_updater::Update>> {
    Ok(app.updater_builder().endpoints(vec![endpoint(channel).parse()?])?.build()?.check().await?)
}

#[tauri::command]
async fn check_update(app: AppHandle, channel: String) -> R<Option<(String, String)>> {
    Ok(find_update(&app, &channel).await.map_err(e)?.map(|u| (u.version.clone(), u.body.clone().unwrap_or_default())))
}

#[tauri::command]
async fn install_update(app: AppHandle, channel: String) -> R<()> {
    let u = find_update(&app, &channel).await.map_err(e)?.ok_or("aucune mise à jour")?;
    u.download_and_install(|_, _| {}, || {}).await.map_err(e)?;
    app.restart()
}

// ---------- window / menu / tray ----------

/// Probes every host every 30 s (or right away when hosts change), then updates UI + tray.
fn spawn_prober(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let hosts = HostsDoc::load().hosts;
            let map = probe::probe_all(&hosts).await;
            *app.state::<AppState>().status.lock().unwrap() = map.clone();
            let _ = app.emit("host-status", &map);
            refresh_tray(&app, &HostsDoc::load());
            let st = app.state::<AppState>();
            let _ = tokio::time::timeout(std::time::Duration::from_secs(30), st.probe_now.notified()).await;
        }
    });
}

fn show_main(app: &AppHandle) {
    app.state::<AppState>().shown.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[derive(serde::Serialize)]
struct Entry {
    id: String,
    label: String,
    accel: String,
}

/// One menu model for every OS: macOS builds the native bar from it, Windows/Linux draw it in HTML (`menu_model`).
/// Ids: "-" separator, "@x" native item (predefined on macOS, handled by the web view elsewhere), "host:<id>" connect.
/// Accelerators come from the user's keybindings so menus always show the real shortcut.
fn menu_model(app: &AppHandle, s: &Settings) -> Vec<(&'static str, String, Vec<Entry>)> {
    let mac = cfg!(target_os = "macos");
    let e = |id: &str, fallback: &str| {
        let key = id.trim_start_matches('@');
        Entry { id: id.into(), label: lbl(app, key, fallback), accel: s.keybindings.get(key).cloned().unwrap_or_default() }
    };
    let sep = || Entry { id: "-".into(), label: String::new(), accel: String::new() };
    let mut termix = vec![e("about", "À propos de Termix"), e("check_update", "Rechercher des mises à jour…"), sep(), e("settings", "Réglages…"), sep()];
    if mac {
        termix.extend([e("@services", ""), sep(), e("@hide", ""), e("@hide_others", ""), e("@show_all", ""), sep()]);
    }
    termix.push(e("@quit_app", "Quitter Termix"));
    let file = vec![
        e("new_tab", "Nouvel onglet"), e("quick_connect", "Connexion rapide…"), sep(),
        e("new_host", "Nouvel hôte…"), e("new_folder", "Nouveau dossier"), sep(),
        e("import_hosts", "Importer des hôtes…"), e("export_hosts", "Exporter les hôtes…"), sep(),
        e("close_pane", "Fermer le panneau"),
    ];
    let edit = vec![
        e("@undo", "Annuler"), e("@redo", "Rétablir"), sep(),
        e("@cut", "Couper"), e("@copy", "Copier"), e("@paste", "Coller"), e("@select_all", "Tout sélectionner"), sep(),
        e("clear_screen", "Effacer l'écran"), sep(), e("search_hosts", "Rechercher un hôte"),
    ];
    let view = vec![
        e("split_right", "Diviser à droite"), e("split_left", "Diviser à gauche"), e("split_down", "Diviser en bas"), e("split_up", "Diviser en haut"), sep(),
        e("toggle_sidebar", "Afficher/masquer les hôtes"), e("toggle_panel", "Afficher/masquer SFTP et stats"), sep(),
        e("zoom_in", "Agrandir le texte"), e("zoom_out", "Réduire le texte"), e("zoom_reset", "Taille réelle"), sep(),
        e("@fullscreen", "Plein écran"),
    ];
    // Same hosts as the tray (favorites / recent / all, per Settings).
    let mut hosts: Vec<Entry> = {
        let b = s.behavior.clone();
        let doc = HostsDoc::load();
        config::tray_hosts(&doc, &b.tray_hosts, b.tray_count, &config::recent_load())
            .iter()
            .map(|h| Entry { id: format!("host:{}", h.id), label: if h.name.is_empty() { h.host.clone() } else { h.name.clone() }, accel: String::new() })
            .collect()
    };
    if !hosts.is_empty() {
        hosts.push(sep());
    }
    hosts.extend([e("quick_connect", "Connexion rapide…"), e("refresh_status", "Actualiser l'état des hôtes")]);
    let window = vec![
        e("@minimize", "Réduire"), e("@maximize", "Agrandir"), sep(),
        e("focus_next", "Panneau suivant"), e("focus_prev", "Panneau précédent"), e("next_tab", "Onglet suivant"), e("prev_tab", "Onglet précédent"), sep(),
        e("@close_window", "Fermer la fenêtre"),
    ];
    let help = vec![
        e("open_docs", "Documentation de la configuration"), e("shortcuts", "Raccourcis clavier"), sep(),
        e("release_notes", "Notes de version"), e("report_issue", "Signaler un problème…"),
    ];
    vec![
        ("termix", "Termix".into(), termix),
        ("menu_file", lbl(app, "menu_file", "Fichier"), file),
        ("menu_edit", lbl(app, "menu_edit", "Édition"), edit),
        ("menu_view", lbl(app, "menu_view", "Présentation"), view),
        ("menu_hosts", lbl(app, "menu_hosts", "Hôtes"), hosts),
        ("menu_window", lbl(app, "menu_window", "Fenêtre"), window),
        ("menu_help", lbl(app, "menu_help", "Aide"), help),
    ]
}

#[tauri::command]
fn get_menu(app: AppHandle, st: State<AppState>) -> Vec<(&'static str, String, Vec<Entry>)> {
    let s = st.settings.lock().unwrap().clone();
    menu_model(&app, &s)
}

#[tauri::command]
fn menu_click(app: AppHandle, id: String) {
    on_menu(&app, &id);
}

/// Native macOS menu bar built from `menu_model`.
#[cfg(target_os = "macos")]
fn build_menu(app: &AppHandle, s: &Settings) -> tauri::Result<Menu<tauri::Wry>> {
    use tauri::menu::PredefinedMenuItem;
    let mut bar = MenuBuilder::new(app);
    for (key, title, entries) in menu_model(app, s) {
        let sub = tauri::menu::Submenu::new(app, title, true)?;
        for x in entries {
            match x.id.as_str() {
                "-" => sub.append(&PredefinedMenuItem::separator(app)?)?,
                "@services" => sub.append(&PredefinedMenuItem::services(app, None)?)?,
                "@hide" => sub.append(&PredefinedMenuItem::hide(app, None)?)?,
                "@hide_others" => sub.append(&PredefinedMenuItem::hide_others(app, None)?)?,
                "@show_all" => sub.append(&PredefinedMenuItem::show_all(app, None)?)?,
                "@quit_app" => sub.append(&PredefinedMenuItem::quit(app, Some(&x.label))?)?,
                "@undo" => sub.append(&PredefinedMenuItem::undo(app, None)?)?,
                "@redo" => sub.append(&PredefinedMenuItem::redo(app, None)?)?,
                "@cut" => sub.append(&PredefinedMenuItem::cut(app, None)?)?,
                "@copy" => sub.append(&PredefinedMenuItem::copy(app, None)?)?,
                "@paste" => sub.append(&PredefinedMenuItem::paste(app, None)?)?,
                "@select_all" => sub.append(&PredefinedMenuItem::select_all(app, None)?)?,
                "@fullscreen" => sub.append(&PredefinedMenuItem::fullscreen(app, None)?)?,
                "@minimize" => sub.append(&PredefinedMenuItem::minimize(app, None)?)?,
                "@maximize" => sub.append(&PredefinedMenuItem::maximize(app, None)?)?,
                "@close_window" => sub.append(&PredefinedMenuItem::close_window(app, None)?)?,
                id => sub.append(&MenuItem::with_id(app, id, &x.label, true, Some(x.accel.as_str()).filter(|k| !k.is_empty()))?)?,
            }
        }
        // Native extras: window list in "Window", search field in "Help".
        match key {
            "menu_window" => sub.set_as_windows_menu_for_nsapp()?,
            "menu_help" => sub.set_as_help_menu_for_nsapp()?,
            _ => {}
        }
        bar = bar.item(&sub);
    }
    bar.build()
}

/// Rebuilds the native menu bar (macOS) and the tray menu after labels, settings or hosts change.
fn refresh_menus(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let s = app.state::<AppState>().settings.lock().unwrap().clone();
        if let Ok(m) = build_menu(app, &s) {
            let _ = app.set_menu(m);
        }
    }
    refresh_tray(app, &HostsDoc::load());
}

/// Tray menu = favourite hosts (or the first ones). Kept icon-only so it stays small next to the MacBook notch.
fn tray_menu(app: &AppHandle, doc: &HostsDoc) -> tauri::Result<Menu<tauri::Wry>> {
    let (mode, count) = {
        let s = app.state::<AppState>().settings.lock().unwrap().behavior.clone();
        (s.tray_hosts, s.tray_count)
    };
    let hosts = config::tray_hosts(doc, &mode, count, &config::recent_load());
    let status = app.state::<AppState>().status.lock().unwrap().clone();
    let mut b = MenuBuilder::new(app);
    for h in &hosts {
        let st = status.get(&h.id).map(|s| s.as_str()).unwrap_or("");
        let label = if h.name == h.host || h.name.is_empty() { h.host.clone() } else { format!("{}  —  {}", h.name, h.host) };
        // GTK app-indicator menus often hide item images, so Linux gets a coloured emoji instead.
        if cfg!(target_os = "linux") {
            let dot = match st { "up" => "🟢", "ping" => "🟡", "down" => "🔴", _ => "⚪" };
            b = b.item(&MenuItem::with_id(app, format!("host:{}", h.id), format!("{dot}  {label}"), true, None::<&str>)?);
        } else {
            b = b.item(&IconMenuItem::with_id(app, format!("host:{}", h.id), label, true, Some(probe::dot(st)), None::<&str>)?);
        }
    }
    if hosts.is_empty() {
        b = b.item(&MenuItem::with_id(app, "noop", lbl(app, "no_hosts", "Aucun hôte"), false, None::<&str>)?);
    }
    b.separator()
        .item(&MenuItem::with_id(app, "show", lbl(app, "open_app", "Ouvrir Termix"), true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "about", lbl(app, "about", "À propos de Termix"), true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "quit", lbl(app, "quit", "Quitter"), true, None::<&str>)?)
        .build()
}

fn refresh_tray(app: &AppHandle, doc: &HostsDoc) {
    if let (Some(t), Ok(m)) = (app.tray_by_id("main"), tray_menu(app, doc)) {
        let _ = t.set_menu(Some(m));
        // macOS tints the template icon itself; elsewhere swap white/black only when the panel changes.
        if cfg!(not(target_os = "macos")) {
            static LAST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
            let dark = panel_is_dark();
            if LAST.swap(dark as u8, std::sync::atomic::Ordering::Relaxed) != dark as u8 {
                if let Ok(i) = tray_icon(dark) {
                    let _ = t.set_icon(Some(i));
                }
            }
        }
    }
}

/// The tray glyph is black (macOS template); recoloured white for dark panels.
fn tray_icon(dark: bool) -> tauri::Result<tauri::image::Image<'static>> {
    let img = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    if cfg!(target_os = "macos") || !dark {
        return Ok(img);
    }
    let mut rgba = img.rgba().to_vec();
    for px in rgba.chunks_mut(4) {
        px[..3].fill(255);
    }
    Ok(tauri::image::Image::new_owned(rgba, img.width(), img.height()))
}

/// Is the taskbar / top panel dark? Windows: the taskbar follows the *system* theme, not the apps one.
#[cfg(windows)]
fn panel_is_dark() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "/v", "SystemUsesLightTheme"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).contains("0x1"))
        .unwrap_or(true)
}

/// GNOME's top bar is dark in both modes; KDE panels follow the colour scheme (kdeglobals).
#[cfg(target_os = "linux")]
fn panel_is_dark() -> bool {
    if !std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().contains("KDE") {
        return true;
    }
    let cfg = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    std::fs::read_to_string(format!("{cfg}/kdeglobals"))
        .map(|s| kde_dark(&s))
        .unwrap_or(true)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn kde_dark(kdeglobals: &str) -> bool {
    kdeglobals.lines().find_map(|l| l.strip_prefix("ColorScheme=")).map_or(true, |v| v.to_lowercase().contains("dark"))
}

#[cfg(target_os = "macos")]
fn panel_is_dark() -> bool {
    true
}

#[test]
fn kde_scheme() {
    assert!(kde_dark("[General]\nColorScheme=BreezeDark\n"));
    assert!(!kde_dark("[General]\nColorScheme=BreezeLight\n"));
    assert!(kde_dark("")); // unknown: Breeze's default panel is dark
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "show" => show_main(app),
        _ => {
            show_main(app);
            if let Some(host) = id.strip_prefix("host:") {
                let _ = app.emit("connect-host", host);
            } else {
                let _ = app.emit("menu", id);
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings = Settings::load();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            settings: Mutex::new(settings.clone()),
            sessions: Default::default(),
            labels: Default::default(),
            status: Default::default(),
            probe_now: Default::default(),
            shown: Default::default(),
        })
        .manage(syncd::SyncCtl::default())
        .setup(move |app| {
            let handle = app.handle().clone();
            // Created here (create: false) so Windows/Linux get our own title bar: menus + Windows-style buttons.
            let mut wc = app.config().app.windows[0].clone();
            if cfg!(not(target_os = "macos")) {
                wc.decorations = false; // Windows 11 keeps its rounded corners and shadow (DWM)
                if cfg!(target_os = "linux") {
                    // Rounded corners are drawn in CSS, so the corners must be see-through.
                    wc.transparent = true;
                    wc.background_color = None;
                }
            }
            tauri::WebviewWindowBuilder::from_config(&handle, &wc)?.build()?;
            #[cfg(target_os = "macos")]
            app.set_menu(build_menu(&handle, &settings)?)?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(tray_icon(true)?)
                .icon_as_template(true)
                .tooltip("Termix")
                .menu(&tray_menu(&handle, &HostsDoc::load())?)
                .show_menu_on_left_click(true)
                .on_tray_icon_event(|t, ev| {
                    if let TrayIconEvent::DoubleClick { .. } = ev {
                        show_main(t.app_handle());
                    }
                })
                .build(app)?;
            tray.set_visible(settings.behavior.tray_icon)?;
            refresh_tray(&handle, &HostsDoc::load()); // picks the white/black glyph for the panel
            spawn_prober(handle.clone());
            syncd::spawn(handle.clone());
            // Safety net: show the window even if the UI never reports ready.
            let h2 = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                if let Some(w) = h2.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(true) && !h2.state::<AppState>().shown.load(std::sync::atomic::Ordering::Relaxed) {
                        show_main(&h2);
                    }
                }
            });
            Ok(())
        })
        .on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()))
        .on_window_event(|w, ev| {
            if let WindowEvent::ThemeChanged(_) = ev {
                refresh_tray(w.app_handle(), &HostsDoc::load());
            }
            if let WindowEvent::CloseRequested { api, .. } = ev {
                let st = w.state::<AppState>();
                let b = &st.settings.lock().unwrap().behavior;
                // Without a tray icon there is no way back to a hidden window on Linux/Windows, so quit there.
                if b.run_in_background && (cfg!(target_os = "macos") || b.tray_icon) {
                    api.prevent_close();
                    let _ = w.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            boot, app_ready, set_labels, get_menu, menu_click, host_status, refresh_status, save_settings, get_hosts, save_hosts, export_hosts, read_hosts_file, set_secret, has_secret, detect_agents, import_key,
            ssh_connect, recent_sessions, ssh_write, ssh_resize, ssh_close, ssh_stats, ssh_history, docker_ps, tmux_list, tmux_kill,
            sftp_list, sftp_download, sftp_upload, sftp_mkdir, sftp_remove,
            check_update, install_update,
            syncd::sync_status, syncd::sync_set_mode, syncd::sync_login, syncd::sync_logout, syncd::sync_unlock, syncd::sync_lock, syncd::sync_now, syncd::sync_set_interval, syncd::sync_resolve,
            syncd::p2p_pair_start, syncd::p2p_pair, syncd::p2p_forget, syncd::sync_set_workspace, syncd::sync_workspaces,
            syncd::sync_keys, syncd::sync_key_import, syncd::sync_key_delete
        ])
        .build(tauri::generate_context!())
        .expect("échec du démarrage de Termix")
        .run(|app, ev| {
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = ev {
                show_main(app);
            }
            let _ = (app, ev);
        });
}
