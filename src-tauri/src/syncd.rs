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

// ---------- SSH keys ----------
// Every key's PEM is in the keychain as "key:<id>", the slot ssh::connect reads for `Host::key_id`. A synced key is
// also sealed in the vault (record "key:<id>"), a key of this device only is listed in keys.json. Moving a key from
// one to the other keeps its id, so the hosts using it don't change.

fn vault_keys(store: &Store) -> Vec<Value> {
    store
        .records
        .iter()
        .filter_map(|(k, r)| {
            let id = k.strip_prefix("key:")?;
            let v = r.v.as_ref()?;
            Some(json!({ "id": id, "name": v["name"], "pub": v["pub"], "synced": true }))
        })
        .collect()
}

/// Synced keys, then this device's own (`synced: false`).
#[tauri::command]
pub fn keys() -> Vec<Value> {
    let mut v = vault_keys(&Store::load());
    v.extend(config::local_keys().into_iter().map(|k| json!({ "id": k.id, "name": k.name, "pub": k.public, "synced": false })));
    v
}

/// Pasted keys often lose their line breaks (one long line with spaces) or gain \r, and russh then skips
/// the body silently: rebuild the layout it reads (BEGIN and END lines, base64 lines of 70 like OpenSSH).
pub fn normalize_pem(raw: &str) -> String {
    let s = raw.trim().replace("\r\n", "\n").replace('\r', "\n");
    let as_is = || format!("{s}\n");
    let Some(b) = s.find("-----BEGIN ") else { return as_is() }; // PuTTY .ppk and the like
    let Some(h) = s[b + 11..].find("-----").map(|i| b + 16 + i) else { return as_is() };
    let Some(e) = s[h..].find("-----END ").map(|i| h + i) else { return as_is() };
    let Some(end) = s[e + 9..].find("-----").map(|i| e + 14 + i) else { return as_is() };
    let body: String = s[h..e].chars().filter(|c| !c.is_whitespace()).collect();
    // Encrypted PKCS#1 keys carry "Proc-Type:"/"DEK-Info:" header lines: keep their layout.
    if s[h..e].contains(':') || !body.is_ascii() {
        return as_is();
    }
    let lines: Vec<&str> = body.as_bytes().chunks(70).map(|c| std::str::from_utf8(c).unwrap_or_default()).collect();
    format!("{}\n{}\n{}\n", &s[b..h], lines.join("\n"), &s[e..end])
}

/// A private key as text (pasted) or from a file. `sync`: sealed in the vault for every device, else this one only.
#[tauri::command]
pub async fn key_import(app: AppHandle, text: Option<String>, path: Option<String>, name: String, sync: bool) -> R<String> {
    let raw = match (text.filter(|t| !t.trim().is_empty()), path) {
        (Some(t), _) => t,
        (None, Some(p)) => std::fs::read_to_string(&p).map_err(|_| "fichier illisible : ce n'est pas une clé privée au format texte".to_string())?,
        (None, None) => return Err("rien à importer".into()),
    };
    if sync && Vault::load().is_none() {
        return Err("déverrouillez d'abord le coffre de synchro".into());
    }
    let pem = normalize_pem(&raw);
    // Checked now rather than at the first connection; a passphrase-protected key is read with the host's passphrase.
    let public = match russh::keys::decode_secret_key(&pem, None) {
        Ok(k) => k.public_key().to_openssh().unwrap_or_default(),
        Err(russh::keys::Error::KeyIsEncrypted) => String::new(),
        Err(err) => return Err(format!("clé privée illisible ({err})")),
    };
    let name = match name.trim() {
        // The key's comment (user@machine), else its type.
        "" => public.split(' ').nth(2).or(public.split(' ').next()).filter(|n| !n.is_empty()).unwrap_or("Clé SSH").to_string(),
        n => n.to_string(),
    };
    let id = uuid::Uuid::new_v4().to_string();
    config::secret_set(&format!("key:{id}"), &pem).map_err(e)?;
    let mut local = config::local_keys();
    local.push(config::LocalKey { id: id.clone(), name, public });
    config::save_local_keys(&local).map_err(e)?;
    if sync {
        keys_set_synced(app, vec![id.clone()], true).await?;
    }
    Ok(id)
}

/// Moves keys into the vault (`synced`) or back to this device only, where the other devices lose them.
#[tauri::command]
pub async fn keys_set_synced(app: AppHandle, ids: Vec<String>, synced: bool) -> R<()> {
    let dev = sync_settings(&app).sync.device_id;
    let vault = if synced { Some(Vault::load().ok_or("déverrouillez d'abord le coffre de synchro")?) } else { None };
    {
        let ctl = app.state::<SyncCtl>();
        let _g = ctl.lock.lock().await;
        let mut store = Store::load();
        let mut local = config::local_keys();
        move_keys(&mut store, &mut local, &ids, vault.as_ref(), &config::secret_get, &dev)?;
        // The side that gains the key is written first: a crash in between leaves it listed twice, never lost.
        if synced {
            store.save().map_err(e)?;
            config::save_local_keys(&local).map_err(e)?;
        } else {
            config::save_local_keys(&local).map_err(e)?;
            store.save().map_err(e)?;
        }
    }
    kick(&app);
    Ok(())
}

/// The bookkeeping of keys_set_synced: vault records and the local list (the keychain slot never changes).
fn move_keys(store: &mut Store, local: &mut Vec<config::LocalKey>, ids: &[String], vault: Option<&Vault>, secret: &dyn Fn(&str) -> Option<String>, dev: &str) -> R<()> {
    for id in ids {
        let slot = format!("key:{id}");
        if let Some(vault) = vault {
            let Some(i) = local.iter().position(|k| &k.id == id) else { continue }; // already synced
            let pem = secret(&slot).ok_or("clé absente du trousseau de cet appareil")?;
            let k = local.remove(i);
            store.put(&slot, Some(json!({ "name": k.name, "pub": k.public, "enc": vault.seal(pem.as_bytes()) })), dev);
        } else {
            let Some(v) = store.records.get(&slot).and_then(|r| r.v.clone()) else { continue }; // already local
            if secret(&slot).is_none() {
                return Err(format!("« {} » n'est pas encore sur cet appareil : déverrouillez le coffre d'abord", v["name"].as_str().unwrap_or(id)));
            }
            let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
            local.push(config::LocalKey { id: id.clone(), name: text("name"), public: text("pub") });
            store.put(&slot, None, dev);
        }
    }
    Ok(())
}

/// Deletes a key from this device, and from every device when it is synced.
#[tauri::command]
pub async fn key_delete(app: AppHandle, id: String) -> R<()> {
    let mut local = config::local_keys();
    if let Some(i) = local.iter().position(|k| k.id == id) {
        local.remove(i);
        config::save_local_keys(&local).map_err(e)?;
        return config::secret_set(&format!("key:{id}"), "").map_err(e);
    }
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

#[cfg(test)]
mod key_tests {
    use super::normalize_pem;
    use russh::keys::ssh_key::{private::Ed25519Keypair, LineEnding, PrivateKey};

    #[test]
    fn pasted_key_without_line_breaks_parses() {
        let pem = PrivateKey::from(Ed25519Keypair::from_seed(&[7; 32])).to_openssh(LineEnding::LF).unwrap();
        assert_eq!(normalize_pem(&pem), *pem, "a well-formed key stays as it is");
        let pasted = format!("  {}\r\n", pem.replace('\n', " "));
        assert_eq!(normalize_pem(&pasted), *pem);
        assert!(russh::keys::decode_secret_key(&normalize_pem(&pasted), None).is_ok());
        assert_eq!(normalize_pem("not a key"), "not a key\n");
    }
    #[test]
    fn key_moves_to_the_vault_and_back_with_the_same_id() {
        use crate::sync::{Store, Vault};
        let vault = Vault::new(&[3; 32]);
        let (mut store, mut local) = (Store::default(), vec![crate::config::LocalKey { id: "k1".into(), name: "laptop".into(), public: "ssh-ed25519 AAA me".into() }]);
        let secret = |slot: &str| (slot == "key:k1").then(|| "PEM".to_string());
        let ids = ["k1".to_string()];
        super::move_keys(&mut store, &mut local, &ids, Some(&vault), &secret, "dev").unwrap();
        assert!(local.is_empty());
        let rec = store.records["key:k1"].v.clone().unwrap();
        assert_eq!((rec["name"].as_str(), vault.open(rec["enc"].as_str().unwrap()).as_deref()), (Some("laptop"), Some(&b"PEM"[..])));
        super::move_keys(&mut store, &mut local, &ids, None, &secret, "dev").unwrap();
        assert_eq!((local.len(), local[0].id.as_str(), local[0].name.as_str()), (1, "k1", "laptop"));
        assert!(store.records["key:k1"].v.is_none(), "a tombstone takes it off the other devices");
        // A synced key this device can't read yet (vault never unlocked here) stays synced.
        store.put("key:k2", Some(serde_json::json!({ "name": "nas" })), "other");
        assert!(super::move_keys(&mut store, &mut local, &["k2".to_string()], None, &secret, "dev").is_err());
    }
}
