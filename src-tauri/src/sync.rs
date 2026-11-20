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
    json!({ "appearance": s.appearance, "terminal": s.terminal, "keybindings": s.keybindings, "themes": s.themes })
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
    dev: &str,
) {
    let mut want: BTreeMap<String, Value> = BTreeMap::new();
    for h in &doc.hosts {
        want.insert(format!("host:{}", h.id), json!(h));
    }
    for f in &doc.folders {
        want.insert(format!("folder:{}", f.id), json!(f));
    }
    want.insert("settings".into(), synced_settings(settings));
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
        if let Ok(k) = serde_json::from_value(v["keybindings"].clone()) {
            s.keybindings = k;
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
    }
    Ok((applied, new_settings))
}

// ---------- sync server client ----------

fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().unwrap_or_default()
}

fn base(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

async fn checked(r: reqwest::Response) -> Result<reqwest::Response> {
    let status = r.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        bail!("session expirée ou identifiants refusés : reconnectez-vous");
    }
    if !status.is_success() {
        let msg = r.text().await.unwrap_or_default();
        bail!("serveur de synchro : {status} {msg}");
    }
    Ok(r)
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

/// Pushes local changes, then pulls everything newer than the last pull.
pub async fn server_sync(url: &str, token: &str, store: &mut Store) -> Result<BTreeSet<String>> {
    let c = http();
    if !store.dirty.is_empty() {
        let recs: Vec<Value> = store
            .dirty
            .iter()
            .filter_map(|k| store.records.get(k).map(|r| json!({ "key": k, "v": r.v, "ts": r.ts, "dev": r.dev })))
            .collect();
        checked(c.post(format!("{}/api/records", base(url))).bearer_auth(token).json(&json!({ "records": recs })).send().await?).await?;
        store.dirty.clear();
    }
    let v: Value =
        checked(c.get(format!("{}/api/records?after={}", base(url), store.server_seq)).bearer_auth(token).send().await?).await?.json().await?;
    let incoming = parse_records(&v["records"]);
    store.server_seq = v["seq"].as_i64().unwrap_or(store.server_seq);
    Ok(merge(&mut store.records, &incoming))
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
    fn local_changes_become_records_and_deletions_tombstones() {
        let mut store = Store::default();
        let mut doc = HostsDoc::default();
        doc.hosts.push(Host { id: "h1".into(), name: "web".into(), ..Default::default() });
        let s = Settings::default();
        record_local(&mut store, &doc, &s, None, &|_| None, "a");
        assert!(store.records["host:h1"].v.is_some());
        let ts = store.records["host:h1"].ts;
        // Unchanged: no new timestamp.
        record_local(&mut store, &doc, &s, None, &|_| None, "a");
        assert_eq!(store.records["host:h1"].ts, ts);
        doc.hosts.clear();
        record_local(&mut store, &doc, &s, None, &|_| None, "a");
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
        record_local(&mut store, &doc, &s, Some(&vault), &|_| None, "a");
        assert!(store.records["secret:h1"].v.is_some());
        // A device that had it and lost it (user cleared the password) propagates the deletion.
        record_local(&mut store, &doc, &s, Some(&vault), &|id| (id == "h1").then(|| "pw".to_string()), "a");
        assert!(store.local_secrets.contains("secret:h1"));
        let sealed = store.records["secret:h1"].v.clone().unwrap();
        assert!(!sealed.to_string().contains("pw\""), "secret must be encrypted");
        record_local(&mut store, &doc, &s, Some(&vault), &|_| None, "a");
        assert_eq!(store.records["secret:h1"].v, None);
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
