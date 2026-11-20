//! Peer-to-peer sync on the local network, no server involved.
//!
//! - Discovery: every Termix in p2p mode announces `_termix._tcp.local.` over mDNS (instance = device id).
//! - Pairing: one device shows a 6-digit code for 2 minutes; the other enters it. Both run SPAKE2 with the
//!   code, which gives them a shared key only if the codes match (a wrong code learns nothing, and the code
//!   dies after 3 failures). The key is kept in the keychain as `sync:peer:<id>`.
//! - Sync: a fresh session key per connection (HKDF of the pair key and both nonces), then both sides send
//!   their records sealed with AES-256-GCM and merge (sync.rs rules).
use crate::config;
use crate::sync::{self, Peer, Records, Store, Vault};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::{Rng, RngCore};
use serde_json::{json, Value};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const SERVICE: &str = "_termix._tcp.local.";
const MAX_FRAME: usize = 64 << 20;
const PAIR_CONFIRM: &[u8] = b"termix-pair";

#[derive(Clone, serde::Serialize)]
pub struct Found {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub addrs: Vec<SocketAddr>,
}

struct Pairing {
    code: String,
    until: Instant,
    failures: u32,
}

struct Running {
    daemon: mdns_sd::ServiceDaemon,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
pub struct P2p {
    found: Mutex<HashMap<String, Found>>,
    pairing: Mutex<Option<Pairing>>,
    running: Mutex<Option<Running>>,
}

/// What a responder needs from the app to answer a sync: merge the peer's records, return ours.
pub type Exchange = std::sync::Arc<dyn Fn(Records) -> futures_lite::BoxedFuture<Records> + Send + Sync>;

mod futures_lite {
    pub type BoxedFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;
}

impl P2p {
    pub fn discovered(&self) -> Vec<Found> {
        let mut v: Vec<Found> = self.found.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn addrs(&self, id: &str) -> Vec<SocketAddr> {
        self.found.lock().unwrap().get(id).map(|f| f.addrs.clone()).unwrap_or_default()
    }

    /// Shows a pairing code on this device for 2 minutes.
    pub fn start_pairing(&self) -> String {
        let code = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));
        *self.pairing.lock().unwrap() = Some(Pairing { code: code.clone(), until: Instant::now() + Duration::from_secs(120), failures: 0 });
        code
    }

    pub fn stop(&self) {
        if let Some(r) = self.running.lock().unwrap().take() {
            r.tasks.iter().for_each(|t| t.abort());
            let _ = r.daemon.shutdown();
        }
        self.found.lock().unwrap().clear();
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }

    /// Announces this device and listens for pairing and sync connections.
    pub async fn start(self: &std::sync::Arc<Self>, device_id: String, device_name: String, exchange: Exchange) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }
        let listener = TcpListener::bind("0.0.0.0:0").await?;
        let port = listener.local_addr()?.port();
        let daemon = mdns_sd::ServiceDaemon::new()?;
        let host = format!("termix-{}.local.", &device_id[..8.min(device_id.len())]);
        let info = mdns_sd::ServiceInfo::new(SERVICE, &device_id, &host, "", port, [("name", device_name.as_str()), ("v", "1")].as_slice())?
            .enable_addr_auto();
        daemon.register(info)?;
        let events = daemon.browse(SERVICE)?;

        let me = self.clone();
        let own = device_id.clone();
        let browse = tokio::task::spawn_blocking(move || {
            while let Ok(ev) = events.recv() {
                match ev {
                    mdns_sd::ServiceEvent::ServiceResolved(s) => {
                        let id = s.fullname.strip_suffix(&format!(".{SERVICE}")).unwrap_or(&s.fullname).to_string();
                        if id == own {
                            continue;
                        }
                        let name = s.get_property_val_str("name").unwrap_or(&id).to_string();
                        let addrs = s.get_addresses_v4().into_iter().map(|ip| SocketAddr::new(ip.into(), s.port)).collect();
                        me.found.lock().unwrap().insert(id.clone(), Found { id, name, addrs });
                    }
                    mdns_sd::ServiceEvent::ServiceRemoved(_, full) => {
                        let id = full.strip_suffix(&format!(".{SERVICE}")).unwrap_or(&full).to_string();
                        me.found.lock().unwrap().remove(&id);
                    }
                    _ => {}
                }
            }
        });

        let me = self.clone();
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (me, id, name, ex) = (me.clone(), device_id.clone(), device_name.clone(), exchange.clone());
                tokio::spawn(async move {
                    if let Err(e) = tokio::time::timeout(Duration::from_secs(30), me.respond(stream, &id, &name, ex)).await.unwrap_or_else(|_| Err(anyhow!("délai dépassé"))) {
                        eprintln!("p2p : {e}");
                    }
                });
            }
        });
        *self.running.lock().unwrap() = Some(Running { daemon, tasks: vec![browse, accept] });
        Ok(())
    }

    async fn respond(&self, mut s: TcpStream, my_id: &str, my_name: &str, exchange: Exchange) -> Result<()> {
        let hello: Value = serde_json::from_slice(&read_frame(&mut s).await?)?;
        let peer_id = hello["id"].as_str().ok_or_else(|| anyhow!("hello invalide"))?.to_string();
        match hello["t"].as_str() {
            Some("pair") => {
                let code = {
                    let p = self.pairing.lock().unwrap();
                    match p.as_ref() {
                        Some(p) if p.until > Instant::now() => p.code.clone(),
                        _ => bail!("aucun appairage en cours"),
                    }
                };
                let (spake, msg) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(code.as_bytes()), &Identity::new(PAIR_CONFIRM));
                write_frame(&mut s, &serde_json::to_vec(&json!({ "msg": B64.encode(msg) }))?).await?;
                let key = spake.finish(&B64.decode(hello["msg"].as_str().unwrap_or_default())?).map_err(|_| anyhow!("échange SPAKE2 invalide"))?;
                let key: [u8; 32] = key.try_into().map_err(|_| anyhow!("clé SPAKE2 invalide"))?;
                let vault = Vault::new(&key);
                let confirm = read_frame(&mut s).await?;
                if vault.open(std::str::from_utf8(&confirm)?).as_deref() != Some(PAIR_CONFIRM) {
                    let mut p = self.pairing.lock().unwrap();
                    if let Some(p) = p.as_mut() {
                        p.failures += 1;
                        if p.failures >= 3 {
                            p.until = Instant::now();
                        }
                    }
                    bail!("code d'appairage incorrect");
                }
                let mut store = Store::load();
                if store.salt.is_empty() {
                    store.salt = sync::random_salt();
                }
                let reply = json!({ "name": my_name, "salt": store.salt });
                write_frame(&mut s, vault.seal(&serde_json::to_vec(&reply)?).as_bytes()).await?;
                remember(&mut store, &peer_id, hello["name"].as_str().unwrap_or(&peer_id), &key)?;
                *self.pairing.lock().unwrap() = None;
                Ok(())
            }
            Some("sync") => {
                let key = peer_key(&peer_id).ok_or_else(|| anyhow!("appareil non appairé : {peer_id}"))?;
                let their_nonce = B64.decode(hello["nonce"].as_str().unwrap_or_default())?;
                let mut my_nonce = [0u8; 16];
                rand::rngs::OsRng.fill_bytes(&mut my_nonce);
                write_frame(&mut s, &serde_json::to_vec(&json!({ "id": my_id, "nonce": B64.encode(my_nonce) }))?).await?;
                let session = session_key(&key, &their_nonce, &my_nonce);
                let sealed = read_frame(&mut s).await?;
                let theirs: Records = serde_json::from_slice(&session.open(std::str::from_utf8(&sealed)?).ok_or_else(|| anyhow!("message illisible"))?)?;
                let ours = exchange(theirs).await;
                write_frame(&mut s, session.seal(&serde_json::to_vec(&ours)?).as_bytes()).await?;
                Ok(())
            }
            _ => bail!("message inconnu"),
        }
    }
}

/// Pairs with the discovered device `peer_id` using the code it shows. Returns its name.
pub async fn pair(addrs: &[SocketAddr], peer_id: &str, my_id: &str, my_name: &str, code: &str) -> Result<String> {
    let mut s = connect(addrs).await?;
    let (spake, msg) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(code.trim().as_bytes()), &Identity::new(PAIR_CONFIRM));
    write_frame(&mut s, &serde_json::to_vec(&json!({ "t": "pair", "id": my_id, "name": my_name, "msg": B64.encode(msg) }))?).await?;
    let reply: Value = serde_json::from_slice(&read_frame(&mut s).await.context("l'appareil n'attend pas d'appairage")?)?;
    let key = spake.finish(&B64.decode(reply["msg"].as_str().unwrap_or_default())?).map_err(|_| anyhow!("échange SPAKE2 invalide"))?;
    let key: [u8; 32] = key.try_into().map_err(|_| anyhow!("clé SPAKE2 invalide"))?;
    let vault = Vault::new(&key);
    write_frame(&mut s, vault.seal(PAIR_CONFIRM).as_bytes()).await?;
    let sealed = read_frame(&mut s).await.map_err(|_| anyhow!("code d'appairage incorrect"))?;
    let info: Value = serde_json::from_slice(&vault.open(std::str::from_utf8(&sealed)?).ok_or_else(|| anyhow!("code d'appairage incorrect"))?)?;
    let name = info["name"].as_str().unwrap_or("?").to_string();
    let mut store = Store::load();
    let salt = info["salt"].as_str().unwrap_or_default();
    if !salt.is_empty() && store.salt != salt {
        // Joining the other device's vault: secrets are re-encrypted after the next unlock.
        store.salt = salt.into();
        let _ = config::secret_set("sync:vault", "");
        // Our old vault's ciphertexts can't be read with the shared one: forget them, the peer's win.
        let stale = |k: &String| k == "vault:check" || k.starts_with("secret:") || k.starts_with("key:");
        store.records.retain(|k, _| !stale(k));
        store.dirty.retain(|k| !stale(k));
        store.local_secrets.clear();
    }
    remember(&mut store, peer_id, &name, &key)?;
    Ok(name)
}

/// Stores a paired device: its key in the keychain, its name in sync.json.
pub fn remember(store: &mut Store, id: &str, name: &str, key: &[u8; 32]) -> Result<()> {
    config::secret_set(&format!("sync:peer:{id}"), &B64.encode(key))?;
    store.peers.retain(|p| p.id != id);
    store.peers.push(Peer { id: id.into(), name: name.into() });
    store.save()
}

fn peer_key(id: &str) -> Option<[u8; 32]> {
    B64.decode(config::secret_get(&format!("sync:peer:{id}"))?).ok()?.try_into().ok()
}

pub fn forget(store: &mut Store, id: &str) -> Result<()> {
    let _ = config::secret_set(&format!("sync:peer:{id}"), "");
    store.peers.retain(|p| p.id != id);
    store.save()
}

fn session_key(pair: &[u8; 32], initiator_nonce: &[u8], responder_nonce: &[u8]) -> Vault {
    let salt = [initiator_nonce, responder_nonce].concat();
    let mut out = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(Some(&salt), pair).expand(b"termix-sync", &mut out).expect("32 bytes is a valid HKDF length");
    Vault::new(&out)
}

/// Sends our records to a paired device and merges its answer into `store`.
pub async fn sync_with(addrs: &[SocketAddr], peer_id: &str, my_id: &str, store: &mut Store) -> Result<()> {
    let key = peer_key(peer_id).ok_or_else(|| anyhow!("clé d'appairage absente"))?;
    let mut s = connect(addrs).await?;
    let mut nonce = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    write_frame(&mut s, &serde_json::to_vec(&json!({ "t": "sync", "id": my_id, "nonce": B64.encode(nonce) }))?).await?;
    let hello: Value = serde_json::from_slice(&read_frame(&mut s).await?)?;
    if hello["id"].as_str() != Some(peer_id) {
        bail!("appareil inattendu");
    }
    let session = session_key(&key, &nonce, &B64.decode(hello["nonce"].as_str().unwrap_or_default())?);
    write_frame(&mut s, session.seal(&serde_json::to_vec(&store.records)?).as_bytes()).await?;
    let sealed = read_frame(&mut s).await?;
    let theirs: Records = serde_json::from_slice(&session.open(std::str::from_utf8(&sealed)?).ok_or_else(|| anyhow!("réponse illisible"))?)?;
    sync::merge(&mut store.records, &theirs);
    Ok(())
}

async fn connect(addrs: &[SocketAddr]) -> Result<TcpStream> {
    for a in addrs {
        if let Ok(Ok(s)) = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(a)).await {
            return Ok(s);
        }
    }
    bail!("appareil injoignable sur le réseau local")
}

async fn write_frame(s: &mut TcpStream, data: &[u8]) -> Result<()> {
    s.write_u32(data.len() as u32).await?;
    s.write_all(data).await?;
    Ok(())
}

async fn read_frame(s: &mut TcpStream) -> Result<Vec<u8>> {
    let n = s.read_u32().await? as usize;
    if n > MAX_FRAME {
        bail!("message trop gros");
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).await?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spake2_agrees_only_on_the_same_code() {
        let id = Identity::new(PAIR_CONFIRM);
        let (a, ma) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(b"123456"), &id);
        let (b, mb) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(b"123456"), &id);
        assert_eq!(a.finish(&mb).unwrap(), b.finish(&ma).unwrap());
        let (a, ma) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(b"123456"), &id);
        let (b, mb) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(b"654321"), &id);
        assert_ne!(a.finish(&mb).unwrap(), b.finish(&ma).unwrap());
    }

    #[test]
    fn session_keys_differ_per_connection() {
        let k = [1u8; 32];
        let s1 = session_key(&k, b"aaaa", b"bbbb");
        let s2 = session_key(&k, b"aaaa", b"cccc");
        let sealed = s1.seal(b"x");
        assert!(s2.open(&sealed).is_none());
        assert_eq!(session_key(&k, b"aaaa", b"bbbb").open(&sealed).unwrap(), b"x");
    }
}
