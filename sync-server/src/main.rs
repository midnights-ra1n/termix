//! termix-sync: self-hosted sync server for Termix.
//!
//! Stores per-user records (hosts, folders, settings, workspaces, plus end-to-end encrypted secrets and
//! SSH keys it cannot read) and serves a small web UI to manage users, hosts and keys.
//! Records follow the same last-writer-wins rule as the app: newest (ts, dev) wins per key.
//!
//! Environment:
//!   TERMIX_LISTEN          address to listen on (default 0.0.0.0:8080)
//!   TERMIX_DB              SQLite database path (default /data/termix-sync.db)
//!   TERMIX_ADMIN_USER      first admin, created when the database has no user (default "admin")
//!   TERMIX_ADMIN_PASSWORD  its password; when unset a random one is printed once in the logs
use argon2::password_hash::rand_core::OsRng as PwRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use base64::Engine;
use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

type Db = Arc<Mutex<Connection>>;

const INDEX: &str = include_str!("web/index.html");
const APP_JS: &str = include_str!("web/app.js");
const STYLE: &str = include_str!("web/style.css");
const LOGO: &str = include_str!("../../assets/icon-rounded.svg");
/// IBM Plex (OFL, see web/fonts/OFL.txt), served locally: the web UI never calls a font CDN.
const FONTS: &[(&str, &[u8])] = &[
    ("plex-sans-400.woff2", include_bytes!("web/fonts/ibm-plex-sans-latin-400-normal.woff2")),
    ("plex-sans-500.woff2", include_bytes!("web/fonts/ibm-plex-sans-latin-500-normal.woff2")),
    ("plex-sans-600.woff2", include_bytes!("web/fonts/ibm-plex-sans-latin-600-normal.woff2")),
    ("plex-mono-400.woff2", include_bytes!("web/fonts/ibm-plex-mono-latin-400-normal.woff2")),
];

async fn font(Path(name): Path<String>) -> Response {
    match FONTS.iter().find(|(n, _)| *n == name) {
        Some((_, bytes)) => ([(header::CONTENT_TYPE, "font/woff2"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], *bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---------- errors ----------

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        eprintln!("database: {e}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
    }
}

fn bad(msg: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

type Res<T> = Result<T, ApiError>;

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn random_b64(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::rngs::OsRng.fill_bytes(&mut b);
    B64URL.encode(b)
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn hash_password(pw: &str) -> String {
    Argon2::default().hash_password(pw.as_bytes(), &SaltString::generate(&mut PwRng)).expect("argon2").to_string()
}

fn verify_password(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

fn valid_key(k: &str) -> bool {
    k.len() <= 200
        && (k == "settings"
            || ["host:", "folder:", "secret:", "key:", "workspace:", "vault:"].iter().any(|p| k.strip_prefix(p).is_some_and(|rest| !rest.is_empty())))
}

// ---------- database ----------

fn open(path: &str) -> anyhow::Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let c = Connection::open(path)?;
    c.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS users(
           id INTEGER PRIMARY KEY, name TEXT UNIQUE NOT NULL, pw TEXT NOT NULL,
           admin INTEGER NOT NULL DEFAULT 0, salt TEXT NOT NULL, created INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS tokens(
           hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           device TEXT NOT NULL, created INTEGER NOT NULL, last_used INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS records(
           user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE, key TEXT NOT NULL,
           v TEXT, ts INTEGER NOT NULL, dev TEXT NOT NULL, seq INTEGER NOT NULL,
           PRIMARY KEY(user_id, key));
         CREATE INDEX IF NOT EXISTS records_seq ON records(user_id, seq);
         CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v INTEGER NOT NULL);",
    )?;
    Ok(c)
}

fn create_user(c: &Connection, name: &str, pw: &str, admin: bool) -> rusqlite::Result<i64> {
    // The vault salt is per account: every device and the web UI derive the same key from the passphrase.
    let salt = {
        let mut s = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut s);
        B64.encode(s)
    };
    c.execute("INSERT INTO users(name, pw, admin, salt, created) VALUES (?1, ?2, ?3, ?4, ?5)", params![name, hash_password(pw), admin, salt, now()])?;
    Ok(c.last_insert_rowid())
}

fn bootstrap(c: &Connection) -> anyhow::Result<()> {
    let n: i64 = c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    if n > 0 {
        return Ok(());
    }
    let name = std::env::var("TERMIX_ADMIN_USER").ok().filter(|s| valid_name(s)).unwrap_or_else(|| "admin".into());
    let (pw, generated) = match std::env::var("TERMIX_ADMIN_PASSWORD").ok().filter(|p| p.len() >= 8) {
        Some(p) => (p, false),
        None => (random_b64(15), true),
    };
    create_user(c, &name, &pw, true)?;
    if generated {
        println!("termix-sync: created admin user \"{name}\" with password: {pw}");
        println!("termix-sync: change it in the web UI (Account) after signing in.");
    } else {
        println!("termix-sync: created admin user \"{name}\"");
    }
    Ok(())
}

// ---------- auth ----------

struct User {
    id: i64,
    name: String,
    admin: bool,
    salt: String,
}

fn bearer(h: &HeaderMap) -> Option<String> {
    h.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(|s| s.trim().to_string())
}

fn auth(db: &Db, h: &HeaderMap) -> Res<User> {
    let unauthorized = || ApiError(StatusCode::UNAUTHORIZED, "sign in required".into());
    let token = bearer(h).ok_or_else(unauthorized)?;
    let hash = sha256_hex(&token);
    let c = db.lock().unwrap();
    let u = c
        .query_row(
            "SELECT u.id, u.name, u.admin, u.salt FROM tokens t JOIN users u ON u.id = t.user_id WHERE t.hash = ?1",
            [&hash],
            |r| Ok(User { id: r.get(0)?, name: r.get(1)?, admin: r.get(2)?, salt: r.get(3)? }),
        )
        .optional()?
        .ok_or_else(unauthorized)?;
    c.execute("UPDATE tokens SET last_used = ?1 WHERE hash = ?2", params![now(), hash])?;
    Ok(u)
}

fn admin(db: &Db, h: &HeaderMap) -> Res<User> {
    let u = auth(db, h)?;
    if !u.admin {
        return Err(ApiError(StatusCode::FORBIDDEN, "administrators only".into()));
    }
    Ok(u)
}

#[derive(Deserialize)]
struct Login {
    user: String,
    password: String,
    #[serde(default)]
    device: String,
}

async fn login(State(db): State<Db>, Json(l): Json<Login>) -> Res<Json<Value>> {
    let row = {
        let c = db.lock().unwrap();
        c.query_row("SELECT id, pw, admin, salt FROM users WHERE name = ?1", [&l.user], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?))
        })
        .optional()?
    };
    let ok = row.as_ref().is_some_and(|(_, hash, _, _)| verify_password(&l.password, hash));
    let Some((id, _, is_admin, salt)) = row.filter(|_| ok) else {
        // Slow down password guessing.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "wrong user or password".into()));
    };
    let token = random_b64(32);
    let device: String = l.device.chars().take(80).collect();
    let device = if device.trim().is_empty() { "unknown".to_string() } else { device };
    db.lock().unwrap().execute(
        "INSERT INTO tokens(hash, user_id, device, created, last_used) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![sha256_hex(&token), id, device, now()],
    )?;
    Ok(Json(json!({ "token": token, "user": l.user, "admin": is_admin, "salt": salt })))
}

async fn logout(State(db): State<Db>, h: HeaderMap) -> Res<StatusCode> {
    auth(&db, &h)?;
    let token = bearer(&h).unwrap_or_default();
    db.lock().unwrap().execute("DELETE FROM tokens WHERE hash = ?1", [sha256_hex(&token)])?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(State(db): State<Db>, h: HeaderMap) -> Res<Json<Value>> {
    let u = auth(&db, &h)?;
    let current = sha256_hex(&bearer(&h).unwrap_or_default());
    let c = db.lock().unwrap();
    let mut st = c.prepare("SELECT hash, device, created, last_used FROM tokens WHERE user_id = ?1 ORDER BY last_used DESC")?;
    let devices: Vec<Value> = st
        .query_map([u.id], |r| {
            let hash: String = r.get(0)?;
            Ok(json!({ "id": &hash[..16], "device": r.get::<_, String>(1)?, "created": r.get::<_, i64>(2)?, "last_used": r.get::<_, i64>(3)?, "current": hash == current }))
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(json!({ "user": u.name, "admin": u.admin, "salt": u.salt, "devices": devices })))
}

#[derive(Deserialize)]
struct PwChange {
    old: String,
    new: String,
}

async fn change_password(State(db): State<Db>, h: HeaderMap, Json(p): Json<PwChange>) -> Res<StatusCode> {
    let u = auth(&db, &h)?;
    if p.new.len() < 8 {
        return Err(bad("the password must be at least 8 characters"));
    }
    let c = db.lock().unwrap();
    let hash: String = c.query_row("SELECT pw FROM users WHERE id = ?1", [u.id], |r| r.get(0))?;
    if !verify_password(&p.old, &hash) {
        return Err(ApiError(StatusCode::FORBIDDEN, "current password is wrong".into()));
    }
    c.execute("UPDATE users SET pw = ?1 WHERE id = ?2", params![hash_password(&p.new), u.id])?;
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_device(State(db): State<Db>, h: HeaderMap, Path(id): Path<String>) -> Res<StatusCode> {
    let u = auth(&db, &h)?;
    if id.len() != 16 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad("invalid device id"));
    }
    db.lock().unwrap().execute("DELETE FROM tokens WHERE user_id = ?1 AND substr(hash, 1, 16) = ?2", params![u.id, id])?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- records ----------

#[derive(Deserialize)]
struct After {
    #[serde(default)]
    after: i64,
}

async fn pull(State(db): State<Db>, h: HeaderMap, Query(q): Query<After>) -> Res<Json<Value>> {
    let u = auth(&db, &h)?;
    let c = db.lock().unwrap();
    let mut st = c.prepare("SELECT key, v, ts, dev, seq FROM records WHERE user_id = ?1 AND seq > ?2 ORDER BY seq")?;
    let mut seq = q.after;
    let records: Vec<Value> = st
        .query_map(params![u.id, q.after], |r| {
            let v: Option<String> = r.get(1)?;
            Ok((r.get::<_, String>(0)?, v, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?))
        })?
        .filter_map(Result::ok)
        .map(|(key, v, ts, dev, s)| {
            seq = seq.max(s);
            let v = v.and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
            json!({ "key": key, "v": v, "ts": ts, "dev": dev })
        })
        .collect();
    Ok(Json(json!({ "records": records, "seq": seq })))
}

#[derive(Deserialize)]
struct Push {
    records: Vec<Incoming>,
}

#[derive(Deserialize)]
struct Incoming {
    key: String,
    #[serde(default)]
    v: Value,
    ts: i64,
    #[serde(default)]
    dev: String,
}

/// Last-writer-wins upsert; returns how many records were taken.
fn upsert(c: &mut Connection, user: i64, recs: &[Incoming]) -> rusqlite::Result<usize> {
    let tx = c.transaction()?;
    let mut taken = 0;
    for r in recs {
        let cur: Option<(i64, String)> =
            tx.query_row("SELECT ts, dev FROM records WHERE user_id = ?1 AND key = ?2", params![user, r.key], |x| Ok((x.get(0)?, x.get(1)?))).optional()?;
        if cur.is_some_and(|(ts, dev)| (r.ts, r.dev.as_str()) <= (ts, dev.as_str())) {
            continue;
        }
        tx.execute("INSERT INTO meta(k, v) VALUES ('seq', 1) ON CONFLICT(k) DO UPDATE SET v = v + 1", [])?;
        let seq: i64 = tx.query_row("SELECT v FROM meta WHERE k = 'seq'", [], |x| x.get(0))?;
        let v = (!r.v.is_null()).then(|| r.v.to_string());
        tx.execute(
            "INSERT INTO records(user_id, key, v, ts, dev, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(user_id, key) DO UPDATE SET v = excluded.v, ts = excluded.ts, dev = excluded.dev, seq = excluded.seq",
            params![user, r.key, v, r.ts, r.dev, seq],
        )?;
        taken += 1;
    }
    tx.commit()?;
    Ok(taken)
}

async fn push(State(db): State<Db>, h: HeaderMap, Json(p): Json<Push>) -> Res<Json<Value>> {
    let u = auth(&db, &h)?;
    if p.records.len() > 10_000 {
        return Err(bad("too many records in one request"));
    }
    if let Some(r) = p.records.iter().find(|r| !valid_key(&r.key) || r.dev.len() > 80) {
        return Err(bad(&format!("invalid record key: {}", r.key.chars().take(60).collect::<String>())));
    }
    let taken = upsert(&mut db.lock().unwrap(), u.id, &p.records)?;
    Ok(Json(json!({ "taken": taken })))
}

// ---------- users (admin) ----------

async fn list_users(State(db): State<Db>, h: HeaderMap) -> Res<Json<Value>> {
    admin(&db, &h)?;
    let c = db.lock().unwrap();
    let mut st = c.prepare(
        "SELECT u.id, u.name, u.admin, u.created,
                (SELECT COUNT(*) FROM records r WHERE r.user_id = u.id AND r.key LIKE 'host:%' AND r.v IS NOT NULL),
                (SELECT COUNT(*) FROM tokens t WHERE t.user_id = u.id)
         FROM users u ORDER BY u.name",
    )?;
    let users: Vec<Value> = st
        .query_map([], |r| {
            Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "admin": r.get::<_, bool>(2)?, "created": r.get::<_, i64>(3)?, "hosts": r.get::<_, i64>(4)?, "devices": r.get::<_, i64>(5)? }))
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(json!(users)))
}

#[derive(Deserialize)]
struct NewUser {
    name: String,
    password: String,
    #[serde(default)]
    admin: bool,
}

async fn add_user(State(db): State<Db>, h: HeaderMap, Json(n): Json<NewUser>) -> Res<Json<Value>> {
    admin(&db, &h)?;
    if !valid_name(&n.name) {
        return Err(bad("user names use letters, digits, dot, dash and underscore (64 max)"));
    }
    if n.password.len() < 8 {
        return Err(bad("the password must be at least 8 characters"));
    }
    let c = db.lock().unwrap();
    let id = create_user(&c, &n.name, &n.password, n.admin).map_err(|e| match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => bad("this user already exists"),
        e => e.into(),
    })?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
struct UserPatch {
    password: Option<String>,
    admin: Option<bool>,
}

fn admins_left(c: &Connection, except: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM users WHERE admin = 1 AND id != ?1", [except], |r| r.get(0))
}

async fn edit_user(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, Json(p): Json<UserPatch>) -> Res<StatusCode> {
    admin(&db, &h)?;
    let c = db.lock().unwrap();
    if let Some(pw) = p.password {
        if pw.len() < 8 {
            return Err(bad("the password must be at least 8 characters"));
        }
        c.execute("UPDATE users SET pw = ?1 WHERE id = ?2", params![hash_password(&pw), id])?;
        // A reset password signs every device out.
        c.execute("DELETE FROM tokens WHERE user_id = ?1", [id])?;
    }
    if let Some(a) = p.admin {
        if !a && admins_left(&c, id)? == 0 {
            return Err(bad("there must be at least one administrator"));
        }
        c.execute("UPDATE users SET admin = ?1 WHERE id = ?2", params![a, id])?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_user(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> Res<StatusCode> {
    let me = admin(&db, &h)?;
    if me.id == id {
        return Err(bad("you cannot delete your own account"));
    }
    let c = db.lock().unwrap();
    if admins_left(&c, id)? == 0 {
        return Err(bad("there must be at least one administrator"));
    }
    c.execute("DELETE FROM users WHERE id = ?1", [id])?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- web UI ----------

fn asset(ct: &'static str, body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

async fn security_headers(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut r = next.run(req).await;
    let h = r.headers_mut();
    // Everything is served from here: no CDN, no fonts, no analytics.
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    r
}

fn app(db: Db) -> Router {
    Router::new()
        .route("/", get(|| async { asset("text/html; charset=utf-8", INDEX) }))
        .route("/app.js", get(|| async { asset("text/javascript; charset=utf-8", APP_JS) }))
        .route("/style.css", get(|| async { asset("text/css; charset=utf-8", STYLE) }))
        .route("/logo.svg", get(|| async { asset("image/svg+xml", LOGO) }))
        .route("/fonts/{name}", get(font))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/password", post(change_password))
        .route("/api/devices/{id}", delete(revoke_device))
        .route("/api/records", get(pull).post(push))
        .route("/api/users", get(list_users).post(add_user))
        .route("/api/users/{id}", patch(edit_user).delete(delete_user))
        .layer(DefaultBodyLimit::max(32 << 20))
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(db)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::var("TERMIX_DB").unwrap_or_else(|_| "/data/termix-sync.db".into());
    let listen = std::env::var("TERMIX_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let conn = open(&path)?;
    bootstrap(&conn)?;
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    println!("termix-sync listening on http://{listen} (database {path})");
    axum::serve(listener, app(Arc::new(Mutex::new(conn)))).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(key: &str, v: Value, ts: i64) -> Incoming {
        Incoming { key: key.into(), v, ts, dev: "d".into() }
    }

    #[test]
    fn newest_record_wins_and_seq_moves_forward() {
        let mut c = open(":memory:").unwrap();
        let u = create_user(&c, "alice", "password1", false).unwrap();
        assert_eq!(upsert(&mut c, u, &[rec("host:1", json!({ "name": "a" }), 10)]).unwrap(), 1);
        assert_eq!(upsert(&mut c, u, &[rec("host:1", json!({ "name": "old" }), 5)]).unwrap(), 0);
        assert_eq!(upsert(&mut c, u, &[rec("host:1", Value::Null, 20)]).unwrap(), 1);
        let (v, seq): (Option<String>, i64) = c.query_row("SELECT v, seq FROM records WHERE key = 'host:1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(v, None);
        assert_eq!(seq, 2);
    }

    #[test]
    fn users_are_isolated() {
        let mut c = open(":memory:").unwrap();
        let a = create_user(&c, "alice", "password1", false).unwrap();
        let b = create_user(&c, "bob", "password2", false).unwrap();
        upsert(&mut c, a, &[rec("host:1", json!(1), 1)]).unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM records WHERE user_id = ?1", [b], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn keys_and_names_are_validated() {
        assert!(valid_key("host:abc") && valid_key("settings") && valid_key("vault:check"));
        assert!(!valid_key("host:") && !valid_key("evil") && !valid_key(&format!("host:{}", "x".repeat(300))));
        assert!(valid_name("noé.s-1") == false && valid_name("noe.s-1"));
    }

    #[test]
    fn passwords_hash_and_verify() {
        let h = hash_password("correct horse");
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
    }
}
