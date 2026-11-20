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
use tauri::menu::{IconMenuItem, Menu, MenuBuilder, MenuItem, PredefinedMenuItem, SubmenuBuilder};
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
    refresh_tray(&app, &HostsDoc::load());
    #[cfg(target_os = "macos")]
    app.set_menu(build_menu(&app, &settings).map_err(e)?).map_err(e)?;
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
    refresh_tray(&app, &doc);
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
    #[cfg(target_os = "macos")]
    {
        let s = st.settings.lock().unwrap().clone();
        app.set_menu(build_menu(&app, &s).map_err(e)?).map_err(e)?;
    }
    refresh_tray(&app, &HostsDoc::load());
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
    refresh_tray(&app, &HostsDoc::load());
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

/// Native macOS menu bar. Accelerators come from the user's keybindings so menus always show the real shortcut.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn build_menu(app: &AppHandle, s: &Settings) -> tauri::Result<Menu<tauri::Wry>> {
    let l = |id: &str, fallback: &str| lbl(app, id, fallback);
    let item = |id: &str, label: &str| {
        MenuItem::with_id(app, id, l(id, label), true, s.keybindings.get(id).map(|k| k.as_str()).filter(|k| !k.is_empty()))
    };
    let plain = |id: &str, label: &str| MenuItem::with_id(app, id, l(id, label), true, None::<&str>);
    let app_menu = SubmenuBuilder::new(app, "Termix")
        .item(&plain("about", "À propos de Termix")?)
        .item(&plain("check_update", "Rechercher des mises à jour…")?)
        .separator()
        .item(&item("settings", "Réglages…")?)
        .separator()
        .item(&PredefinedMenuItem::services(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some(&l("quit_app", "Quitter Termix")))?)
        .build()?;
    let file = SubmenuBuilder::new(app, l("menu_file", "Fichier"))
        .item(&item("new_host", "Nouvel hôte…")?)
        .item(&plain("new_folder", "Nouveau dossier")?)
        .separator()
        .item(&plain("import_hosts", "Importer des hôtes…")?)
        .item(&plain("export_hosts", "Exporter les hôtes…")?)
        .separator()
        .item(&item("close_pane", "Fermer le panneau")?)
        .build()?;
    let edit = SubmenuBuilder::new(app, l("menu_edit", "Édition"))
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .separator()
        .item(&item("search_hosts", "Rechercher un hôte")?)
        .build()?;
    let view = SubmenuBuilder::new(app, l("menu_view", "Présentation"))
        .item(&item("split_right", "Diviser à droite")?)
        .item(&item("split_left", "Diviser à gauche")?)
        .item(&item("split_down", "Diviser en bas")?)
        .item(&item("split_up", "Diviser en haut")?)
        .separator()
        .item(&item("focus_next", "Panneau suivant")?)
        .item(&item("focus_prev", "Panneau précédent")?)
        .item(&item("next_tab", "Onglet suivant")?)
        .item(&item("prev_tab", "Onglet précédent")?)
        .separator()
        .item(&item("toggle_sidebar", "Afficher/masquer les hôtes")?)
        .item(&item("toggle_panel", "Afficher/masquer SFTP et stats")?)
        .separator()
        .item(&item("zoom_in", "Agrandir le texte")?)
        .item(&item("zoom_out", "Réduire le texte")?)
        .item(&item("zoom_reset", "Taille réelle")?)
        .separator()
        .item(&PredefinedMenuItem::fullscreen(app, None)?)
        .build()?;
    let window = SubmenuBuilder::new(app, l("menu_window", "Fenêtre"))
        .item(&PredefinedMenuItem::minimize(app, None)?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::close_window(app, None)?)
        .build()?;
    let help = SubmenuBuilder::new(app, l("menu_help", "Aide"))
        .item(&plain("open_docs", "Documentation de la configuration")?)
        .build()?;
    MenuBuilder::new(app).items(&[&app_menu, &file, &edit, &view, &window, &help]).build()
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
    }
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
            #[cfg(target_os = "macos")]
            app.set_menu(build_menu(&handle, &settings)?)?;
            #[cfg(not(target_os = "macos"))]
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.remove_menu();
            }
            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(icon)
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
            boot, app_ready, set_labels, host_status, refresh_status, save_settings, get_hosts, save_hosts, export_hosts, read_hosts_file, set_secret, has_secret, detect_agents, import_key,
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
