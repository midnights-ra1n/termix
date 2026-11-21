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
    let raw = p.trim().trim_matches('"');
    match ["~/", "$HOME/", "${HOME}/"].iter().find_map(|pre| raw.strip_prefix(pre)) {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(raw),
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
        // $SHELL is the login shell, often bash even when the terminal runs zsh/fish: ask each installed shell,
        // without the inherited SSH_AUTH_SOCK so only what the rc file exports counts.
        let mut shells: Vec<String> = std::env::var("SHELL").ok().into_iter().collect();
        shells.extend(["zsh", "bash", "fish"].map(String::from));
        shells.dedup_by(|a, b| a.rsplit('/').next() == b.rsplit('/').next());
        shells.into_iter().find_map(|sh| {
            let out = std::process::Command::new(sh)
                .args(["-l", "-i", "-c", "echo __RT__$SSH_AUTH_SOCK"])
                .env_remove("SSH_AUTH_SOCK")
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()?;
            let s = String::from_utf8_lossy(&out.stdout);
            let v = s.lines().find_map(|l| l.strip_prefix("__RT__"))?.trim().to_string();
            (!v.is_empty() && alive(&v)).then_some(v)
        })
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
        ("Lightwarden".into(), h.join(".lightwarden-ssh-agent.sock")),
        ("Proton Pass".into(), h.join(".ssh/proton-pass-agent.sock")),
        ("Secretive".into(), h.join("Library/Containers/com.maxgoedjen.Secretive.SecretAgent/Data/socket.ssh")),
    ];
    if let Ok(x) = std::env::var("XDG_RUNTIME_DIR") {
        v.push(("GNOME Keyring / KeePassXC".into(), PathBuf::from(&x).join("keyring/ssh")));
        v.push(("ssh-agent (systemd)".into(), PathBuf::from(x).join("ssh-agent.socket")));
    }
    let mut out: Vec<(String, String)> =
        v.into_iter().map(|(n, p)| (n, p.to_string_lossy().into_owned())).filter(|(_, p)| alive(p)).collect();
    if let Some(s) = ssh_config_agent().filter(|s| alive(s)) {
        out.insert(0, ("~/.ssh/config (IdentityAgent)".into(), s));
    }
    if let Some(s) = shell_env_agent().filter(|s| alive(s)) {
        out.insert(0, ("Shell ($SSH_AUTH_SOCK)".into(), s));
    }
    if cfg!(windows) {
        out.push(("OpenSSH (Windows, KeePassXC, Bitwarden, 1Password)".into(), r"\\.\pipe\openssh-ssh-agent".into()));
    }
    out
}

fn shell_env_agent() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    // The login shell wins: on macOS launchd hands every GUI app an SSH_AUTH_SOCK for the (empty) system agent,
    // which would shadow the agent exported in .zshrc/.bashrc/config.fish.
    CACHE.get_or_init(|| shell_agent().or_else(|| std::env::var("SSH_AUTH_SOCK").ok().filter(|s| !s.is_empty()))).clone()
}

/// A socket file can outlive its agent (app quit or crashed): only keep ones that accept a connection.
fn alive(sock: &str) -> bool {
    #[cfg(unix)]
    return std::os::unix::net::UnixStream::connect(sock).is_ok();
    #[cfg(not(unix))]
    return !sock.is_empty();
}

pub fn resolve_agent(configured: &str) -> Option<String> {
    if cfg!(windows) {
        let s = configured.trim();
        return Some(if s.is_empty() { r"\\.\pipe\openssh-ssh-agent".into() } else { s.into() });
    }
    // A forced socket that is dead (stale file) falls back to auto-detection instead of a password prompt.
    let forced = expand(configured).to_string_lossy().into_owned();
    if !configured.trim().is_empty() && alive(&forced) {
        return Some(forced);
    }
    ssh_config_agent().filter(|s| alive(s)).or_else(|| shell_env_agent().filter(|s| alive(s)))
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
    // A key file wins; otherwise a key from the sync vault.
    let vault_pem = if host.key_path.is_empty() && !host.key_id.is_empty() { secret_get(&format!("key:{}", host.key_id)) } else { None };
    if !host.key_path.is_empty() || vault_pem.is_some() {
        let pass = secret_get(&format!("{}:passphrase", host.id));
        let key = match &vault_pem {
            Some(pem) => russh::keys::decode_secret_key(pem, pass.as_deref()).context("lecture de la clé synchronisée")?,
            None => russh::keys::load_secret_key(expand(&host.key_path), pass.as_deref())
                .with_context(|| format!("lecture de la clé {}", host.key_path))?,
        };
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
    tmux: Option<&str>,
    on_data: impl Fn(Vec<u8>) + Send + 'static,
    on_close: impl FnOnce() + Send + 'static,
) -> Result<Arc<Session>> {
    let mut ch = handle.channel_open_session().await?;
    ch.request_pty(false, "xterm-256color", cols, rows, 0, 0, &[]).await?;
    match tmux {
        Some(name) => ch.exec(false, tmux_cmd(name)?).await?,
        None => ch.request_shell(false).await?,
    }
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

// ---------- tmux: shells that survive the SSH connection ----------

/// tmux session names Termix can pass to a remote shell without quoting.
pub fn valid_tmux_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Attach the session, creating it if needed (mouse on so the wheel scrolls tmux history, status bar off so it
/// looks like a plain shell); without tmux on the server, a plain login shell.
/// An exec channel gets no MOTD from sshd, so a new session prints it itself. tmux < 3.5 defaults to TERM=screen,
/// which shells' prompts and `ls` treat as colourless: upgrade it to screen-256color unless the user chose one.
pub fn tmux_cmd(name: &str) -> Result<String> {
    if !valid_tmux_name(name) {
        bail!("nom de session tmux invalide : {name}");
    }
    Ok(format!(
        r##"sh -c 'export COLORTERM=truecolor; M="cat /run/motd.dynamic /etc/motd 2>/dev/null"; command -v tmux >/dev/null 2>&1 && exec tmux if -F "#{{==:#{{default-terminal}},screen}}" "set -g default-terminal screen-256color" \; new-session -A -s {name} "$M; exec \"\$SHELL\" -l" \; set-option mouse on \; set-option status off; eval "$M"; exec "${{SHELL:-sh}}" -l'"##
    ))
}

pub const TMUX_LS_CMD: &str = r##"sh -c 'command -v tmux >/dev/null 2>&1 || { echo NOTMUX; exit 0; }; tmux ls -F "#{session_name}|#{session_windows}|#{session_attached}|#{session_activity}|#{pane_current_command}" 2>/dev/null; true'"##;

#[derive(Serialize, Debug, PartialEq)]
pub struct TmuxSession {
    pub name: String,
    pub windows: u32,
    pub attached: bool,
    pub activity: u64,
    pub command: String,
}

/// `None` when tmux is not installed on the server.
pub fn parse_tmux(raw: &str) -> Option<Vec<TmuxSession>> {
    if raw.lines().any(|l| l.trim() == "NOTMUX") {
        return None;
    }
    let mut v: Vec<TmuxSession> = raw
        .lines()
        .filter_map(|l| {
            // From the right: only the session name may contain '|'.
            let mut f = l.rsplitn(5, '|');
            let (command, activity, attached, windows, name) = (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
            Some(TmuxSession {
                name: name.into(),
                windows: windows.parse().ok()?,
                attached: attached != "0",
                activity: activity.parse().unwrap_or(0),
                command: command.into(),
            })
        })
        .collect();
    v.sort_by(|a, b| b.activity.cmp(&a.activity));
    Some(v)
}

pub fn tmux_kill_cmd(name: &str) -> Result<String> {
    if !valid_tmux_name(name) {
        bail!("nom de session tmux invalide : {name}");
    }
    Ok(format!("sh -c 'tmux kill-session -t ={name} 2>&1'"))
}

// ---------- docker containers ----------

/// Read-only listing, nothing else is ever run against the daemon.
pub const DOCKER_PS_CMD: &str = r#"sh -c 'command -v docker >/dev/null 2>&1 || { echo NODOCKER; exit 0; }; docker ps -a --format "{{.Names}}|{{.Image}}|{{.State}}|{{.Status}}" 2>&1; true'"#;

#[derive(Serialize, Debug, PartialEq)]
pub struct Container {
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Docker {
    pub installed: bool,
    pub containers: Vec<Container>,
    /// Daemon/permission error when no container could be listed.
    pub error: String,
}

pub fn parse_docker(raw: &str) -> Docker {
    if raw.lines().any(|l| l.trim() == "NODOCKER") {
        return Docker { installed: false, containers: vec![], error: String::new() };
    }
    let (mut containers, mut errors) = (vec![], vec![]);
    for l in raw.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = l.splitn(4, '|').collect();
        match f[..] {
            [name, image, state, status] => containers.push(Container { name: name.into(), image: image.into(), state: state.into(), status: status.into() }),
            _ => errors.push(l.trim()),
        }
    }
    let rank = |s: &str| match s { "running" => 0, "restarting" | "paused" => 1, _ => 2 };
    containers.sort_by(|a, b| rank(&a.state).cmp(&rank(&b.state)).then_with(|| a.name.cmp(&b.name)));
    let error = if containers.is_empty() { errors.join("\n") } else { String::new() };
    Docker { installed: true, containers, error }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn tmux_names_are_checked_before_reaching_the_shell() {
        assert!(tmux_cmd("rt-a1b2_C").unwrap().contains("new-session -A -s rt-a1b2_C"));
        assert!(tmux_cmd("x").unwrap().contains("/etc/motd"), "exec channels get no MOTD from sshd");
        for bad in ["", "a b", "x';rm -rf ~'", "a;b", "$(id)"] {
            assert!(tmux_cmd(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_tmux_ls() {
        assert_eq!(parse_tmux("NOTMUX\n"), None);
        assert_eq!(parse_tmux("").unwrap(), vec![]);
        let v = parse_tmux("main|2|1|1700000000|vim\nrt-x|1|0|1700000500|htop\n").unwrap();
        assert_eq!(v[0], TmuxSession { name: "rt-x".into(), windows: 1, attached: false, activity: 1700000500, command: "htop".into() });
        assert!(v[1].attached);
    }

    #[test]
    fn parses_docker_ps() {
        assert!(!parse_docker("NODOCKER\n").installed);
        let d = parse_docker("web|nginx:1.27|exited|Exited (0) 2 hours ago\ndb|postgres:16|running|Up 3 days\n");
        assert_eq!(d.containers.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["db", "web"]);
        assert_eq!(d.error, "");
        let denied = parse_docker("permission denied while trying to connect to the Docker daemon socket\n");
        assert!(denied.installed && denied.containers.is_empty() && denied.error.starts_with("permission denied"));
    }
}

// ---------- remote shell history (seed for command suggestions) ----------

pub const HISTORY_CMD: &str = r#"sh -c 'tail -n 3000 ~/.bash_history ~/.zsh_history ~/.local/share/fish/fish_history 2>/dev/null' 2>/dev/null"#;

/// Parses bash, zsh (incl. EXTENDED_HISTORY `: ts:0;cmd`) and fish (`- cmd: …`) history. Most recent last, deduplicated.
pub fn parse_history(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        if line.starts_with("==> ") {
            continue; // `tail` file headers
        }
        let cmd = if let Some(rest) = line.strip_prefix("- cmd: ") {
            rest
        } else if line.starts_with("  when:") || line.starts_with("  paths:") || line.starts_with("    - ") || line.starts_with('#') {
            continue;
        } else if let Some(rest) = line.strip_prefix(": ").and_then(|r| r.split_once(';')).map(|(_, c)| c) {
            rest
        } else {
            line
        };
        let cmd = cmd.trim();
        if cmd.len() < 2 || cmd.len() > 500 || cmd.ends_with('\\') {
            continue;
        }
        out.retain(|c| c != cmd);
        out.push(cmd.to_string());
    }
    out
}

#[cfg(test)]
mod history_tests {
    #[test]
    fn parses_all_shells() {
        let raw = "==> /root/.bash_history <==\nls -la\ngit status\n#1700000000\nls -la\n\n==> /root/.zsh_history <==\n: 1700000000:0;docker ps\n\n==> /root/.local/share/fish/fish_history <==\n- cmd: systemctl status nginx\n  when: 1700000000\n";
        assert_eq!(super::parse_history(raw), vec!["git status", "ls -la", "docker ps", "systemctl status nginx"]);
    }
}

#[cfg(all(test, unix))]
mod agent_tests {
    #[test]
    fn stale_agent_socket_is_skipped() {
        let dir = std::env::temp_dir().join(format!("rt-agent-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("a.sock");
        let _ = std::fs::remove_file(&p);
        let l = std::os::unix::net::UnixListener::bind(&p).unwrap();
        let s = p.to_string_lossy().into_owned();
        assert!(super::alive(&s));
        assert_eq!(super::resolve_agent(&s).as_deref(), Some(s.as_str()));
        drop(l); // the file stays, like a quit agent's socket
        assert!(!super::alive(&s));
        assert_ne!(super::resolve_agent(&s).as_deref(), Some(s.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn home_is_expanded() {
        let h = dirs::home_dir().unwrap().join("a.sock");
        for p in ["~/a.sock", "$HOME/a.sock", "${HOME}/a.sock", "\"${HOME}/a.sock\""] {
            assert_eq!(super::expand(p), h, "{p}");
        }
    }
}
