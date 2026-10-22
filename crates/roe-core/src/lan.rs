//! Finding other launchers on the local network, for the Server Browser.
//!
//! A launcher that searches sends `QUERY` to UDP `DISCOVERY_PORT`: as a broadcast, and also directly to every
//! address of its own private /24 network (a "sweep"), because many firewalls (e.g. ufw's default deny) drop the
//! answers to a broadcast but let through an answer from the very address that was asked. A launcher that has
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
    /// Set when the host lets others join (see `relay`): its [game, tunnel] relay ports. Older launchers omit it.
    pub relay: Option<[u16; 2]>,
    /// The server profile the host's server runs with: the joining bridge must use the same one (the server's LAN
    /// tunnel refuses a different one). Older launchers omit it, meaning "default".
    pub profile: Option<String>,
}

impl Default for Announcement {
    fn default() -> Self {
        Self { protocol: PROTOCOL, instance: String::new(), name: String::new(), port: 8001, players: 0, max_players: 0, version: None, mode: None, relay: None, profile: None }
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
        relay: a.relay.filter(|p| p.iter().all(|&x| x != 0)),
        profile: a.profile.and_then(|p| crate::profiles::validate(&p).ok()),
        source: Source::Lan,
        ..Default::default()
    })
}

/// Searches the local network (broadcast + sweep of this computer's private /24) and collects the answers for `wait`.
pub fn discover(wait: Duration, skip_instance: Option<&str>) -> Result<Vec<ServerEntry>> {
    search(true, &[], wait, skip_instance).map(|(found, _)| found)
}

/// One search for the Server Browser: the local network (if `network`) plus the computers the user added by address
/// (`hosts`, e.g. Hamachi or Radmin VPN peers the network search can't see). Returns what answered and the hosts
/// that didn't.
pub fn search(network: bool, hosts: &[IpAddr], wait: Duration, skip_instance: Option<&str>) -> Result<(Vec<ServerEntry>, Vec<IpAddr>)> {
    let mut targets = Vec::new();
    if network {
        targets.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DISCOVERY_PORT));
        if let Some(ip) = local_ipv4() {
            targets.extend(sweep_targets(ip));
        }
    }
    targets.extend(hosts.iter().map(|&ip| SocketAddr::new(ip, DISCOVERY_PORT)));
    if targets.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let found = discover_many(&targets, wait, skip_instance)?;
    let silent = hosts
        .iter()
        .filter(|ip| !found.iter().any(|e| e.address.parse::<SocketAddr>().is_ok_and(|a| a.ip() == **ip)))
        .copied()
        .collect();
    Ok((found, silent))
}

/// A host address typed by the user: an IPv4/IPv6 address, with or without a port (the port is ignored).
pub fn parse_host(text: &str) -> Option<IpAddr> {
    let t = text.trim();
    t.parse::<IpAddr>().ok().or_else(|| t.parse::<SocketAddr>().ok().map(|a| a.ip()))
}

/// Same as `discover`, sent to one address (used by tests and for a single known host).
pub fn discover_at(target: SocketAddr, wait: Duration, skip_instance: Option<&str>) -> Result<Vec<ServerEntry>> {
    discover_many(&[target], wait, skip_instance)
}

/// This computer's IPv4 address on its main network, or None (e.g. no network). Uses the routing table: "connecting"
/// a UDP socket sends nothing, it only picks the local address that would be used.
pub fn local_ipv4() -> Option<Ipv4Addr> {
    // any public address works; nothing is sent to it
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v) if !v.is_unspecified() && !v.is_loopback() => Some(v),
        _ => None,
    }
}

/// The other addresses of `ip`'s /24, only for private networks (never sweeps public addresses).
pub fn sweep_targets(ip: Ipv4Addr) -> Vec<SocketAddr> {
    if !ip.is_private() {
        return Vec::new();
    }
    let [a, b, c, own] = ip.octets();
    (1..=254u8)
        .filter(|&d| d != own)
        .map(|d| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(a, b, c, d)), DISCOVERY_PORT))
        .collect()
}

/// Sends the query to every target from one socket and collects the answers for `wait`.
fn discover_many(targets: &[SocketAddr], wait: Duration, skip_instance: Option<&str>) -> Result<Vec<ServerEntry>> {
    let loopback = targets.iter().all(|t| t.ip().is_loopback());
    let bind: SocketAddr = if loopback { "127.0.0.1:0" } else { "0.0.0.0:0" }.parse().unwrap();
    let sock = UdpSocket::bind(bind).context("could not open a UDP socket for the network search")?;
    sock.set_broadcast(true)?;
    let mut sent = 0;
    for t in targets {
        // a single unreachable address (or a refused broadcast) must not stop the search
        if sock.send_to(QUERY, t).is_ok() {
            sent += 1;
        }
    }
    if sent == 0 {
        anyhow::bail!("could not send the network search");
    }
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
    crate::relay::lan_peer(ip)
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

    #[test]
    fn a_hosts_profile_reaches_the_joiner_in_the_servers_form() {
        let from: SocketAddr = "192.168.1.20:18110".parse().unwrap();
        let reply = |profile: Option<&str>| serde_json::to_vec(&Announcement { profile: profile.map(String::from), ..ann("h") }).unwrap();
        assert_eq!(parse_reply(&reply(Some("Wojtek")), from, None).unwrap().profile.as_deref(), Some("wojtek"));
        assert_eq!(parse_reply(&reply(Some("not ok!")), from, None).unwrap().profile, None);
        // an older launcher doesn't send it at all: the joiner uses "default"
        let old = br#"{"protocol":1,"instance":"o","name":"Old PC","port":8001,"players":0,"max_players":1}"#;
        assert_eq!(parse_reply(old, from, None).unwrap().profile, None);
    }

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

    /// The real search on this computer's network; prints what it finds (`ROE_EXPECT_HOST=10.113.0.4` also checks
    /// that this host answered). Needs a launcher on the network with "visible" on and its server running.
    #[test]
    #[ignore]
    fn live_discover() {
        println!("local address: {:?}", local_ipv4());
        let found = discover(Duration::from_millis(1500), None).unwrap();
        for e in &found {
            println!("found {} at {} ({}/{} players, {:?})", e.name, e.address, e.players, e.max_players, e.version);
        }
        if let Ok(host) = std::env::var("ROE_EXPECT_HOST") {
            assert!(found.iter().any(|e| e.address.starts_with(&format!("{host}:"))), "{host} did not answer");
        }
    }

    #[test]
    fn a_host_that_lets_others_join_can_be_joined() {
        let from: SocketAddr = "10.113.0.4:18110".parse().unwrap();
        let mut a = ann("host");
        // an older launcher (no relay): listed, but not joinable
        let old = parse_reply(&serde_json::to_vec(&a).unwrap(), from, None).unwrap();
        assert_eq!(old.relay, None);
        assert!(crate::servers::can_join(&old).is_err());
        a.relay = Some([crate::relay::HOST_GAME_PORT, crate::relay::HOST_TUNNEL_PORT]);
        let host = parse_reply(&serde_json::to_vec(&a).unwrap(), from, None).unwrap();
        assert_eq!(host.relay, Some([28001, 29001]));
        assert!(crate::servers::can_join(&host).is_ok());
        // a zero port is ignored
        a.relay = Some([0, 29001]);
        assert_eq!(parse_reply(&serde_json::to_vec(&a).unwrap(), from, None).unwrap().relay, None);
    }

    #[test]
    fn hosts_added_by_address_are_asked_directly() {
        // a visible launcher on loopback stands in for a Hamachi peer
        let r = Responder::start("127.0.0.1:0".parse().unwrap(), Arc::new(|| Some(ann("vpn-host")))).unwrap();
        let port = r.local.port();
        let found = discover_many(&[SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)], Duration::from_millis(800), None).unwrap();
        assert_eq!(found.len(), 1);
        // search() reports a silent host (nothing listens on the discovery port of 127.0.0.2)
        let (_, silent) = search(false, &["127.0.0.2".parse().unwrap()], Duration::from_millis(300), None).unwrap();
        assert_eq!(silent, vec!["127.0.0.2".parse::<IpAddr>().unwrap()]);
        assert_eq!(parse_host(" 25.1.2.3 "), Some("25.1.2.3".parse().unwrap()));
        assert_eq!(parse_host("26.4.5.6:8001"), Some("26.4.5.6".parse().unwrap()));
        assert_eq!(parse_host("not an ip"), None);
        assert_eq!(search(false, &[], Duration::from_millis(10), None).unwrap(), (Vec::new(), Vec::new()));
    }

    #[test]
    fn the_sweep_covers_the_private_24_without_self() {
        let t = sweep_targets(Ipv4Addr::new(10, 113, 0, 2));
        assert_eq!(t.len(), 253);
        assert!(t.iter().all(|a| a.port() == DISCOVERY_PORT));
        assert!(t.contains(&"10.113.0.4:18110".parse().unwrap()));
        assert!(!t.contains(&"10.113.0.2:18110".parse().unwrap()));
        assert!(!t.contains(&"10.113.0.0:18110".parse().unwrap()));
        assert!(!t.contains(&"10.113.0.255:18110".parse().unwrap()));
        assert_eq!(sweep_targets(Ipv4Addr::new(192, 168, 1, 77)).len(), 253);
        // never sweep public networks
        assert!(sweep_targets(Ipv4Addr::new(203, 0, 113, 5)).is_empty());
        assert!(sweep_targets(Ipv4Addr::new(8, 8, 8, 8)).is_empty());
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
