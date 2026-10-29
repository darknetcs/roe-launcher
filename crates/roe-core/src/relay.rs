//! LAN play without SSH: a launcher-to-launcher relay.
//!
//! ROE-LIVE's services only listen on 127.0.0.1, and the game is told to send its match traffic to 127.0.0.1, so a
//! second computer can't simply connect to `host:8001`. Upstream solves that with an SSH tunnel (`--lan-tunnel` on
//! the host, `--remote-peer` on the player). The relay does SSH's part itself:
//! - **host**: the server runs with `--lan-tunnel` (upstream's UDP↔TCP bridge on 127.0.0.1:19001); `HostRelay`
//!   forwards `HOST_GAME_PORT` → 127.0.0.1:8001 and `HOST_TUNNEL_PORT` → 127.0.0.1:19001, for private addresses only.
//! - **player**: `JoinRelay` listens on 127.0.0.1:8001 and :19001, forwards both to the host, and runs upstream's
//!   bridge in client mode (UDP 127.0.0.1:18001 ↔ TCP 19001); the game then starts with `-server=127.0.0.1:8001`.
//!
//! Hamachi and Radmin VPN networks count as LAN too (`VPN_NETWORKS`), so a player can join a host over them; their
//! hosts are found by address ("Join a host by address" in the Server Browser), since the network search only sweeps
//! the computer's main network.
//!
//! Plain `std::net` TCP, the same on Linux and Windows. Verified first by hand over SSH on 2026-10-20 (Windows host,
//! Linux player, a full Training match).
use crate::process::{self, Handle};
use crate::Sink;
use anyhow::{bail, Context, Result};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The game's login/lobby port (ROE-LIVE's proxy).
pub const GAME_PORT: u16 = 8001;
/// Upstream's bridge port (`lan_tunnel_2022.py`, TCP on loopback).
pub const TUNNEL_PORT: u16 = 19001;
/// The host's LAN ports for the two forwards (not used by the game or the server).
pub const HOST_GAME_PORT: u16 = 28001;
pub const HOST_TUNNEL_PORT: u16 = 29001;

/// Forwards every TCP connection on `listen` to `target` until dropped. Connections from addresses `allow` refuses
/// are closed at once.
pub struct Forwarder {
    stop: Arc<AtomicBool>,
    pub local: SocketAddr,
    open: Arc<Mutex<Vec<TcpStream>>>,
}

impl Forwarder {
    pub fn start(listen: SocketAddr, target: SocketAddr, allow: fn(IpAddr) -> bool) -> Result<Forwarder> {
        let listener = TcpListener::bind(listen).with_context(|| format!("could not listen on TCP {listen} (already in use?)"))?;
        listener.set_nonblocking(true)?;
        let local = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let open: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
        let (flag, streams) = (stop.clone(), open.clone());
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((s, peer)) if allow(peer.ip()) => {
                        let streams = streams.clone();
                        std::thread::spawn(move || {
                            let _ = pipe(s, target, &streams);
                        });
                    }
                    Ok(_) => {} // dropped: not allowed
                    Err(_) => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        });
        Ok(Forwarder { stop, local, open })
    }
}

impl Drop for Forwarder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for s in self.open.lock().unwrap().drain(..) {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

/// Copies both ways between `a` and a new connection to `target` until either side closes.
fn pipe(a: TcpStream, target: SocketAddr, open: &Mutex<Vec<TcpStream>>) -> io::Result<()> {
    a.set_nonblocking(false)?;
    let b = TcpStream::connect_timeout(&target, Duration::from_secs(5))?;
    let _ = (a.set_nodelay(true), b.set_nodelay(true));
    open.lock().unwrap().extend([a.try_clone()?, b.try_clone()?]);
    let (mut a_read, mut b_write) = (a.try_clone()?, b.try_clone()?);
    let back = std::thread::spawn(move || {
        let _ = io::copy(&mut a_read, &mut b_write);
        let _ = b_write.shutdown(Shutdown::Write);
    });
    let (mut b_read, mut a_write) = (b, a);
    let _ = io::copy(&mut b_read, &mut a_write);
    let _ = a_write.shutdown(Shutdown::Write);
    let _ = back.join();
    Ok(())
}

/// Gaming VPNs that act as a LAN: Hamachi (25.0.0.0/8) and Radmin VPN (26.0.0.0/8). Formally public ranges these
/// programs borrow, so they're accepted only because the user switched hosting on and shares the VPN with the player.
pub const VPN_NETWORKS: [(Ipv4Addr, u8); 2] = [(Ipv4Addr::new(25, 0, 0, 0), 8), (Ipv4Addr::new(26, 0, 0, 0), 8)];

fn in_vpn(v: Ipv4Addr) -> bool {
    VPN_NETWORKS.iter().any(|&(net, bits)| u32::from(v) >> (32 - bits) == u32::from(net) >> (32 - bits))
}

/// Computers on the same network: private, link-local or loopback IPv4, a Hamachi / Radmin VPN address, or local IPv6.
pub fn lan_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local() || in_vpn(v),
        IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xffc0) == 0xfe80 || (v.segments()[0] & 0xfe00) == 0xfc00,
    }
}

fn loopback_only(ip: IpAddr) -> bool {
    ip.is_loopback()
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

/// The host's side: forwards its LAN ports to the server, which must run with `--lan-tunnel`.
pub struct HostRelay {
    _game: Forwarder,
    _tunnel: Forwarder,
}

impl HostRelay {
    pub fn start() -> Result<HostRelay> {
        let any = |port| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
        Ok(HostRelay {
            _game: Forwarder::start(any(HOST_GAME_PORT), loopback(GAME_PORT), lan_peer)?,
            _tunnel: Forwarder::start(any(HOST_TUNNEL_PORT), loopback(TUNNEL_PORT), lan_peer)?,
        })
    }

    /// What a host announces (game port, tunnel port), so a player knows where to connect.
    pub const PORTS: [u16; 2] = [HOST_GAME_PORT, HOST_TUNNEL_PORT];
}

/// The player's side: local forwards to the host plus upstream's bridge client. Dropping it stops everything.
pub struct JoinRelay {
    _game: Forwarder,
    _tunnel: Forwarder,
    bridge: Option<Handle>,
}

impl JoinRelay {
    /// `host` = the host's IP, `ports` = its announced [game, tunnel] ports. `python`/`server_dir` run upstream's
    /// `tools/lan_tunnel_2022.py client`; `duration` is its time limit in seconds.
    /// `profile`: the host's server profile (its announcement says which); the server's tunnel refuses another one.
    pub fn start(host: IpAddr, ports: [u16; 2], profile: &str, python: &Path, server_dir: &Path, duration: u32, sink: Sink) -> Result<JoinRelay> {
        let bridge_script = server_dir.join("tools/lan_tunnel_2022.py");
        if !bridge_script.is_file() {
            bail!("the server files have no tools/lan_tunnel_2022.py (update the server on the Updates page)");
        }
        let game = Forwarder::start(loopback(GAME_PORT), SocketAddr::new(host, ports[0]), loopback_only)?;
        let tunnel = Forwarder::start(loopback(TUNNEL_PORT), SocketAddr::new(host, ports[1]), loopback_only)?;
        let mut c = Command::new(python);
        c.args(["-B", "-u"]).arg(&bridge_script).args(["client", "--profile", profile, "--duration"]);
        c.arg(duration.to_string()).current_dir(server_dir);
        let bridge = process::spawn_owned(c, sink).context("starting the match-traffic bridge")?;
        Ok(JoinRelay { _game: game, _tunnel: tunnel, bridge: Some(bridge) })
    }

    pub fn bridge_running(&self) -> bool {
        self.bridge.as_ref().is_some_and(|b| b.is_running())
    }
}

impl Drop for JoinRelay {
    fn drop(&mut self) {
        if let Some(b) = self.bridge.take() {
            b.request_stop(Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn a_forwarder_carries_data_both_ways() {
        // an echo server stands in for the game server
        let echo = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = echo.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in echo.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut s = s;
                    let mut buf = [0u8; 64];
                    while let Ok(n) = s.read(&mut buf) {
                        if n == 0 || s.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                });
            }
        });
        let f = Forwarder::start("127.0.0.1:0".parse().unwrap(), target, lan_peer).unwrap();
        for _ in 0..2 {
            let mut c = TcpStream::connect(f.local).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            c.write_all(b"hello relay").unwrap();
            let mut got = [0u8; 11];
            c.read_exact(&mut got).unwrap();
            assert_eq!(&got, b"hello relay");
        }
    }

    #[test]
    fn refused_peers_are_closed_at_once() {
        let target = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let f = Forwarder::start("127.0.0.1:0".parse().unwrap(), target, |_| false).unwrap();
        let mut c = TcpStream::connect(f.local).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut buf = [0u8; 4];
        // closed without any data
        assert!(matches!(c.read(&mut buf), Ok(0) | Err(_)));
    }

    #[test]
    fn only_lan_addresses_may_use_a_host() {
        for ok in ["10.113.0.2", "192.168.1.5", "172.16.0.9", "127.0.0.1", "169.254.1.1"] {
            assert!(lan_peer(ok.parse().unwrap()), "{ok}");
        }
        for bad in ["8.8.8.8", "203.0.113.5", "172.32.0.1"] {
            assert!(!lan_peer(bad.parse().unwrap()), "{bad}");
        }
        // Hamachi and Radmin VPN, but not their neighbours or Tailscale's CGNAT range
        for ok in ["25.12.34.56", "25.0.0.1", "26.1.2.3", "26.255.255.254"] {
            assert!(lan_peer(ok.parse().unwrap()), "{ok}");
        }
        for bad in ["24.255.255.255", "27.0.0.1", "100.101.102.103"] {
            assert!(!lan_peer(bad.parse().unwrap()), "{bad}");
        }
    }
}
