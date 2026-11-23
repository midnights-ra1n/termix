//! Host reachability: "up" = SSH banner received, "ping" = ICMP answers but no SSH, "down" = neither.
use crate::config::Host;
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

const T: Duration = Duration::from_secs(3);

async fn ssh_ok(host: &str, port: u16) -> bool {
    let Ok(Ok(mut s)) = timeout(T, TcpStream::connect((host, port))).await else { return false };
    let mut buf = [0u8; 4];
    matches!(timeout(T, s.read_exact(&mut buf)).await, Ok(Ok(_))) && &buf == b"SSH-"
}

async fn ping_ok(host: &str) -> bool {
    if cfg!(target_os = "ios") {
        return false; // an iOS app can't run `ping`
    }
    let mut c = tokio::process::Command::new("ping");
    if cfg!(target_os = "windows") {
        c.args(["-n", "1", "-w", "2000", host]);
    } else if cfg!(target_os = "macos") {
        c.args(["-c", "1", "-t", "2", host]);
    } else {
        c.args(["-c", "1", "-W", "2", host]);
    }
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash
    c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    matches!(timeout(Duration::from_secs(4), c.status()).await, Ok(Ok(s)) if s.success())
}

pub async fn probe(h: &Host) -> &'static str {
    let port = if h.port == 0 { 22 } else { h.port };
    if ssh_ok(&h.host, port).await {
        "up"
    } else if ping_ok(&h.host).await {
        "ping"
    } else {
        "down"
    }
}

pub async fn probe_all(hosts: &[Host]) -> HashMap<String, String> {
    let tasks: Vec<_> = hosts
        .iter()
        .cloned()
        .map(|h| tokio::spawn(async move { (h.id.clone(), probe(&h).await.to_string()) }))
        .collect();
    let mut out = HashMap::new();
    for t in tasks {
        if let Ok((id, s)) = t.await {
            out.insert(id, s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn closed_port_is_not_ssh() {
        assert!(!super::ssh_ok("127.0.0.1", 1).await);
    }
}
