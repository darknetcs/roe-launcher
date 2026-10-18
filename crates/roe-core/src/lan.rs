//! Finding other launchers on the local network, for the Server Browser.
//!
//! A launcher that searches broadcasts `QUERY` to UDP `DISCOVERY_PORT`. A launcher that has
//! "visible on the network" switched on answers with an `Announcement` (JSON) while its local
//! server is running. The answer only describes the server: the game connects to `host:port`
//! as usual, so this protocol never touches the game ports.
//!
//! Only `std::net` is used, so it works the same on Linux and Windows. Answers go only to
//! private, link-local or loopback addresses, and a malformed query gets no answer.
use crate::servers::{validate_address, ServerEntry, Source};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Not one of the game ports (8001, 9001, 18001).
pub const DISCOVERY_PORT: u16 = 18110;
pub const QUERY: &[u8] = b"ROE-LAUNCHER-DISCOVER/1";
pub const PROTOCOL: u32 = 1;
const MAX_REPLY: usize = 1024;

/// What a visible launcher says about its server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Announcement {
    pub protocol: u32,
    /// Random per launcher run, so a search can skip its own answer.
    pub instance: String,
    pub name: String,
    /// The game port on the answering computer.
    pub port: u16,
    pub players: u32,
    pub max_players: u32,
    pub version: Option<String>,
    pub mode: Option<String>,
}

impl Default for Announcement {
    fn default() -> Self {
        Self { protocol: PROTOCOL, instance: String::new(), name: String::new(), port: 8001, players: 0, max_players: 0, version: None, mode: None }
    }
}

/// Turns an answer into a browser entry; the host is the address the answer came from.
pub fn parse_reply(bytes: &[u8], from: SocketAddr, skip_instance: Option<&str>) -> Option<ServerEntry> {
    if bytes.len() > MAX_REPLY {
        return None;
    }
    let a: Announcement = serde_json::from_slice(bytes).ok()?;
    if a.protocol != PROTOCOL || a.port == 0 || skip_instance.is_some_and(|s| s == a.instance) {
        return None;
    }
    let address = SocketAddr::new(from.ip(), a.port).to_string();
    validate_address(&address).ok()?;
    let name: String = a.name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
    Some(ServerEntry {
        id: format!("lan:{address}"),
        name: if name.is_empty() { format!("Launcher at {}", from.ip()) } else { name },
        address,
        region: Some("LAN".into()),
        players: a.players,
        max_players: a.max_players,
        version: a.version,
        mode: a.mode,
        source: Source::Lan,
        ..Default::default()
    })
}

/// Broadcasts a query on the local network and collects the answers for `wait`.
pub fn discover(wait: Duration, skip_instance: Option<&str>) -> Result<Vec<ServerEntry>> {
    discover_at(SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DISCOVERY_PORT), wait, skip_instance)
}

/// Same as `discover`, sent to one address (used by tests and for a single known host).
pub fn discover_at(target: SocketAddr, wait: Duration, skip_instance: Option<&str>) -> Result<Vec<ServerEntry>> {
    let bind: SocketAddr = if target.ip().is_loopback() { "127.0.0.1:0" } else { "0.0.0.0:0" }.parse().unwrap();
    let sock = UdpSocket::bind(bind).context("could not open a UDP socket for the network search")?;
    sock.set_broadcast(true)?;
    sock.send_to(QUERY, target).context("could not send the network search")?;
    let mut found: Vec<ServerEntry> = Vec::new();
    let mut buf = [0u8; MAX_REPLY + 1];
    let end = Instant::now() + wait;
    while let Some(left) = end.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
        sock.set_read_timeout(Some(left))?;
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(e) = parse_reply(&buf[..n], from, skip_instance) {
                    if !found.iter().any(|f| f.address == e.address) {
                        found.push(e);
                    }
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
            // e.g. Windows reports "connection reset" for an ICMP port-unreachable: keep listening
            Err(_) => continue,
        }
    }
    Ok(found)
}

/// Only answer computers on the same network.
fn local_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local(),
        IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xffc0) == 0xfe80 || (v.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// Answers searches in a background thread until dropped.
pub struct Responder {
    stop: Arc<AtomicBool>,
    pub local: SocketAddr,
}

impl Responder {
    /// `bind` is usually `0.0.0.0:DISCOVERY_PORT`. `info` is asked on every query;
    /// `None` = stay silent (e.g. the server is not running).
    pub fn start(bind: SocketAddr, info: Arc<dyn Fn() -> Option<Announcement> + Send + Sync>) -> Result<Responder> {
        let sock = UdpSocket::bind(bind).with_context(|| format!("could not listen on UDP port {} (is another launcher visible on this computer?)", bind.port()))?;
        sock.set_read_timeout(Some(Duration::from_millis(400)))?;
        let local = sock.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            while !flag.load(Ordering::Relaxed) {
                let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
                if &buf[..n] != QUERY || !local_peer(from.ip()) {
                    continue;
                }
                if let Some(a) = info() {
                    if let Ok(bytes) = serde_json::to_vec(&a) {
                        if bytes.len() <= MAX_REPLY {
                            let _ = sock.send_to(&bytes, from);
                        }
                    }
                }
            }
        });
        Ok(Responder { stop, local })
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ann(instance: &str) -> Announcement {
        Announcement { instance: instance.into(), name: "Living-room PC".into(), players: 1, max_players: 1, mode: Some("Training".into()), ..Default::default() }
    }

    #[test]
    fn parses_answers_and_rejects_bad_ones() {
        let from: SocketAddr = "192.168.1.20:18110".parse().unwrap();
        let e = parse_reply(&serde_json::to_vec(&ann("a")).unwrap(), from, None).unwrap();
        assert_eq!((e.address.as_str(), e.name.as_str(), e.source), ("192.168.1.20:8001", "Living-room PC", Source::Lan));
        assert!(parse_reply(&serde_json::to_vec(&ann("me")).unwrap(), from, Some("me")).is_none());
        assert!(parse_reply(b"not json", from, None).is_none());
        let wrong = Announcement { protocol: 99, ..ann("a") };
        assert!(parse_reply(&serde_json::to_vec(&wrong).unwrap(), from, None).is_none());
        let unnamed = Announcement { name: "\u{7}  ".into(), ..ann("a") };
        assert_eq!(parse_reply(&serde_json::to_vec(&unnamed).unwrap(), from, None).unwrap().name, "Launcher at 192.168.1.20");
    }

    #[test]
    fn only_local_networks_get_answers() {
        for ok in ["192.168.0.5", "10.1.2.3", "172.16.0.1", "127.0.0.1", "169.254.1.1", "::1", "fe80::1", "fd00::1"] {
            assert!(local_peer(ok.parse().unwrap()), "{ok}");
        }
        for bad in ["8.8.8.8", "203.0.113.5", "2001:db8::1"] {
            assert!(!local_peer(bad.parse().unwrap()), "{bad}");
        }
    }

    #[test]
    fn a_search_finds_a_visible_launcher_over_loopback() {
        let visible = Arc::new(AtomicBool::new(true));
        let v = visible.clone();
        let r = Responder::start("127.0.0.1:0".parse().unwrap(), Arc::new(move || v.load(Ordering::Relaxed).then(|| ann("other")))).unwrap();
        let found = discover_at(r.local, Duration::from_millis(500), Some("mine")).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].address, "127.0.0.1:8001");
        // its own answer is skipped
        assert!(discover_at(r.local, Duration::from_millis(300), Some("other")).unwrap().is_empty());
        // silent while there is nothing to announce
        visible.store(false, Ordering::Relaxed);
        assert!(discover_at(r.local, Duration::from_millis(300), None).unwrap().is_empty());
    }
}
