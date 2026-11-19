//! SSH sessions: auth (agent → key file → password), PTY shell, SFTP and server stats.
use crate::config::{secret_get, Host};
use anyhow::{anyhow, bail, Context, Result};
use russh::client::{self, AuthResult, Handle};
use russh::keys::{agent::client::AgentClient, agent::AgentIdentity, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex, OnceCell};

pub struct Client {
    host: String,
    port: u16,
}

impl client::Handler for Client {
    type Error = anyhow::Error;
    /// Same policy as OpenSSH `StrictHostKeyChecking=accept-new`: learn unknown hosts, refuse changed keys.
    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            bail!("certificats d'hôte non pris en charge");
        };
        match russh::keys::check_known_hosts(&self.host, self.port, key) {
            Ok(true) => Ok(true),
            Ok(false) => {
                let _ = russh::keys::known_hosts::learn_known_hosts(&self.host, self.port, key);
                Ok(true)
            }
            Err(e) => bail!("clé d'hôte modifiée pour {} ({e}) — vérifiez ~/.ssh/known_hosts", self.host),
        }
    }
}

pub enum Cmd {
    Data(Vec<u8>),
    Resize(u32, u32),
    Close,
}

pub struct Session {
    pub tx: mpsc::UnboundedSender<Cmd>,
    pub handle: Arc<Handle<Client>>,
    sftp: OnceCell<SftpSession>,
}

pub type Sessions = Mutex<HashMap<String, Arc<Session>>>;

// ---------- agent discovery ----------

fn expand(p: &str) -> PathBuf {
    let p = p.trim().trim_matches('"');
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(p),
    }
}

/// Global `IdentityAgent` from ~/.ssh/config (where Bitwarden/1Password/Secretive usually get wired).
fn ssh_config_agent() -> Option<String> {
    let text = std::fs::read_to_string(dirs::home_dir()?.join(".ssh/config")).ok()?;
    for line in text.lines() {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();
        if lower.starts_with("host ") || lower.starts_with("match ") {
            break; // shortcut: only the global section is read, per-host IdentityAgent is ignored
        }
        if lower.starts_with("identityagent") {
            let v = l[13..].trim_start_matches([' ', '=', '\t']).trim();
            if !v.is_empty() && v != "none" && v != "SSH_AUTH_SOCK" {
                return Some(expand(v).to_string_lossy().into());
            }
        }
    }
    None
}

/// GUI apps (macOS especially) don't inherit the env exported by .zshrc/.bashrc/config.fish, so ask the login shell.
fn shell_agent() -> Option<String> {
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let out = std::process::Command::new(shell)
            .args(["-l", "-i", "-c", "echo __RT__$SSH_AUTH_SOCK"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let v = s.lines().find_map(|l| l.strip_prefix("__RT__"))?.trim().to_string();
        (!v.is_empty()).then_some(v)
    }
    #[cfg(not(unix))]
    None
}

/// Well-known sockets of password-manager agents, shown in settings as choices.
pub fn known_agents() -> Vec<(String, String)> {
    let h = dirs::home_dir().unwrap_or_default();
    let mut v: Vec<(String, PathBuf)> = vec![
        ("1Password".into(), h.join("Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock")),
        ("1Password".into(), h.join(".1password/agent.sock")),
        ("Bitwarden".into(), h.join(".bitwarden-ssh-agent.sock")),
        ("Bitwarden".into(), h.join("Library/Containers/com.bitwarden.desktop/Data/.bitwarden-ssh-agent.sock")),
        ("Bitwarden (Flatpak)".into(), h.join(".var/app/com.bitwarden.desktop/data/.bitwarden-ssh-agent.sock")),
        ("Proton Pass".into(), h.join(".ssh/proton-pass-agent.sock")),
        ("Secretive".into(), h.join("Library/Containers/com.maxgoedjen.Secretive.SecretAgent/Data/socket.ssh")),
    ];
    if let Ok(x) = std::env::var("XDG_RUNTIME_DIR") {
        v.push(("GNOME Keyring / KeePassXC".into(), PathBuf::from(&x).join("keyring/ssh")));
        v.push(("ssh-agent (systemd)".into(), PathBuf::from(x).join("ssh-agent.socket")));
    }
    let mut out: Vec<(String, String)> =
        v.into_iter().filter(|(_, p)| p.exists()).map(|(n, p)| (n, p.to_string_lossy().into())).collect();
    if let Some(s) = ssh_config_agent() {
        out.insert(0, ("~/.ssh/config (IdentityAgent)".into(), s));
    }
    if let Some(s) = shell_env_agent() {
        out.insert(0, ("Shell ($SSH_AUTH_SOCK)".into(), s));
    }
    if cfg!(windows) {
        out.push(("OpenSSH (Windows, KeePassXC, Bitwarden, 1Password)".into(), r"\\.\pipe\openssh-ssh-agent".into()));
    }
    out
}

fn shell_env_agent() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE.get_or_init(|| std::env::var("SSH_AUTH_SOCK").ok().filter(|s| !s.is_empty()).or_else(shell_agent)).clone()
}

pub fn resolve_agent(configured: &str) -> Option<String> {
    if !configured.trim().is_empty() {
        return Some(expand(configured).to_string_lossy().into());
    }
    if cfg!(windows) {
        return Some(r"\\.\pipe\openssh-ssh-agent".into());
    }
    ssh_config_agent().or_else(shell_env_agent)
}

async fn agent_try<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    h: &mut Handle<Client>,
    user: &str,
    mut agent: AgentClient<S>,
) -> Result<bool> {
    let hash = h.best_supported_rsa_hash().await?.flatten();
    for id in agent.request_identities().await? {
        let AgentIdentity::PublicKey { key, .. } = id else { continue };
        if let Ok(AuthResult::Success) = h.authenticate_publickey_with(user, key, hash, &mut agent).await {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn auth_agent(h: &mut Handle<Client>, user: &str, sock: &str) -> Result<bool> {
    #[cfg(unix)]
    {
        agent_try(h, user, AgentClient::connect_uds(sock).await?).await
    }
    #[cfg(windows)]
    {
        agent_try(h, user, AgentClient::connect_named_pipe(sock).await?).await
    }
}

// ---------- connect ----------

pub async fn connect(host: &Host, agent_sock: Option<String>, password: Option<String>, keepalive: u64) -> Result<Handle<Client>> {
    let cfg = Arc::new(client::Config {
        keepalive_interval: (keepalive > 0).then(|| Duration::from_secs(keepalive)),
        inactivity_timeout: None,
        ..Default::default()
    });
    let port = if host.port == 0 { 22 } else { host.port };
    let user = if host.user.is_empty() { whoami() } else { host.user.clone() };
    let addr = (host.host.as_str(), port);
    let mut h = tokio::time::timeout(Duration::from_secs(15), client::connect(cfg, addr, Client { host: host.host.clone(), port }))
        .await
        .map_err(|_| anyhow!("délai de connexion dépassé"))??;

    if let Some(sock) = agent_sock {
        match auth_agent(&mut h, &user, &sock).await {
            Ok(true) => return Ok(h),
            Ok(false) => {}
            Err(e) => eprintln!("agent SSH indisponible ({sock}): {e}"),
        }
    }
    if !host.key_path.is_empty() {
        let pass = secret_get(&format!("{}:passphrase", host.id));
        let key = russh::keys::load_secret_key(expand(&host.key_path), pass.as_deref())
            .with_context(|| format!("lecture de la clé {}", host.key_path))?;
        let hash = h.best_supported_rsa_hash().await?.flatten();
        if h.authenticate_publickey(&user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await?.success() {
            return Ok(h);
        }
    }
    if let Some(pw) = password.or_else(|| secret_get(&host.id)) {
        if h.authenticate_password(&user, pw).await?.success() {
            return Ok(h);
        }
        bail!("authentification refusée");
    }
    bail!("NEED_PASSWORD")
}

fn whoami() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "root".into())
}

/// Opens the PTY shell and pumps output to `on_data`. Returns the session handle.
pub async fn open_shell(
    handle: Handle<Client>,
    cols: u32,
    rows: u32,
    on_data: impl Fn(Vec<u8>) + Send + 'static,
    on_close: impl FnOnce() + Send + 'static,
) -> Result<Arc<Session>> {
    let mut ch = handle.channel_open_session().await?;
    ch.request_pty(false, "xterm-256color", cols, rows, 0, 0, &[]).await?;
    ch.request_shell(false).await?;
    let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
    let handle = Arc::new(handle);
    let h2 = handle.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = ch.wait() => match msg {
                    Some(ChannelMsg::Data { data }) => on_data(data.to_vec()),
                    Some(ChannelMsg::ExtendedData { data, .. }) => on_data(data.to_vec()),
                    Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                    _ => {}
                },
                cmd = rx.recv() => match cmd {
                    Some(Cmd::Data(d)) => { if ch.data(&d[..]).await.is_err() { break } }
                    Some(Cmd::Resize(c, r)) => { let _ = ch.window_change(c, r, 0, 0).await; }
                    Some(Cmd::Close) | None => { let _ = ch.close().await; break }
                },
            }
        }
        let _ = h2.disconnect(Disconnect::ByApplication, "", "").await;
        on_close();
    });
    Ok(Arc::new(Session { tx, handle, sftp: OnceCell::new() }))
}

// ---------- exec / stats ----------

pub async fn exec(handle: &Handle<Client>, cmd: &str) -> Result<String> {
    let mut ch = handle.channel_open_session().await?;
    ch.exec(true, cmd).await?;
    let mut out = Vec::new();
    while let Some(msg) = ch.wait().await {
        match msg {
            ChannelMsg::Data { data } => out.extend_from_slice(&data),
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Run through `sh` so it works whatever the remote login shell is (fish…).
pub const STATS_CMD: &str = r#"sh -c 'head -1 /proc/stat; grep -E "^(MemTotal|MemAvailable):" /proc/meminfo; df -Pk / | tail -1; cat /proc/loadavg; tail -n +3 /proc/net/dev; echo "HOST $(cat /proc/sys/kernel/hostname 2>/dev/null || hostname)"; grep -E "^(ID|PRETTY_NAME)=" /etc/os-release 2>/dev/null; echo "KERNEL $(uname -sr)"; echo "UPTIME $(cut -d" " -f1 /proc/uptime 2>/dev/null)"' 2>/dev/null"#;

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Stats {
    pub cpu_total: u64,
    pub cpu_idle: u64,
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub disk_total_kb: u64,
    pub disk_used_kb: u64,
    pub load: [f64; 3],
    pub net_rx: u64,
    pub net_tx: u64,
    pub hostname: String,
    pub os_id: String,
    pub os_name: String,
    pub kernel: String,
    pub uptime_secs: u64,
}

/// Raw counters; the UI diffs two samples for CPU % and network rates.
/// shortcut: Linux /proc only, BSD/macOS servers show empty stats — add sysctl parsing if needed.
pub fn parse_stats(s: &str) -> Stats {
    let mut st = Stats::default();
    for line in s.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let n = |i: usize| f.get(i).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
        let rest = |p: &str| line.strip_prefix(p).map(|v| v.trim().trim_matches('"').to_string());
        if let Some(v) = rest("HOST ") {
            st.hostname = v;
            continue;
        }
        if let Some(v) = rest("ID=") {
            st.os_id = v;
            continue;
        }
        if let Some(v) = rest("PRETTY_NAME=") {
            st.os_name = v;
            continue;
        }
        if let Some(v) = rest("KERNEL ") {
            st.kernel = v;
            continue;
        }
        if let Some(v) = rest("UPTIME ") {
            st.uptime_secs = v.split('.').next().and_then(|x| x.parse().ok()).unwrap_or(0);
            continue;
        }
        match f.first().copied() {
            Some("cpu") => {
                let v: Vec<u64> = f[1..].iter().filter_map(|x| x.parse().ok()).collect();
                st.cpu_total = v.iter().take(8).sum();
                st.cpu_idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
            }
            Some("MemTotal:") => st.mem_total_kb = n(1),
            Some("MemAvailable:") => st.mem_avail_kb = n(1),
            Some(x) if x.starts_with('/') && f.len() >= 6 => {
                st.disk_total_kb = n(1);
                st.disk_used_kb = n(2);
            }
            Some(x) if x.ends_with(':') && f.len() >= 10 => {
                if x != "lo:" {
                    st.net_rx += n(1);
                    st.net_tx += n(9);
                }
            }
            Some(_) if f.len() == 5 && f[3].contains('/') => {
                for i in 0..3 {
                    st.load[i] = f[i].parse().unwrap_or(0.0);
                }
            }
            _ => {}
        }
    }
    st
}

// ---------- sftp ----------

#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub modified: u64,
}

impl Session {
    pub async fn sftp(&self) -> Result<&SftpSession> {
        self.sftp
            .get_or_try_init(|| async {
                let ch = self.handle.channel_open_session().await?;
                ch.request_subsystem(true, "sftp").await?;
                Ok::<_, anyhow::Error>(SftpSession::new(ch.into_stream()).await?)
            })
            .await
    }
}

pub async fn sftp_list(s: &SftpSession, path: &str) -> Result<(String, Vec<Entry>)> {
    let path = s.canonicalize(if path.is_empty() { "." } else { path }).await?;
    let mut v: Vec<Entry> = s
        .read_dir(&path)
        .await?
        .map(|e| {
            let m = e.metadata();
            Entry {
                dir: m.is_dir(),
                size: m.len(),
                modified: m.mtime.unwrap_or(0) as u64,
                name: e.file_name(),
            }
        })
        .filter(|e| e.name != "." && e.name != "..")
        .collect();
    v.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok((path, v))
}

pub async fn sftp_download(s: &SftpSession, remote: &str, local: &str) -> Result<()> {
    let mut f = s.open(remote).await?;
    let mut out = tokio::fs::File::create(local).await?;
    tokio::io::copy(&mut f, &mut out).await?;
    Ok(())
}

pub async fn sftp_upload(s: &SftpSession, local: &str, remote: &str) -> Result<()> {
    let mut src = tokio::fs::File::open(local).await?;
    let mut f = s.create(remote).await?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = src.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).await?;
    }
    f.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_proc_output() {
        let raw = "cpu  100 0 50 800 50 0 0 0 0 0\nMemTotal:  2000 kB\nMemAvailable: 500 kB\n/dev/sda1 1000 400 600 40% /\n0.10 0.20 0.30 1/100 42\n    lo: 5 0 0 0 0 0 0 0 5 0 0 0 0 0 0 0\n  eth0: 1000 0 0 0 0 0 0 0 2000 0 0 0 0 0 0 0\nHOST pve-31\nID=debian\nPRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nKERNEL Linux 6.8.12\nUPTIME 3600.52 7000.1\n";
        let s = parse_stats(raw);
        assert_eq!((s.cpu_total, s.cpu_idle), (1000, 850));
        assert_eq!((s.mem_total_kb, s.mem_avail_kb), (2000, 500));
        assert_eq!((s.disk_total_kb, s.disk_used_kb), (1000, 400));
        assert_eq!(s.load, [0.1, 0.2, 0.3]);
        assert_eq!((s.net_rx, s.net_tx), (1000, 2000));
        assert_eq!((s.hostname.as_str(), s.os_id.as_str()), ("pve-31", "debian"));
        assert_eq!(s.os_name, "Debian GNU/Linux 12 (bookworm)");
        assert_eq!((s.kernel.as_str(), s.uptime_secs), ("Linux 6.8.12", 3600));
    }
}
