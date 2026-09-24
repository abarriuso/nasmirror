use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

#[derive(Debug, thiserror::Error)]
pub enum WolError {
    #[error("invalid MAC address: {0}")]
    InvalidMac(String),
    #[error("the destination ({0}) did not respond after Wake-on-LAN (timed out)")]
    Timeout(String),
    #[error("Wake-on-LAN cancelled")]
    Cancelled,
}

fn parse_mac(mac: &str) -> Result<[u8; 6], WolError> {
    let hex: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 12 {
        return Err(WolError::InvalidMac(mac.to_string()));
    }
    let mut bytes = [0u8; 6];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| WolError::InvalidMac(mac.to_string()))?;
    }
    Ok(bytes)
}

fn magic_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut pkt = [0u8; 102];
    pkt[..6].copy_from_slice(&[0xFF; 6]);
    for i in 0..16 {
        pkt[6 + i * 6..6 + i * 6 + 6].copy_from_slice(&mac);
    }
    pkt
}

async fn send_magic(pkt: &[u8; 102]) {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else {
        return;
    };
    let _ = sock.set_broadcast(true);
    for port in [9u16, 7u16] {
        let _ = sock
            .send_to(pkt, SocketAddr::from(([255, 255, 255, 255], port)))
            .await;
    }
}

/// Whether something is listening on the SMB port (445) of `host`. Used both
/// to decide whether the destination needs waking and to detect when it is up.
/// DNS resolution runs in `spawn_blocking` because `to_socket_addrs` is
/// synchronous and can be slow on some networks.
pub async fn host_reachable(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    let addr_str = format!("{host}:445");
    let resolved = tokio::task::spawn_blocking(move || addr_str.to_socket_addrs())
        .await
        .ok()
        .and_then(|r| r.ok())
        .and_then(|mut addrs| addrs.next());
    let Some(sockaddr) = resolved else {
        return false;
    };
    matches!(
        timeout(Duration::from_millis(1500), TcpStream::connect(sockaddr)).await,
        Ok(Ok(_))
    )
}

/// Sends the magic packet. If a host is given, resends it every 3 s for up to
/// `timeout_secs` until the host answers on port 445; without a host, resends
/// a few times and waits a fixed grace period. Honours `cancel` so the user
/// can stop waiting from the UI.
pub async fn wake_and_wait(
    mac: &str,
    host: &str,
    timeout_secs: u32,
    cancel: Arc<AtomicBool>,
) -> Result<(), WolError> {
    let mac_bytes = parse_mac(mac)?;
    let pkt = magic_packet(mac_bytes);

    if host_reachable(host).await {
        return Ok(()); // already awake
    }

    send_magic(&pkt).await;

    if host.is_empty() {
        for _ in 0..4 {
            if cancel.load(Ordering::SeqCst) {
                return Err(WolError::Cancelled);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            send_magic(&pkt).await;
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
        return Ok(());
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs as u64);
    while tokio::time::Instant::now() < deadline {
        if cancel.load(Ordering::SeqCst) {
            return Err(WolError::Cancelled);
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        send_magic(&pkt).await;
        if host_reachable(host).await {
            return Ok(());
        }
    }
    Err(WolError::Timeout(host.to_string()))
}

pub fn guess_host_from_share(share: &str) -> Option<String> {
    let rest = share.strip_prefix("\\\\")?;
    let host = rest.split('\\').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mac_formats() {
        let expected = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        assert_eq!(parse_mac("AA:BB:CC:DD:EE:FF").unwrap(), expected);
        assert_eq!(parse_mac("aa-bb-cc-dd-ee-ff").unwrap(), expected);
        assert_eq!(parse_mac("aabbccddeeff").unwrap(), expected);
        assert!(parse_mac("AA:BB:CC").is_err());
        assert!(parse_mac("").is_err());
    }

    #[test]
    fn magic_packet_layout() {
        let mac = [1, 2, 3, 4, 5, 6];
        let pkt = magic_packet(mac);
        assert_eq!(&pkt[..6], &[0xFF; 6]);
        for i in 0..16 {
            assert_eq!(&pkt[6 + i * 6..12 + i * 6], &mac);
        }
    }

    #[test]
    fn host_from_share() {
        assert_eq!(guess_host_from_share(r"\\nas\backup").as_deref(), Some("nas"));
        assert_eq!(guess_host_from_share(r"C:\data"), None);
        assert_eq!(guess_host_from_share(r"\\"), None);
    }
}
