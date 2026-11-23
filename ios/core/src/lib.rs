//! Termix's core for the native iOS app: the desktop's Rust modules, shared file for file, behind a small C API
//! (declared in ../../Termix/Bridging.h). Calls answer in JSON; events carry JSON, or raw bytes for terminal output.
#![allow(dead_code)] // the shared modules keep their desktop-only helpers (agents, menus…)

#[path = "../../../src-tauri/src/background.rs"]
mod background;
#[path = "../../../src-tauri/src/config.rs"]
mod config;
#[path = "../../../src-tauri/src/p2p.rs"]
mod p2p;
#[path = "../../../src-tauri/src/probe.rs"]
mod probe;
#[path = "../../../src-tauri/src/ssh.rs"]
mod ssh;
#[path = "../../../src-tauri/src/sync.rs"]
mod sync;
#[path = "../../../src-tauri/src/syncd.rs"]
mod syncd;

use config::{HostsDoc, Settings};
use serde::Serialize;
use serde_json::{json, Value};
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

/// What syncd.rs expects from the app (same fields as the desktop's AppState).
pub struct AppState {
    settings: Mutex<Settings>,
    /// Poked by syncd after a sync; iOS doesn't probe hosts.
    probe_now: tokio::sync::Notify,
    sessions: ssh::Sessions,
}

type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(err: E) -> String {
    format!("{err:#}")
}
/// No tray or menu bar on iOS.
fn refresh_menus(_: &AppHandle) {}

type EventFn = extern "C" fn(kind: *const c_char, id: *const c_char, data: *const u8, len: usize);
type DoneFn = extern "C" fn(ctx: *mut c_void, ok: bool, json: *const c_char);
static EVENTS: OnceLock<EventFn> = OnceLock::new();

fn event(kind: &str, id: &str, data: &[u8]) {
    if let Some(f) = EVENTS.get() {
        let (k, i) = (c_string(kind), c_string(id));
        f(k.as_ptr(), i.as_ptr(), data.as_ptr(), data.len());
    }
}

fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

fn from_c(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn state() -> &'static AppState {
    AppHandle.state::<AppState>().inner()
}

/// Loads the settings and starts the sync loop. `device` names this device in sync ("iPhone", "iPad").
#[no_mangle]
pub extern "C" fn termix_init(device: *const c_char, events: EventFn) {
    if EVENTS.set(events).is_err() {
        return;
    }
    let mut settings = Settings::load();
    // config::device_name() runs `hostname`, which an iOS app can't: it falls back to the OS name.
    if settings.sync.device_name == std::env::consts::OS {
        settings.sync.device_name = from_c(device);
    }
    let app = AppHandle;
    app.on_emit(|name, json| event(name, "", json.as_bytes()));
    app.manage(AppState { settings: Mutex::new(settings), probe_now: Default::default(), sessions: Default::default() });
    app.manage(syncd::SyncCtl::default());
    syncd::spawn(app);
}

/// Runs `cmd` with JSON `args` off the calling thread, then `done(ctx, ok, json)`: the result, or the error text.
#[no_mangle]
pub extern "C" fn termix_call(cmd: *const c_char, args: *const c_char, ctx: *mut c_void, done: DoneFn) {
    let (cmd, args) = (from_c(cmd), from_c(args));
    let ctx = ctx as usize; // raw pointers aren't Send; the caller keeps it alive until `done`
    tauri::async_runtime::spawn(async move {
        let args: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let (ok, out) = match dispatch(&cmd, &args).await {
            Ok(v) => (true, v.to_string()),
            Err(err) => (false, err),
        };
        done(ctx as *mut c_void, ok, c_string(&out).as_ptr());
    });
}

/// Keystrokes go straight to the session, in order (a spawned call per key could reorder them).
///
/// # Safety
/// `data` points to `len` readable bytes (or is null), `id` to a C string.
#[no_mangle]
pub unsafe extern "C" fn termix_write(id: *const c_char, data: *const u8, len: usize) {
    if data.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    if let Some(s) = state().sessions.blocking_lock().get(&from_c(id)) {
        let _ = s.tx.send(ssh::Cmd::Data(bytes));
    }
}

fn to<T: Serialize>(v: R<T>) -> R<Value> {
    v.and_then(|v| serde_json::to_value(v).map_err(e))
}

async fn session(id: &str) -> R<std::sync::Arc<ssh::Session>> {
    state().sessions.lock().await.get(id).cloned().ok_or_else(|| "session fermée".into())
}

async fn dispatch(cmd: &str, a: &Value) -> R<Value> {
    let app = AppHandle;
    let s = |k: &str| a[k].as_str().unwrap_or_default().to_string();
    let n = |k: &str| a[k].as_u64().unwrap_or(0) as u32;
    match cmd {
        "boot" => Ok(json!({ "settings": *state().settings.lock().unwrap(), "version": env!("CARGO_PKG_VERSION") })),
        // `path` is a JSON pointer into the settings, e.g. "/appearance/theme".
        "set_setting" => {
            let mut v = serde_json::to_value(&*state().settings.lock().unwrap()).map_err(e)?;
            *v.pointer_mut(&s("path")).ok_or("réglage inconnu")? = a["value"].clone();
            let new: Settings = serde_json::from_value(v).map_err(e)?;
            new.save().map_err(e)?;
            *state().settings.lock().unwrap() = new;
            syncd::kick(&app);
            Ok(Value::Null)
        }
        "get_hosts" => to(Ok(HostsDoc::load())),
        "save_hosts" => {
            let doc: HostsDoc = serde_json::from_value(a["doc"].clone()).map_err(e)?;
            doc.save().map_err(e)?;
            syncd::kick(&app);
            Ok(Value::Null)
        }
        "set_secret" => {
            config::secret_set(&s("id"), &s("value")).map_err(e)?;
            syncd::kick(&app);
            Ok(Value::Null)
        }
        "has_secret" => Ok(json!(config::secret_get(&s("id")).is_some())),
        // Synced keys (the vault) and this device's own: the same store as the desktop (syncd.rs).
        "keys" => Ok(json!(syncd::keys())),
        "import_key" => syncd::key_import(app, Some(s("pem")), None, s("name"), a["sync"].as_bool().unwrap_or(false)).await.map(Value::from),
        "delete_key" => to(syncd::key_delete(app, s("id")).await),
        "keys_set_synced" => {
            let ids = a["ids"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            to(syncd::keys_set_synced(app, ids, a["synced"].as_bool().unwrap_or(false)).await)
        }
        "recent" => Ok(json!(config::recent_load())),
        "probe" => to(Ok(probe::probe_all(&HostsDoc::load().hosts).await)),
        "ssh_connect" => ssh_connect(a).await.map(|_| Value::Null),
        "tmux_list" => {
            let sess = session(&s("id")).await?;
            to(Ok(ssh::parse_tmux(&ssh::exec(&sess.handle, ssh::TMUX_LS_CMD).await.map_err(e)?)))
        }
        "tmux_kill" => {
            let sess = session(&s("id")).await?;
            let out = ssh::exec(&sess.handle, &ssh::tmux_kill_cmd(&s("name")).map_err(e)?).await.map_err(e)?;
            if out.trim().is_empty() { Ok(Value::Null) } else { Err(out.trim().to_string()) }
        }
        "docker_ps" => {
            let sess = session(&s("id")).await?;
            to(Ok(ssh::parse_docker(&ssh::exec(&sess.handle, ssh::DOCKER_PS_CMD).await.map_err(e)?)))
        }
        "ssh_resize" => session(&s("id")).await?.tx.send(ssh::Cmd::Resize(n("cols"), n("rows"))).map(|_| Value::Null).map_err(e),
        "ssh_close" => {
            if let Some(sess) = state().sessions.lock().await.remove(&s("id")) {
                let _ = sess.tx.send(ssh::Cmd::Close);
            }
            Ok(Value::Null)
        }
        "ssh_stats" => {
            let sess = session(&s("id")).await?;
            to(Ok(ssh::parse_stats(&ssh::exec(&sess.handle, ssh::STATS_CMD).await.map_err(e)?)))
        }
        "sftp_list" => {
            let sess = session(&s("id")).await?;
            let (path, entries) = ssh::sftp_list(sess.sftp().await.map_err(e)?, &s("path")).await.map_err(e)?;
            Ok(json!({ "path": path, "entries": entries }))
        }
        "sftp_download" => {
            let sess = session(&s("id")).await?;
            to(ssh::sftp_download(sess.sftp().await.map_err(e)?, &s("remote"), &s("local")).await.map_err(e))
        }
        "sftp_upload" => {
            let sess = session(&s("id")).await?;
            to(ssh::sftp_upload(sess.sftp().await.map_err(e)?, &s("local"), &s("remote")).await.map_err(e))
        }
        "sftp_mkdir" => {
            let sess = session(&s("id")).await?;
            to(sess.sftp().await.map_err(e)?.create_dir(s("path")).await.map_err(e))
        }
        "sftp_remove" => {
            let sess = session(&s("id")).await?;
            let sftp = sess.sftp().await.map_err(e)?;
            let path = s("path");
            to(if a["dir"].as_bool().unwrap_or(false) { sftp.remove_dir(path).await } else { sftp.remove_file(path).await }.map_err(e))
        }
        "sync_status" => Ok(syncd::sync_status(app).await),
        "sync_login" => to(syncd::sync_login(app, s("url"), s("user"), s("password")).await),
        "sync_logout" => to(syncd::sync_logout(app).await),
        "sync_unlock" => to(syncd::sync_unlock(app, s("passphrase")).await),
        "sync_lock" => to(syncd::sync_lock()),
        "sync_now" => to(syncd::sync_now(app).await),
        "sync_resolve" => to(syncd::sync_resolve(app, s("key"), s("keep")).await),
        // Open sessions ({host, tmux}), so a computer can reopen them, and the other devices' ones.
        "sync_set_workspace" => to(syncd::sync_set_workspace(app, a["tabs"].clone()).await),
        "sync_workspaces" => Ok(json!(syncd::sync_workspaces(app))),
        _ => Err(format!("commande inconnue : {cmd}")),
    }
}

/// Opens a shell for `host_id` under the caller's `session` id; output arrives as "data" events for that id.
/// `tmux`: the tmux session to attach or create (null: a plain shell). `host`: an unsaved quick-connect host.
async fn ssh_connect(a: &Value) -> R<()> {
    let s = |k: &str| a[k].as_str().unwrap_or_default().to_string();
    let id = s("session");
    let quick = serde_json::from_value::<config::Host>(a["host"].clone()).ok().filter(|h| h.id.starts_with("quick:") && h.id == s("host_id"));
    let mut host = match quick {
        Some(h) => h,
        None => HostsDoc::load().hosts.into_iter().find(|h| h.id == s("host_id")).ok_or("hôte introuvable")?,
    };
    // A key file synced from a computer (~/.ssh/id_ed25519) doesn't exist here: fall back to the vault key or password.
    if !host.key_path.is_empty() && !std::path::Path::new(&host.key_path).is_file() {
        host.key_path.clear();
    }
    let keepalive = state().settings.lock().unwrap().ssh.keepalive_secs;
    let handle = ssh::connect(&host, None, a["password"].as_str().map(String::from), keepalive).await.map_err(e)?;
    let (cols, rows) = (a["cols"].as_u64().unwrap_or(80) as u32, a["rows"].as_u64().unwrap_or(24) as u32);
    let (id_data, id_close) = (id.clone(), id.clone());
    let tmux = a["tmux"].as_str().filter(|n| !n.is_empty()).map(String::from);
    let sess = ssh::open_shell(
        handle,
        cols,
        rows,
        tmux.as_deref(),
        move |d| event("data", &id_data, &d),
        move |ended| {
            event("ssh-closed", &id_close, if ended { &b"true"[..] } else { &b"false"[..] });
            tauri::async_runtime::spawn(async move { state().sessions.lock().await.remove(&id_close) });
        },
    )
    .await
    .map_err(e)?;
    state().sessions.lock().await.insert(id, sess);
    config::recent_push(&host.id);
    Ok(())
}
