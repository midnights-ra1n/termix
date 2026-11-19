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

/// Small filled circle for native menu items.
pub fn dot(status: &str) -> tauri::image::Image<'static> {
    let (r, g, b) = match status {
        "up" => (0x34, 0xC7, 0x59),
        "ping" => (0xFF, 0xCC, 0x00),
        "down" => (0xFF, 0x3B, 0x30),
        _ => (0x8E, 0x8E, 0x93),
    };
    const N: usize = 32;
    let mut px = vec![0u8; N * N * 4];
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let a = (9.5 - d).clamp(0.0, 1.0); // radius ~10px out of 32, antialiased edge
            let i = (y * N + x) * 4;
            px[i..i + 4].copy_from_slice(&[r, g, b, (a * 255.0) as u8]);
        }
    }
    tauri::image::Image::new_owned(px, N as u32, N as u32)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn closed_port_is_not_ssh() {
        assert!(!super::ssh_ok("127.0.0.1", 1).await);
    }
}
