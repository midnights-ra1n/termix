//! Sync orchestration: the background loop and the Tauri commands behind Settings → Sync.
use crate::config::{self, HostsDoc};
use crate::p2p::{self, P2p};
use crate::sync::{self, Store, Vault};
use crate::{e, AppState, R};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub struct SyncCtl {
    kick: tokio::sync::Notify,
    /// One sync at a time: store file, keychain and hosts.json are shared.
    lock: tokio::sync::Mutex<()>,
    pub p2p: Arc<P2p>,
    syncing: std::sync::atomic::AtomicBool,
}

/// Tells the title bar where sync stands.
fn emit_status(app: &AppHandle) {
    let _ = app.emit("sync-status", status(app));
}

pub fn kick(app: &AppHandle) {
    app.state::<SyncCtl>().kick.notify_one();
}

fn sync_settings(app: &AppHandle) -> config::Settings {
    app.state::<AppState>().settings.lock().unwrap().clone()
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        ensure_p2p(&app).await;
        let mut first = true;
        loop {
            let _ = run_once(&app).await;
            let ctl = app.state::<SyncCtl>();
            let s = sync_settings(&app).sync;
            // Right after launch, local-network peers take a few seconds to show up: try again soon.
            let wait = match (first && s.mode == "p2p", s.interval_secs) {
                (true, _) => Some(5),
                (_, 0) => None,
                (_, n) => Some(n.max(5) as u64),
            };
            first = false;
            // shortcut: "live" polls every 5 s (the server's ?after= query is cheap); a push channel would cut the delay to zero.
            tokio::select! {
                _ = ctl.kick.notified() => tokio::time::sleep(Duration::from_millis(1500)).await,
                _ = async { match wait { Some(n) => tokio::time::sleep(Duration::from_secs(n)).await, None => std::future::pending().await } } => {}
            }
        }
    });
}

/// Starts or stops the local-network service to match the mode.
async fn ensure_p2p(app: &AppHandle) {
    let s = sync_settings(app).sync;
    let ctl = app.state::<SyncCtl>();
    if s.mode != "p2p" {
        ctl.p2p.stop();
        return;
    }
    let a = app.clone();
    let exchange: p2p::Exchange = Arc::new(move |theirs| {
        let app = a.clone();
        Box::pin(async move {
            let ctl = app.state::<SyncCtl>();
            // Both devices may be syncing towards each other at once: never wait for the lock here (that
            // would deadlock both). Answer with what we have; our own next run pulls their changes.
            let Ok(_g) = ctl.lock.try_lock() else {
                kick(&app);
                return Store::load().records;
            };
            let mut store = Store::load();
            sync::merge_into(&mut store, &theirs, &sync_settings(&app).sync.device_id);
            store.dirty.clear(); // the peer gets everything we have in the answer
            let _ = finish(&app, &mut store, Vault::load().as_ref());
            emit_status(&app);
            store.records
        })
    });
    if let Err(err) = ctl.p2p.start(s.device_id, s.device_name, exchange).await {
        let mut store = Store::load();
        store.last_error = format!("réseau local : {err}");
        let _ = store.save();
    }
}

/// Records → this device, then tells the UI what changed.
fn finish(app: &AppHandle, store: &mut Store, vault: Option<&Vault>) -> anyhow::Result<()> {
    let st = app.state::<AppState>();
    let settings = st.settings.lock().unwrap().clone();
    let (applied, new_settings) = sync::apply(store, vault, &settings)?;
    store.save()?;
    if applied.settings {
        *st.settings.lock().unwrap() = new_settings.clone();
    }
    if applied.hosts || applied.settings {
        crate::refresh_menus(app);
    }
    if applied.hosts {
        st.probe_now.notify_one();
    }
    if applied.hosts || applied.settings {
        let _ = app.emit("sync-changed", json!({ "hosts": applied.hosts, "settings": applied.settings }));
    }
    Ok(())
}

pub async fn run_once(app: &AppHandle) -> R<()> {
    let ctl = app.state::<SyncCtl>();
    let _g = ctl.lock.lock().await;
    let settings = sync_settings(app);
    let mode = settings.sync.mode.as_str();
    if mode != "server" && mode != "p2p" {
        emit_status(app);
        return Ok(());
    }
    ctl.syncing.store(true, std::sync::atomic::Ordering::Relaxed);
    emit_status(app);
    let r = sync_round(app, &ctl, &settings).await;
    ctl.syncing.store(false, std::sync::atomic::Ordering::Relaxed);
    emit_status(app);
    r
}

async fn sync_round(app: &AppHandle, ctl: &SyncCtl, settings: &config::Settings) -> R<()> {
    let (mode, dev) = (settings.sync.mode.clone(), settings.sync.device_id.clone());
    let vault = Vault::load();
    let mut store = Store::load();
    sync::record_local(&mut store, &HostsDoc::load(), &settings, vault.as_ref(), &config::secret_get, &crate::background::read, &dev);
    if config::denied() {
        // Unreadable secrets would look deleted and be wiped on every device: skip the round, store unsaved.
        return Err("accès au trousseau refusé : relance Termix et autorise-le".into());
    }
    let result: anyhow::Result<()> = if mode == "server" {
        match config::secret_get("sync:token") {
            Some(tok) => sync::server_sync(&settings.sync.server_url, &tok, &mut store, &dev).await.map(|_| ()),
            None => Err(anyhow::anyhow!("non connecté au serveur de synchro")),
        }
    } else {
        let (mut errs, mut reached) = (vec![], false);
        for p in store.peers.clone() {
            let addrs = ctl.p2p.addrs(&p.id);
            if addrs.is_empty() {
                continue; // not on this network right now
            }
            match p2p::sync_with(&addrs, &p.id, &dev, &mut store).await {
                Ok(()) => reached = true,
                Err(err) => errs.push(format!("{} : {err}", p.name)),
            }
        }
        // shortcut: delivered to one peer counts as delivered; a third device offline now won't flag a conflict later.
        if reached {
            store.dirty.clear();
        }
        if errs.is_empty() { Ok(()) } else { Err(anyhow::anyhow!(errs.join(" · "))) }
    };
    match &result {
        Ok(()) => {
            store.last_ok = sync::now_ms();
            store.last_error.clear();
        }
        Err(err) => store.last_error = format!("{err:#}"),
    }
    finish(app, &mut store, vault.as_ref()).map_err(e)?;
    result.map_err(e)
}

// ---------- commands ----------

/// Async so the keychain read runs off the main thread (it froze the window at launch).
#[tauri::command]
pub async fn sync_status(app: AppHandle) -> Value {
    status(&app)
}

fn status(app: &AppHandle) -> Value {
    let s = sync_settings(app).sync;
    let store = Store::load();
    let ctl = app.state::<SyncCtl>();
    let paired: std::collections::BTreeSet<&str> = store.peers.iter().map(|p| p.id.as_str()).collect();
    let visible: std::collections::BTreeSet<String> = ctl.p2p.discovered().into_iter().map(|f| f.id).collect();
    json!({
        "mode": s.mode, "server_url": s.server_url, "user": s.user, "device_id": s.device_id, "device_name": s.device_name,
        "logged_in": config::secret_get("sync:token").is_some(),
        "unlocked": Vault::load().is_some(),
        "vault_exists": store.records.get("vault:check").is_some_and(|r| r.v.is_some()),
        "last_ok": store.last_ok, "last_error": store.last_error, "interval": s.interval_secs, "keybindings": s.keybindings,
        "syncing": ctl.syncing.load(std::sync::atomic::Ordering::Relaxed),
        "conflicts": store.conflicts.iter().map(|(k, c)| json!({
            "key": k,
            "mine": { "v": c.mine.v, "ts": c.mine.ts },
            "theirs": { "v": c.theirs.v, "ts": c.theirs.ts, "device": device_name(&store, &c.theirs.dev) },
        })).collect::<Vec<_>>(),
        "peers": store.peers.iter().map(|p| json!({ "id": p.id, "name": p.name, "online": visible.contains(&p.id) })).collect::<Vec<_>>(),
        "discovered": ctl.p2p.discovered().into_iter().filter(|f| !paired.contains(f.id.as_str())).collect::<Vec<_>>(),
    })
}

fn save_sync(app: &AppHandle, f: impl FnOnce(&mut config::Sync)) -> R<()> {
    let st = app.state::<AppState>();
    let mut s = st.settings.lock().unwrap().clone();
    f(&mut s.sync);
    s.save().map_err(e)?;
    *st.settings.lock().unwrap() = s;
    Ok(())
}

#[tauri::command]
pub async fn sync_set_mode(app: AppHandle, mode: String, device_name: String) -> R<()> {
    if !["off", "server", "p2p"].contains(&mode.as_str()) {
        return Err(format!("mode inconnu : {mode}"));
    }
    save_sync(&app, |s| {
        s.mode = mode;
        if !device_name.trim().is_empty() {
            s.device_name = device_name.trim().into();
        }
    })?;
    ensure_p2p(&app).await;
    kick(&app);
    Ok(())
}

#[tauri::command]
pub async fn sync_login(app: AppHandle, url: String, user: String, password: String) -> R<()> {
    let device = sync_settings(&app).sync.device_name;
    let (token, salt) = sync::server_login(&url, &user, &password, &device).await.map_err(e)?;
    let prev = sync_settings(&app).sync;
    {
        let ctl = app.state::<SyncCtl>();
        let _g = ctl.lock.lock().await;
        // Another account or server: start from a clean slate (local hosts are re-sent on the next run).
        if prev.server_url != url || prev.user != user {
            Store::reset();
            let _ = config::secret_set("sync:vault", "");
        }
        let mut store = Store::load();
        store.salt = salt;
        store.save().map_err(e)?;
        config::secret_set("sync:token", &token).map_err(e)?;
    }
    save_sync(&app, |s| {
        s.mode = "server".into();
        s.server_url = url.trim().trim_end_matches('/').into();
        s.user = user;
    })?;
    ensure_p2p(&app).await;
    run_once(&app).await
}

#[tauri::command]
pub async fn sync_logout(app: AppHandle) -> R<()> {
    let ctl = app.state::<SyncCtl>();
    let _g = ctl.lock.lock().await;
    let _ = config::secret_set("sync:token", "");
    let _ = config::secret_set("sync:vault", "");
    Store::reset();
    save_sync(&app, |s| s.mode = "off".into())
}

/// Unlocks (or creates) the end-to-end encrypted vault with the account passphrase.
#[tauri::command]
pub async fn sync_unlock(app: AppHandle, passphrase: String) -> R<()> {
    if passphrase.chars().count() < 8 {
        return Err("la phrase secrète doit faire au moins 8 caractères".into());
    }
    let settings = sync_settings(&app);
    {
        let ctl = app.state::<SyncCtl>();
        let _g = ctl.lock.lock().await;
        let mut store = Store::load();
        // Fetch the account's vault check first, so a second device can't create a competing one.
        if settings.sync.mode == "server" {
            let tok = config::secret_get("sync:token").ok_or("non connecté au serveur de synchro")?;
            sync::server_sync(&settings.sync.server_url, &tok, &mut store, &settings.sync.device_id).await.map_err(e)?;
        }
        if store.salt.is_empty() {
            if settings.sync.mode == "server" {
                return Err("le serveur n'a pas fourni de sel de chiffrement".into());
            }
            store.salt = sync::random_salt();
        }
        let salt = store.salt.clone();
        let key = tokio::task::spawn_blocking(move || Vault::derive(&passphrase, &salt)).await.map_err(e)?.map_err(e)?;
        sync::check_or_init(&mut store, &key, &settings.sync.device_id).map_err(e)?;
        config::secret_set("sync:vault", &B64.encode(key)).map_err(e)?;
        // Write the account's secrets to this keychain before anything could read their absence as a deletion.
        finish(&app, &mut store, Some(&Vault::new(&key))).map_err(e)?;
    }
    kick(&app);
    Ok(())
}

#[tauri::command]
pub fn sync_lock() -> R<()> {
    config::secret_set("sync:vault", "").map_err(e)
}

/// A device's name from its pairing or its synced workspace, else a short id.
fn device_name(store: &Store, dev: &str) -> String {
    store.peers.iter().find(|p| p.id == dev).map(|p| p.name.clone())
        .or_else(|| store.records.get(&format!("workspace:{dev}"))?.v.as_ref()?["name"].as_str().map(String::from))
        .unwrap_or_else(|| dev.chars().take(8).collect())
}

#[tauri::command]
pub async fn sync_set_interval(app: AppHandle, secs: u32) -> R<()> {
    save_sync(&app, |s| s.interval_secs = secs)?;
    kick(&app);
    Ok(())
}

/// synced | local (see config::Sync::keybindings). Going back to synced publishes this device's shortcuts.
#[tauri::command]
pub async fn sync_set_keybindings(app: AppHandle, mode: String) -> R<()> {
    if !["synced", "local"].contains(&mode.as_str()) {
        return Err(format!("mode inconnu : {mode}"));
    }
    save_sync(&app, |s| s.keybindings = mode)?;
    kick(&app);
    Ok(())
}

/// Replaces this device's shortcuts with the synced ones (adapted to this system).
#[tauri::command]
pub async fn sync_take_keybindings(app: AppHandle) -> R<()> {
    let store = Store::load();
    let keys = store.records.get("settings").and_then(|r| sync::native_keys(&r.v.as_ref()?["keybindings"]))
        .ok_or("aucun raccourci synchronisé pour l'instant")?;
    let st = app.state::<AppState>();
    let mut s = st.settings.lock().unwrap().clone();
    s.keybindings = keys;
    s.save().map_err(e)?;
    *st.settings.lock().unwrap() = s;
    crate::refresh_menus(&app);
    let _ = app.emit("sync-changed", json!({ "hosts": false, "settings": true }));
    Ok(())
}

/// Settles a conflict: `keep` = "mine" re-publishes this device's version (newest, so it wins everywhere),
/// "theirs" accepts what last-writer-wins chose.
#[tauri::command]
pub async fn sync_resolve(app: AppHandle, key: String, keep: String) -> R<()> {
    {
        let ctl = app.state::<SyncCtl>();
        let _g = ctl.lock.lock().await;
        let mut store = Store::load();
        let c = store.conflicts.remove(&key).ok_or("conflit déjà réglé")?;
        let v = if keep == "mine" { c.mine.v } else { c.theirs.v };
        if store.records.get(&key).map(|r| &r.v) != Some(&v) {
            store.put(&key, v, &sync_settings(&app).sync.device_id);
        }
        finish(&app, &mut store, Vault::load().as_ref()).map_err(e)?;
    }
    emit_status(&app);
    kick(&app);
    Ok(())
}

#[tauri::command]
pub async fn sync_now(app: AppHandle) -> R<()> {
    run_once(&app).await
}

#[tauri::command]
pub async fn p2p_pair_start(app: AppHandle) -> R<String> {
    if sync_settings(&app).sync.mode != "p2p" {
        return Err("activez d'abord la synchro en réseau local".into());
    }
    ensure_p2p(&app).await;
    Ok(app.state::<SyncCtl>().p2p.start_pairing())
}

#[tauri::command]
pub async fn p2p_pair(app: AppHandle, peer: String, code: String) -> R<String> {
    let s = sync_settings(&app).sync;
    let addrs = app.state::<SyncCtl>().p2p.addrs(&peer);
    if addrs.is_empty() {
        return Err("appareil introuvable sur le réseau local".into());
    }
    let name = p2p::pair(&addrs, &peer, &s.device_id, &s.device_name, &code).await.map_err(e)?;
    kick(&app);
    Ok(name)
}

#[tauri::command]
pub async fn p2p_forget(app: AppHandle, peer: String) -> R<()> {
    let ctl = app.state::<SyncCtl>();
    let _g = ctl.lock.lock().await;
    p2p::forget(&mut Store::load(), &peer).map_err(e)
}

// ---------- workspaces ----------

#[tauri::command]
pub async fn sync_set_workspace(app: AppHandle, tabs: Value) -> R<()> {
    let s = sync_settings(&app).sync;
    if s.mode == "off" {
        return Ok(());
    }
    {
        let ctl = app.state::<SyncCtl>();
        let _g = ctl.lock.lock().await;
        let mut store = Store::load();
        let key = format!("workspace:{}", s.device_id);
        let v = json!({ "name": s.device_name, "tabs": tabs });
        if store.records.get(&key).and_then(|r| r.v.as_ref()) == Some(&v) {
            return Ok(());
        }
        store.put(&key, Some(v), &s.device_id);
        store.save().map_err(e)?;
    }
    kick(&app);
    Ok(())
}

/// Other devices' workspaces, newest first.
#[tauri::command]
pub fn sync_workspaces(app: AppHandle) -> Vec<Value> {
    let me = format!("workspace:{}", sync_settings(&app).sync.device_id);
    let store = Store::load();
    let mut v: Vec<(i64, Value)> = store
        .records
        .iter()
        .filter(|(k, r)| k.starts_with("workspace:") && **k != me && r.v.as_ref().is_some_and(|v| v["tabs"].as_array().is_some_and(|t| !t.is_empty())))
        .map(|(_, r)| (r.ts, json!({ "name": r.v.as_ref().unwrap()["name"], "tabs": r.v.as_ref().unwrap()["tabs"], "ts": r.ts })))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, v)| v).collect()
}

// ---------- SSH keys in the vault ----------

#[tauri::command]
pub fn sync_keys() -> Vec<Value> {
    Store::load()
        .records
        .iter()
        .filter_map(|(k, r)| {
            let id = k.strip_prefix("key:")?;
            let v = r.v.as_ref()?;
            Some(json!({ "id": id, "name": v["name"], "pub": v["pub"] }))
        })
        .collect()
}

#[tauri::command]
pub async fn sync_key_import(app: AppHandle, path: String, name: String) -> R<String> {
    let pem = std::fs::read_to_string(&path).map_err(e)?;
    if !pem.contains("PRIVATE KEY") {
        return Err("ce fichier n'est pas une clé privée".into());
    }
    let public = russh::keys::decode_secret_key(&pem, None).ok().and_then(|k| k.public_key().to_openssh().ok()).unwrap_or_default();
    let vault = Vault::load().ok_or("déverrouillez d'abord le coffre de synchro")?;
    let dev = sync_settings(&app).sync.device_id;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let ctl = app.state::<SyncCtl>();
        let _g = ctl.lock.lock().await;
        let mut store = Store::load();
        store.put(&format!("key:{id}"), Some(json!({ "name": name, "pub": public, "enc": vault.seal(pem.as_bytes()) })), &dev);
        config::secret_set(&format!("key:{id}"), &pem).map_err(e)?;
        store.save().map_err(e)?;
    }
    kick(&app);
    Ok(id)
}

#[tauri::command]
pub async fn sync_key_delete(app: AppHandle, id: String) -> R<()> {
    let dev = sync_settings(&app).sync.device_id;
    {
        let ctl = app.state::<SyncCtl>();
        let _g = ctl.lock.lock().await;
        let mut store = Store::load();
        store.put(&format!("key:{id}"), None, &dev);
        let _ = config::secret_set(&format!("key:{id}"), "");
        store.save().map_err(e)?;
    }
    kick(&app);
    Ok(())
}
