//! Synchronisation of hosts, settings, secrets, SSH keys and workspaces between devices, through a
//! Termix sync server or directly over the local network (see p2p.rs).
//!
//! Everything is a record keyed `kind:id` (`host:…`, `folder:…`, `settings`, `secret:…`, `key:…`,
//! `workspace:<device>`, `vault:check`). The newest `(ts, dev)` wins per record and deletions are
//! tombstones (`v = null`). Secrets and keys are end-to-end encrypted: AES-256-GCM with a key derived
//! from the user's passphrase (PBKDF2-HMAC-SHA256, 600 000 rounds, per-account salt). The sync server
//! and its web UI (WebCrypto) use the exact same format, so the server never sees a password or a key.
use crate::config::{self, Folder, Host, HostsDoc, Settings};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, bail, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const PBKDF2_ROUNDS: u32 = 600_000;
/// Plaintext of the `vault:check` record: proves a passphrase is the account's one.
const CHECK: &[u8] = b"termix-vault";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rec {
    pub v: Option<Value>,
    pub ts: i64,
    pub dev: String,
}

pub type Records = BTreeMap<String, Rec>;

fn wins(a: &Rec, b: &Rec) -> bool {
    (a.ts, a.dev.as_str()) > (b.ts, b.dev.as_str())
}

/// Last-writer-wins merge of `incoming` into `into`. Returns the keys that changed.
pub fn merge(into: &mut Records, incoming: &Records) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    for (k, r) in incoming {
        if into.get(k).is_none_or(|cur| wins(r, cur)) {
            into.insert(k.clone(), r.clone());
            changed.insert(k.clone());
        }
    }
    changed
}

/// Both versions of a record edited on two devices between two syncs. Last-writer-wins already picked one;
/// the user can keep the other.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Conflict {
    pub mine: Rec,
    pub theirs: Rec,
}

/// Records the user edits by hand: hosts, folders, settings (secrets are opaque, workspaces per device).
fn user_edited(key: &str) -> bool {
    key == "settings" || key.starts_with("host:") || key.starts_with("folder:")
}

/// Merges records from another device into the store, noting a conflict when one of them overwrites (or is
/// beaten by) a change this device has not delivered yet.
pub fn merge_into(store: &mut Store, incoming: &Records, me: &str) -> BTreeSet<String> {
    for (k, theirs) in incoming {
        let Some(mine) = store.records.get(k) else { continue };
        if user_edited(k) && store.dirty.contains(k) && mine.dev == me && theirs.dev != me && theirs.v != mine.v {
            store.conflicts.insert(k.clone(), Conflict { mine: mine.clone(), theirs: theirs.clone() });
        }
    }
    merge(&mut store.records, incoming)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ---------- local store (sync.json) ----------

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Store {
    pub records: Records,
    /// Keys changed on this device and not yet accepted by the server.
    pub dirty: BTreeSet<String>,
    /// Last server sequence number pulled.
    pub server_seq: i64,
    /// Vault salt (base64), shared by every device of the account.
    pub salt: String,
    /// `secret:` keys this device holds in its keychain. Only those can be deleted from here: a device
    /// without a keychain (or still locked) must never wipe everyone's secrets.
    pub local_secrets: BTreeSet<String>,
    pub peers: Vec<Peer>,
    pub last_ok: i64,
    pub last_error: String,
    /// Concurrent edits waiting for the user's choice.
    pub conflicts: BTreeMap<String, Conflict>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Peer {
    pub id: String,
    pub name: String,
}

impl Store {
    fn path() -> std::path::PathBuf {
        config::dir().join("sync.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self) -> Result<()> {
        config::write_atomic(&Self::path(), &serde_json::to_vec(self)?)
    }
    pub fn reset() {
        let _ = std::fs::remove_file(Self::path());
    }
    pub fn put(&mut self, key: &str, v: Option<Value>, dev: &str) {
        self.records.insert(key.into(), Rec { v, ts: now_ms(), dev: dev.into() });
        self.dirty.insert(key.into());
    }
}

// ---------- vault (end-to-end encryption) ----------

pub struct Vault(Aes256Gcm);

impl Vault {
    pub fn derive(passphrase: &str, salt_b64: &str) -> Result<[u8; 32]> {
        let salt = B64.decode(salt_b64)?;
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(passphrase.as_bytes(), &salt, PBKDF2_ROUNDS, &mut key);
        Ok(key)
    }
    pub fn new(key: &[u8; 32]) -> Self {
        Vault(Aes256Gcm::new(key.into()))
    }
    /// base64(nonce ‖ ciphertext ‖ tag), the format WebCrypto produces too.
    pub fn seal(&self, plain: &[u8]) -> String {
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ct = self.0.encrypt(Nonce::from_slice(&nonce), plain).expect("AES-GCM encryption cannot fail");
        let mut out = nonce.to_vec();
        out.extend(ct);
        B64.encode(out)
    }
    pub fn open(&self, sealed: &str) -> Option<Vec<u8>> {
        let b = B64.decode(sealed).ok()?;
        if b.len() < 12 + 16 {
            return None;
        }
        self.0.decrypt(Nonce::from_slice(&b[..12]), &b[12..]).ok()
    }
    fn open_rec(&self, r: Option<&Rec>) -> Option<Vec<u8>> {
        self.open(r?.v.as_ref()?.get("enc")?.as_str()?)
    }
    /// The unlocked vault key of this device (kept in the OS keychain).
    pub fn load() -> Option<Self> {
        let k = B64.decode(config::secret_get("sync:vault")?).ok()?;
        Some(Vault::new(&k.try_into().ok()?))
    }
}

pub fn random_salt() -> String {
    let mut s = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut s);
    B64.encode(s)
}

/// Verifies the passphrase key against the account's `vault:check` record, creating it on first use.
pub fn check_or_init(store: &mut Store, key: &[u8; 32], dev: &str) -> Result<()> {
    let vault = Vault::new(key);
    match store.records.get("vault:check").filter(|r| r.v.is_some()) {
        Some(r) => {
            if vault.open_rec(Some(r)).as_deref() != Some(CHECK) {
                bail!("phrase secrète incorrecte");
            }
        }
        None => store.put("vault:check", Some(json!({ "enc": vault.seal(CHECK) })), dev),
    }
    Ok(())
}

// ---------- local state <-> records ----------

fn synced_settings(s: &Settings) -> Value {
    let keys: BTreeMap<_, _> = s.keybindings.iter().map(|(a, k)| (a, portable_key(k, MAC))).collect();
    json!({ "appearance": s.appearance, "terminal": s.terminal, "keybindings": keys, "themes": s.themes })
}

const MAC: bool = cfg!(target_os = "macos");

/// Shortcuts travel with "Mod" for the system's main modifier: Cmd on macOS, Ctrl+Shift elsewhere
/// (plain Ctrl keys belong to the remote shell), so Cmd+T on a Mac is Ctrl+Shift+T on Windows/Linux.
pub fn portable_key(k: &str, mac: bool) -> String {
    let (mut mods, key) = split_key(k, mac);
    if mac && mods.remove("Cmd") || !mac && mods.contains("Ctrl") && mods.contains("Shift") && mods.remove("Ctrl") && mods.remove("Shift") {
        mods.insert("Mod");
    }
    join_key(mods, &key)
}

/// The other way. shortcut: Cmd+Shift+X becomes Ctrl+Shift+X like Cmd+X (Windows/Linux can't tell them apart).
pub fn native_key(k: &str, mac: bool) -> String {
    let (mut mods, key) = split_key(k, mac);
    if mods.remove("Mod") {
        if mac { mods.insert("Cmd"); } else { mods.extend(["Ctrl", "Shift"]); }
    }
    join_key(mods, &key)
}

fn split_key(k: &str, mac: bool) -> (BTreeSet<&'static str>, String) {
    let mut parts: Vec<&str> = k.split('+').collect();
    let key = parts.pop().unwrap_or_default().to_string();
    let mods = parts.iter().filter_map(|m| Some(match m.to_lowercase().as_str() {
        "mod" => "Mod",
        "cmd" | "command" | "meta" => if mac { "Cmd" } else { "Super" },
        "super" | "win" => if mac { "Cmd" } else { "Super" },
        "cmdorctrl" => if mac { "Cmd" } else { "Ctrl" },
        "ctrl" | "control" => "Ctrl",
        "alt" | "option" => "Alt",
        "shift" => "Shift",
        _ => return None,
    })).collect();
    (mods, key)
}

/// Same order as the shortcut recorder in the UI (accelFromEvent).
fn join_key(mods: BTreeSet<&str>, key: &str) -> String {
    let order = ["Mod", "Cmd", "Super", "Ctrl", "Alt", "Shift"];
    order.iter().filter(|m| mods.contains(*m)).copied().chain([key]).collect::<Vec<_>>().join("+")
}

/// Synced shortcuts in this system's form, completed with the defaults of actions they don't name.
pub fn native_keys(v: &Value) -> Option<BTreeMap<String, String>> {
    let synced: BTreeMap<String, String> = serde_json::from_value(v.clone()).ok()?;
    let mut keys = config::default_keybindings();
    keys.extend(synced.iter().map(|(a, k)| (a.clone(), native_key(k, MAC))));
    Some(keys)
}

fn secret_ids(doc: &HostsDoc) -> impl Iterator<Item = String> + '_ {
    doc.hosts.iter().flat_map(|h| [h.id.clone(), format!("{}:passphrase", h.id)])
}

/// Local state → records: changed values get a fresh timestamp, vanished ones a tombstone.
pub fn record_local(
    store: &mut Store,
    doc: &HostsDoc,
    settings: &Settings,
    vault: Option<&Vault>,
    secret: &dyn Fn(&str) -> Option<String>,
    file: &dyn Fn(&str) -> Option<Vec<u8>>,
    dev: &str,
) {
    let mut want: BTreeMap<String, Value> = BTreeMap::new();
    for h in &doc.hosts {
        want.insert(format!("host:{}", h.id), json!(h));
    }
    for f in &doc.folders {
        want.insert(format!("folder:{}", f.id), json!(f));
    }
    let mut synced = synced_settings(settings);
    if let Some(old) = store.records.get("settings").and_then(|r| r.v.as_ref()) {
        // Shortcuts kept on this device, or the same as the synced ones once adapted to this system: not a change
        // (Cmd+Shift+D from a Mac reads back as Ctrl+Shift+D here, which must not overwrite the Mac's).
        let mine: BTreeMap<String, String> = settings.keybindings.iter().map(|(a, k)| (a.clone(), native_key(&portable_key(k, MAC), MAC))).collect();
        if settings.sync.keybindings == "local" || native_keys(&old["keybindings"]).as_ref() == Some(&mine) {
            synced["keybindings"] = old["keybindings"].clone();
        }
    }
    want.insert("settings".into(), synced);
    for (k, v) in &want {
        if store.records.get(k).and_then(|r| r.v.as_ref()) != Some(v) {
            store.put(k, Some(v.clone()), dev);
        }
    }
    let gone: Vec<String> = store
        .records
        .iter()
        .filter(|(k, r)| r.v.is_some() && (k.starts_with("host:") || k.starts_with("folder:")) && !want.contains_key(*k))
        .map(|(k, _)| k.clone())
        .collect();
    for k in gone {
        store.put(&k, None, dev);
    }

    // Secrets need the vault: without it they are neither sent nor deleted.
    let Some(vault) = vault else { return };
    let mut present = BTreeSet::new();
    for id in secret_ids(doc) {
        let key = format!("secret:{id}");
        let Some(plain) = secret(&id) else { continue };
        present.insert(key.clone());
        if vault.open_rec(store.records.get(&key)).as_deref() != Some(plain.as_bytes()) {
            store.put(&key, Some(json!({ "enc": vault.seal(plain.as_bytes()) })), dev);
        }
    }
    let removed: Vec<String> = store.local_secrets.difference(&present).cloned().collect();
    for k in removed {
        if store.records.get(&k).is_some_and(|r| r.v.is_some()) {
            store.put(&k, None, dev);
        }
    }
    store.local_secrets = present;

    // The terminal background image travels too (encrypted, under its content-hash name), only the current one.
    let bg = settings.terminal.background_image.as_str();
    let want = crate::background::is_name(bg).then(|| format!("file:{bg}"));
    let old: Vec<String> = store
        .records
        .iter()
        .filter(|(k, r)| k.starts_with("file:") && r.v.is_some() && Some(*k) != want.as_ref())
        .map(|(k, _)| k.clone())
        .collect();
    for k in old {
        store.put(&k, None, dev);
    }
    if let Some(key) = want {
        // Missing on the server (never sent, or dropped by a device that switched image meanwhile): send it.
        if store.records.get(&key).is_none_or(|r| r.v.is_none()) {
            if let Some(bytes) = file(bg) {
                store.put(&key, Some(json!({ "enc": vault.seal(&bytes) })), dev);
            }
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Applied {
    pub hosts: bool,
    pub settings: bool,
}

fn values<T: serde::de::DeserializeOwned>(store: &Store, prefix: &str) -> BTreeMap<String, T> {
    store
        .records
        .iter()
        .filter_map(|(k, r)| Some((k.strip_prefix(prefix)?.to_string(), serde_json::from_value(r.v.clone()?).ok()?)))
        .collect()
}

/// Keeps the local order of known items, replaces their content, appends new ones by name.
fn reorder<T: Clone>(local: &[T], mut remote: BTreeMap<String, T>, id: impl Fn(&T) -> &str, name: impl Fn(&T) -> String) -> Vec<T> {
    let mut out: Vec<T> = local.iter().filter_map(|x| remote.remove(id(x))).collect();
    let mut rest: Vec<T> = remote.into_values().collect();
    rest.sort_by_key(|x| name(x).to_lowercase());
    out.extend(rest);
    out
}

/// Records → hosts document and synced settings. Pure, so it can be tested without disk or keychain.
pub fn materialize(store: &Store, doc: &HostsDoc, settings: &Settings) -> (HostsDoc, Settings) {
    let hosts = reorder(&doc.hosts, values::<Host>(store, "host:"), |h| &h.id, |h| h.name.clone());
    let folders = reorder(&doc.folders, values::<Folder>(store, "folder:"), |f| &f.id, |f| f.name.clone());
    let mut s = settings.clone();
    if let Some(v) = store.records.get("settings").and_then(|r| r.v.as_ref()) {
        if let Ok(a) = serde_json::from_value(v["appearance"].clone()) {
            s.appearance = a;
        }
        if let Ok(t) = serde_json::from_value(v["terminal"].clone()) {
            s.terminal = t;
        }
        if settings.sync.keybindings != "local" {
            if let Some(k) = native_keys(&v["keybindings"]) {
                s.keybindings = k;
            }
        }
        if let Ok(t) = serde_json::from_value(v["themes"].clone()) {
            s.themes = t;
        }
    }
    (HostsDoc { folders, hosts }, s)
}

/// Writes the merged records to this device: hosts.json, config.toml, keychain.
pub fn apply(store: &mut Store, vault: Option<&Vault>, settings: &Settings) -> Result<(Applied, Settings)> {
    let doc = HostsDoc::load();
    let (new_doc, new_settings) = materialize(store, &doc, settings);
    let mut applied = Applied::default();
    if json!(new_doc) != json!(doc) {
        new_doc.save()?;
        applied.hosts = true;
    }
    if synced_settings(&new_settings) != synced_settings(settings) {
        new_settings.save()?;
        applied.settings = true;
    }
    if let Some(vault) = vault {
        let keys: Vec<(String, Option<Value>)> = store
            .records
            .iter()
            .filter(|(k, _)| k.starts_with("secret:") || k.starts_with("key:"))
            .map(|(k, r)| (k.clone(), r.v.clone()))
            .collect();
        for (k, v) in keys {
            // Secrets live under their host id, keys under "key:<id>".
            let slot = k.strip_prefix("secret:").unwrap_or(&k).to_string();
            match v {
                Some(v) => {
                    let Some(plain) = v.get("enc").and_then(Value::as_str).and_then(|e| vault.open(e)) else { continue };
                    let plain = String::from_utf8_lossy(&plain).into_owned();
                    if config::secret_get(&slot).as_deref() != Some(&plain) && config::secret_set(&slot, &plain).is_err() {
                        continue;
                    }
                    if k.starts_with("secret:") {
                        store.local_secrets.insert(k);
                    }
                }
                None => {
                    if config::secret_get(&slot).is_some() {
                        let _ = config::secret_set(&slot, "");
                    }
                    store.local_secrets.remove(&k);
                }
            }
        }
        // Background image named by the settings but not on this device yet.
        let bg = new_settings.terminal.background_image.as_str();
        if crate::background::is_name(bg) && crate::background::read(bg).is_none() {
            let enc = store.records.get(&format!("file:{bg}")).and_then(|r| r.v.as_ref()?.get("enc")?.as_str().map(String::from));
            if let Some(bytes) = enc.and_then(|e| vault.open(&e)) {
                let ext = bg.rsplit('.').next().unwrap_or_default();
                if crate::background::name_for(&bytes, ext) == bg {
                    crate::background::save(bg, &bytes)?;
                    applied.settings = true; // the UI loads it
                }
            }
        }
    }
    Ok((applied, new_settings))
}

// ---------- sync server client ----------

fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().unwrap_or_default()
}

fn base(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    // "sync.example.com" is a relative URL for reqwest ("builder error"): assume HTTPS.
    if u.contains("://") { u.into() } else { format!("https://{u}") }
}

async fn checked(r: reqwest::Response) -> Result<reqwest::Response> {
    let status = r.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        bail!("session expirée ou identifiants refusés : reconnectez-vous");
    }
    if !status.is_success() {
        let body = r.text().await.unwrap_or_default();
        bail!("{}", http_error(status.as_u16(), &body));
    }
    Ok(r)
}

/// One readable line: a proxy in front of a down server (Cloudflare 52x, nginx 502…) answers with a whole HTML page.
fn http_error(code: u16, body: &str) -> String {
    let body = body.trim();
    let detail = if body.starts_with('<') {
        body.split_once("<title>").and_then(|(_, t)| t.split_once("</title>")).map_or("", |(t, _)| t.trim()).to_string()
    } else {
        body.chars().take(200).collect()
    };
    let what = if matches!(code, 502..=504 | 520..=530) { "serveur de synchro injoignable" } else { "serveur de synchro" };
    if detail.is_empty() { format!("{what} (HTTP {code})") } else { format!("{what} (HTTP {code} : {detail})") }
}

/// Returns (token, vault salt).
pub async fn server_login(url: &str, user: &str, password: &str, device: &str) -> Result<(String, String)> {
    let r = http()
        .post(format!("{}/api/login", base(url)))
        .json(&json!({ "user": user, "password": password, "device": device }))
        .send()
        .await?;
    let v: Value = checked(r).await?.json().await?;
    let token = v["token"].as_str().ok_or_else(|| anyhow!("réponse du serveur invalide"))?;
    Ok((token.into(), v["salt"].as_str().unwrap_or_default().into()))
}

/// Pulls everything newer than the last pull (conflicts are spotted while our changes are still dirty),
/// then pushes local changes.
pub async fn server_sync(url: &str, token: &str, store: &mut Store, me: &str) -> Result<BTreeSet<String>> {
    let c = http();
    let v: Value =
        checked(c.get(format!("{}/api/records?after={}", base(url), store.server_seq)).bearer_auth(token).send().await?).await?.json().await?;
    let incoming = parse_records(&v["records"]);
    store.server_seq = v["seq"].as_i64().unwrap_or(store.server_seq);
    let changed = merge_into(store, &incoming, me);
    if !store.dirty.is_empty() {
        let recs: Vec<Value> = store
            .dirty
            .iter()
            .filter_map(|k| store.records.get(k).map(|r| json!({ "key": k, "v": r.v, "ts": r.ts, "dev": r.dev })))
            .collect();
        checked(c.post(format!("{}/api/records", base(url))).bearer_auth(token).json(&json!({ "records": recs })).send().await?).await?;
        store.dirty.clear();
    }
    Ok(changed)
}

pub fn parse_records(v: &Value) -> Records {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let key = r["key"].as_str()?.to_string();
            let v = Some(r["v"].clone()).filter(|v| !v.is_null());
            Some((key, Rec { v, ts: r["ts"].as_i64()?, dev: r["dev"].as_str().unwrap_or_default().into() }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_defaults_to_https() {
        assert_eq!(base(" sync.example.com/ "), "https://sync.example.com");
        assert_eq!(base("http://10.0.0.2:8080/"), "http://10.0.0.2:8080");
    }

    #[tokio::test]
    async fn network_errors_show_their_cause() {
        let err = server_login("http://127.0.0.1:1", "u", "p", "d").await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains(": "), "{msg}");
        println!("{msg}");
    }

    #[test]
    fn shortcuts_are_adapted_to_each_system() {
        assert_eq!(portable_key("Cmd+T", true), "Mod+T");
        assert_eq!(portable_key("Ctrl+Shift+T", false), "Mod+T");
        assert_eq!(native_key("Mod+T", false), "Ctrl+Shift+T");
        assert_eq!(native_key("Mod+T", true), "Cmd+T");
        assert_eq!(native_key(&portable_key("Cmd+Alt+Right", true), false), "Ctrl+Alt+Shift+Right");
        assert_eq!(native_key(&portable_key("Ctrl+Alt+Shift+Right", false), true), "Cmd+Alt+Right");
        // Plain Ctrl, and keys that are themselves "+"-free symbols, pass through.
        assert_eq!(native_key(&portable_key("Ctrl+K", false), true), "Ctrl+K");
        assert_eq!(native_key(&portable_key("Option+Cmd+,", true), false), "Ctrl+Alt+Shift+,");
    }

    #[test]
    fn adapted_shortcuts_dont_bounce_between_systems() {
        let mut store = Store::default();
        let mut s = Settings { keybindings: config::default_keybindings(), ..Default::default() };
        record_local(&mut store, &HostsDoc::default(), &s, None, &|_| None, &|_| None, "me");
        let ts = store.records["settings"].ts;
        // A record whose shortcuts read back as ours (other spelling) is left alone.
        let mut v = store.records["settings"].v.clone().unwrap();
        v["keybindings"]["new_tab"] = json!(if MAC { "Command+T" } else { "Mod+Shift+T" });
        store.records.insert("settings".into(), rec(v.clone(), ts, "other"));
        record_local(&mut store, &HostsDoc::default(), &s, None, &|_| None, &|_| None, "me");
        assert_eq!(store.records["settings"].dev, "other");
        // Device-local shortcuts: never sent, never applied.
        s.sync.keybindings = "local".into();
        s.keybindings.insert("new_tab".into(), "Alt+Y".into());
        record_local(&mut store, &HostsDoc::default(), &s, None, &|_| None, &|_| None, "me");
        assert_eq!(store.records["settings"].dev, "other");
        assert_eq!(materialize(&store, &HostsDoc::default(), &s).1.keybindings["new_tab"], "Alt+Y");
    }

    #[test]
    fn html_error_pages_are_summed_up() {
        let page = "<!DOCTYPE html><html><head><title>521 - Web server is down</title><style>h1{}</style></head><body>…</body></html>";
        assert_eq!(http_error(521, page), "serveur de synchro injoignable (HTTP 521 : 521 - Web server is down)");
        assert_eq!(http_error(502, "<html><body>Bad</body></html>"), "serveur de synchro injoignable (HTTP 502)");
        assert_eq!(http_error(409, "compte déjà existant"), "serveur de synchro (HTTP 409 : compte déjà existant)");
    }

    fn rec(v: Value, ts: i64, dev: &str) -> Rec {
        Rec { v: Some(v), ts, dev: dev.into() }
    }

    #[test]
    fn newest_record_wins_and_ties_are_stable() {
        let mut a: Records = [("host:1".to_string(), rec(json!("old"), 10, "a"))].into();
        let b: Records = [("host:1".to_string(), rec(json!("new"), 20, "b")), ("host:2".to_string(), rec(json!("x"), 5, "b"))].into();
        assert_eq!(merge(&mut a, &b).len(), 2);
        assert_eq!(a["host:1"].v, Some(json!("new")));
        // Older or equal records never overwrite.
        let stale: Records = [("host:1".to_string(), rec(json!("stale"), 15, "z"))].into();
        assert!(merge(&mut a, &stale).is_empty());
        let same: Records = [("host:1".to_string(), rec(json!("new"), 20, "b"))].into();
        assert!(merge(&mut a, &same).is_empty());
    }

    #[test]
    fn concurrent_edits_are_kept_as_conflicts() {
        let mut store = Store::default();
        store.put("host:1", Some(json!("mine")), "me");
        store.put("folder:1", Some(json!("f")), "me");
        let theirs: Records = [
            ("host:1".to_string(), rec(json!("theirs"), i64::MAX, "other")),
            // Same value on both sides, or a change this device already delivered: no conflict.
            ("folder:1".to_string(), rec(json!("f"), i64::MAX, "other")),
        ]
        .into();
        merge_into(&mut store, &theirs, "me");
        assert_eq!(store.records["host:1"].v, Some(json!("theirs")));
        assert_eq!(store.conflicts.keys().collect::<Vec<_>>(), ["host:1"]);
        assert_eq!(store.conflicts["host:1"].mine.v, Some(json!("mine")));
        store.dirty.clear();
        store.conflicts.clear();
        merge_into(&mut store, &[("host:1".to_string(), rec(json!("later"), i64::MAX, "z"))].into(), "me");
        assert!(store.conflicts.is_empty());
    }

    #[test]
    fn vault_roundtrip_and_wrong_key() {
        let salt = random_salt();
        let k = Vault::derive("correct horse", &salt).unwrap();
        let v = Vault::new(&k);
        let sealed = v.seal(b"hunter2");
        assert_eq!(v.open(&sealed).unwrap(), b"hunter2");
        let other = Vault::new(&Vault::derive("wrong", &salt).unwrap());
        assert!(other.open(&sealed).is_none());
        let mut store = Store::default();
        check_or_init(&mut store, &k, "a").unwrap();
        assert!(check_or_init(&mut store, &k, "b").is_ok());
        assert!(check_or_init(&mut store, &Vault::derive("wrong", &salt).unwrap(), "b").is_err());
    }

    #[test]
    fn opens_what_the_web_ui_sealed_with_webcrypto() {
        // Produced by the sync server's web UI code path (WebCrypto PBKDF2 + AES-GCM).
        let key = Vault::derive("correct horse battery", "AwMDAwMDAwMDAwMDAwMDAw==").unwrap();
        let plain = Vault::new(&key).open("CQkJCQkJCQkJCQkJkEqJyPNoTC8tU3j/aryuiCuij6p1537WW1IwvzFugnntV+p2").unwrap();
        assert_eq!(plain, b"hello from webcrypto");
    }

    /// Against a running termix-sync: TERMIX_TEST_SERVER=http://127.0.0.1:18080 cargo test -- --ignored live_server
    #[tokio::test]
    #[ignore]
    async fn live_server_roundtrip() {
        let url = std::env::var("TERMIX_TEST_SERVER").unwrap();
        let (tok, salt) = server_login(&url, "admin", "adminpass1", "test").await.unwrap();
        assert!(!salt.is_empty());
        let mut a = Store::default();
        a.put("host:live", Some(json!({ "id": "live", "name": "from a" })), "a");
        server_sync(&url, &tok, &mut a, "a").await.unwrap();
        assert!(a.dirty.is_empty());
        let mut b = Store::default();
        let changed = server_sync(&url, &tok, &mut b, "b").await.unwrap();
        assert!(changed.contains("host:live"));
        assert_eq!(b.records["host:live"].v, a.records["host:live"].v);
        assert!(server_login(&url, "admin", "nope", "test").await.is_err());
    }

    #[test]
    fn local_changes_become_records_and_deletions_tombstones() {
        let mut store = Store::default();
        let mut doc = HostsDoc::default();
        doc.hosts.push(Host { id: "h1".into(), name: "web".into(), ..Default::default() });
        let s = Settings::default();
        record_local(&mut store, &doc, &s, None, &|_| None, &|_| None, "a");
        assert!(store.records["host:h1"].v.is_some());
        let ts = store.records["host:h1"].ts;
        // Unchanged: no new timestamp.
        record_local(&mut store, &doc, &s, None, &|_| None, &|_| None, "a");
        assert_eq!(store.records["host:h1"].ts, ts);
        doc.hosts.clear();
        record_local(&mut store, &doc, &s, None, &|_| None, &|_| None, "a");
        assert_eq!(store.records["host:h1"].v, None);
    }

    #[test]
    fn secrets_are_encrypted_and_only_deleted_by_a_device_that_had_them() {
        let vault = Vault::new(&[7u8; 32]);
        let mut doc = HostsDoc::default();
        doc.hosts.push(Host { id: "h1".into(), ..Default::default() });
        let s = Settings::default();
        // A device that never had the secret (no keychain, or not applied yet) must not tombstone it.
        let mut store = Store::default();
        store.records.insert("secret:h1".into(), rec(json!({ "enc": vault.seal(b"pw") }), 1, "other"));
        record_local(&mut store, &doc, &s, Some(&vault), &|_| None, &|_| None, "a");
        assert!(store.records["secret:h1"].v.is_some());
        // A device that had it and lost it (user cleared the password) propagates the deletion.
        record_local(&mut store, &doc, &s, Some(&vault), &|id| (id == "h1").then(|| "pw".to_string()), &|_| None, "a");
        assert!(store.local_secrets.contains("secret:h1"));
        let sealed = store.records["secret:h1"].v.clone().unwrap();
        assert!(!sealed.to_string().contains("pw\""), "secret must be encrypted");
        record_local(&mut store, &doc, &s, Some(&vault), &|_| None, &|_| None, "a");
        assert_eq!(store.records["secret:h1"].v, None);
    }

    #[test]
    fn background_image_travels_encrypted_once_and_old_ones_are_dropped() {
        let vault = Vault::new(&[7u8; 32]);
        let doc = HostsDoc::default();
        let img = b"\x89PNG not really an image".to_vec();
        let name = crate::background::name_for(&img, "png");
        let key = format!("file:{name}");
        let mut s = Settings::default();
        s.terminal.background_image = name.clone();
        let file = |n: &str| (n == name).then(|| img.clone());
        let mut store = Store::default();
        // Without the vault nothing is sent (like passwords).
        record_local(&mut store, &doc, &s, None, &|_| None, &file, "a");
        assert!(!store.records.contains_key(&key));
        record_local(&mut store, &doc, &s, Some(&vault), &|_| None, &file, "a");
        let sealed = store.records[&key].v.clone().unwrap();
        assert!(!sealed.to_string().contains("not really"), "image must be encrypted");
        assert_eq!(vault.open(sealed["enc"].as_str().unwrap()).unwrap(), img);
        // Sent once: later rounds leave it alone.
        store.dirty.clear();
        record_local(&mut store, &doc, &s, Some(&vault), &|_| None, &file, "a");
        assert!(!store.dirty.contains(&key));
        // Another background (here a path on this machine, never synced): the old file leaves the sync.
        s.terminal.background_image = "/home/me/pic.png".into();
        record_local(&mut store, &doc, &s, Some(&vault), &|_| None, &file, "a");
        assert_eq!(store.records[&key].v, None);
    }

    #[test]
    fn materialize_keeps_local_order_and_appends_new_hosts() {
        let mut store = Store::default();
        for (id, name) in [("b", "beta"), ("a", "alpha"), ("c", "gamma")] {
            store.records.insert(format!("host:{id}"), rec(json!(Host { id: id.into(), name: name.into(), ..Default::default() }), 1, "x"));
        }
        store.records.insert("host:gone".into(), Rec { v: None, ts: 2, dev: "x".into() });
        let local = HostsDoc { folders: vec![], hosts: vec![Host { id: "c".into(), ..Default::default() }, Host { id: "gone".into(), ..Default::default() }] };
        let (doc, _) = materialize(&store, &local, &Settings::default());
        assert_eq!(doc.hosts.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["c", "a", "b"]);
        assert_eq!(doc.hosts[0].name, "gamma");
    }
}
