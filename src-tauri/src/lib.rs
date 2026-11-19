mod config;
mod ssh;

use config::{HostsDoc, Settings};
use std::sync::{Arc, Mutex};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::menu::{Menu, MenuBuilder, MenuItem, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_updater::UpdaterExt;

const REPO: &str = "midnights-ra1n/RustTerm";

struct AppState {
    settings: Mutex<Settings>,
    sessions: ssh::Sessions,
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
    version: String,
    config_dir: String,
}

#[tauri::command]
fn boot(app: AppHandle, st: State<AppState>) -> Boot {
    Boot {
        settings: st.settings.lock().unwrap().clone(),
        defaults: config::default_keybindings(),
        platform: std::env::consts::OS,
        version: app.package_info().version.to_string(),
        config_dir: config::dir().to_string_lossy().into(),
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, st: State<AppState>, settings: Settings) -> R<()> {
    settings.save().map_err(e)?;
    *st.settings.lock().unwrap() = settings.clone();
    if let Some(t) = app.tray_by_id("main") {
        let _ = t.set_visible(settings.behavior.tray_icon);
    }
    #[cfg(target_os = "macos")]
    app.set_menu(build_menu(&app, &settings).map_err(e)?).map_err(e)?;
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
    Ok(())
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
fn set_secret(id: String, value: String) -> R<()> {
    config::secret_set(&id, &value).map_err(e)
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
    cols: u32,
    rows: u32,
    password: Option<String>,
    on_data: Channel<InvokeResponseBody>,
) -> R<String> {
    let host = HostsDoc::load().hosts.into_iter().find(|h| h.id == host_id).ok_or("hôte introuvable")?;
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
    Ok(id)
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

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Native macOS menu bar. Accelerators come from the user's keybindings so menus always show the real shortcut.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn build_menu(app: &AppHandle, s: &Settings) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, label: &str| {
        MenuItem::with_id(app, id, label, true, s.keybindings.get(id).map(|k| k.as_str()).filter(|k| !k.is_empty()))
    };
    let app_menu = SubmenuBuilder::new(app, "RustTerm")
        .item(&PredefinedMenuItem::about(app, Some("À propos de RustTerm"), None)?)
        .item(&MenuItem::with_id(app, "check_update", "Rechercher des mises à jour…", true, None::<&str>)?)
        .separator()
        .item(&item("settings", "Réglages…")?)
        .separator()
        .item(&PredefinedMenuItem::services(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("Quitter RustTerm"))?)
        .build()?;
    let file = SubmenuBuilder::new(app, "Fichier")
        .item(&item("new_host", "Nouvel hôte…")?)
        .item(&MenuItem::with_id(app, "new_folder", "Nouveau dossier", true, None::<&str>)?)
        .separator()
        .item(&MenuItem::with_id(app, "import_hosts", "Importer des hôtes…", true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "export_hosts", "Exporter les hôtes…", true, None::<&str>)?)
        .separator()
        .item(&item("close_pane", "Fermer le panneau")?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Édition")
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
    let view = SubmenuBuilder::new(app, "Présentation")
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
    let window = SubmenuBuilder::new(app, "Fenêtre")
        .item(&PredefinedMenuItem::minimize(app, None)?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::close_window(app, None)?)
        .build()?;
    let help = SubmenuBuilder::new(app, "Aide")
        .item(&MenuItem::with_id(app, "open_docs", "Documentation de la configuration", true, None::<&str>)?)
        .build()?;
    MenuBuilder::new(app).items(&[&app_menu, &file, &edit, &view, &window, &help]).build()
}

/// Tray menu = favourite hosts (or the first ones). Kept icon-only so it stays small next to the MacBook notch.
fn tray_menu(app: &AppHandle, doc: &HostsDoc) -> tauri::Result<Menu<tauri::Wry>> {
    let mut hosts: Vec<_> = doc.hosts.iter().filter(|h| h.favorite).collect();
    if hosts.is_empty() {
        hosts = doc.hosts.iter().collect();
    }
    let mut b = MenuBuilder::new(app);
    for h in hosts.iter().take(12) {
        b = b.item(&MenuItem::with_id(app, format!("host:{}", h.id), &h.name, true, None::<&str>)?);
    }
    if hosts.is_empty() {
        b = b.item(&MenuItem::with_id(app, "noop", "Aucun hôte", false, None::<&str>)?);
    }
    b.separator()
        .item(&MenuItem::with_id(app, "show", "Ouvrir RustTerm", true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?)
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
        .manage(AppState { settings: Mutex::new(settings.clone()), sessions: Default::default() })
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
                .tooltip("RustTerm")
                .menu(&tray_menu(&handle, &HostsDoc::load())?)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()))
                .on_tray_icon_event(|t, ev| {
                    if let TrayIconEvent::DoubleClick { .. } = ev {
                        show_main(t.app_handle());
                    }
                })
                .build(app)?;
            tray.set_visible(settings.behavior.tray_icon)?;
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
            boot, save_settings, get_hosts, save_hosts, export_hosts, read_hosts_file, set_secret, has_secret, detect_agents, import_key,
            ssh_connect, ssh_write, ssh_resize, ssh_close, ssh_stats,
            sftp_list, sftp_download, sftp_upload, sftp_mkdir, sftp_remove,
            check_update, install_update
        ])
        .build(tauri::generate_context!())
        .expect("échec du démarrage de RustTerm")
        .run(|app, ev| {
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = ev {
                show_main(app);
            }
            let _ = (app, ev);
        });
}
