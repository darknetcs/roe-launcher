//! Model and logic behind the **Server Browser** tab.
//!
//! It defines:
//! * the data model of a server entry and where it came from (`Source`),
//! * a `ServerSource` trait so different directories (a hosted list, a LAN scan, the local
//!   server) can be plugged in without touching the UI,
//! * merging, filtering / sorting / favourites logic, and address validation,
//! * a parser for a simple JSON directory format (`{"servers":[...]}`), which is a proposal
//!   to be settled once a real directory service exists.
//!
//! Only the local server can be joined today: the community server listens on 127.0.0.1 only.
//! The launcher already routes the game through `config::Connection`, so joining another
//! server later only means setting `Connection::Remote("host:port")` (see `can_join`).
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Where an entry came from. Never read from or written to files: each source sets it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// The server the launcher runs on this computer.
    Local,
    /// Added by the user (kept in the launcher settings).
    #[default]
    Saved,
    /// Answered a search on the local network (see `lan`).
    Lan,
    /// Listed by a JSON directory URL.
    Directory,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Local => "This computer",
            Source::Saved => "Saved",
            Source::Lan => "LAN",
            Source::Directory => "Directory",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerEntry {
    /// Stable id (used for favourites and diffing between refreshes).
    pub id: String,
    pub name: String,
    /// `host:port` the game connects to.
    pub address: String,
    pub region: Option<String>,
    pub players: u32,
    pub max_players: u32,
    pub ping_ms: Option<u32>,
    /// Server software / protocol version.
    pub version: Option<String>,
    pub mode: Option<String>,
    pub tags: Vec<String>,
    pub official: bool,
    pub password: bool,
    /// A LAN host that lets others join: its [game, tunnel] relay ports (`relay`). Not saved.
    #[serde(skip)]
    pub relay: Option<[u16; 2]>,
    /// A LAN host's server profile (see `profiles`), for the bridge when joining. Not saved.
    #[serde(skip)]
    pub profile: Option<String>,
    #[serde(skip)]
    pub source: Source,
}

impl Default for ServerEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            address: String::new(),
            region: None,
            players: 0,
            max_players: 0,
            ping_ms: None,
            version: None,
            mode: None,
            tags: vec![],
            official: false,
            password: false,
            relay: None,
            profile: None,
            source: Source::Saved,
        }
    }
}

impl ServerEntry {
    pub fn is_full(&self) -> bool {
        self.max_players > 0 && self.players >= self.max_players
    }
}

/// Something that can list servers. Implementations block, so call them from a worker thread.
pub trait ServerSource: Send + Sync {
    fn name(&self) -> &str;
    fn fetch(&self) -> Result<Vec<ServerEntry>>;
}

/// The server the launcher runs on this computer.
pub fn local_entry() -> ServerEntry {
    ServerEntry {
        id: "local".into(),
        name: "Local server (this computer)".into(),
        address: crate::config::Connection::LOCAL_ADDRESS.into(),
        region: Some("LAN".into()),
        mode: Some("Training".into()),
        source: Source::Local,
        ..Default::default()
    }
}

/// Why an entry cannot be joined yet, or `Ok` when it can.
pub fn can_join(e: &ServerEntry) -> Result<(), &'static str> {
    match e.source {
        Source::Local => Ok(()),
        Source::Lan if e.relay.is_some() => Ok(()),
        Source::Lan => Err("This computer's launcher doesn't let others join (it needs \"Let others on my network join\" and launcher v0.1.9 or newer)."),
        _ => Err("Joining servers outside your network is not possible yet."),
    }
}

/// Joins the lists in priority order (earlier lists win). Entries with the same address are kept once.
pub fn merge(lists: &[&[ServerEntry]]) -> Vec<ServerEntry> {
    let mut out: Vec<ServerEntry> = Vec::new();
    for e in lists.iter().flat_map(|l| l.iter()) {
        let key = e.address.trim().to_lowercase();
        if !out.iter().any(|o| o.address.trim().to_lowercase() == key) {
            out.push(e.clone());
        }
    }
    out
}

/// A fixed list; used for the local server today and for tests.
pub struct StaticSource {
    pub label: String,
    pub entries: Vec<ServerEntry>,
}

impl ServerSource for StaticSource {
    fn name(&self) -> &str {
        &self.label
    }
    fn fetch(&self) -> Result<Vec<ServerEntry>> {
        Ok(self.entries.clone())
    }
}

/// A hosted directory returning `{"servers":[ServerEntry, ...]}`. Proposal only: no such
/// service exists yet.
pub struct JsonDirectory {
    pub url: String,
}

impl ServerSource for JsonDirectory {
    fn name(&self) -> &str {
        &self.url
    }
    fn fetch(&self) -> Result<Vec<ServerEntry>> {
        let text = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(10))
            .user_agent("roe-launcher")
            .build()
            .get(&self.url)
            .call()
            .map_err(|e| anyhow!("could not reach the server list: {e}"))?
            .into_string()?;
        let mut v = parse_directory(&text)?;
        for e in &mut v {
            e.source = Source::Directory;
            if e.id.is_empty() {
                e.id = format!("dir:{}", e.address);
            }
        }
        Ok(v)
    }
}

pub fn parse_directory(json: &str) -> Result<Vec<ServerEntry>> {
    #[derive(Deserialize)]
    struct Doc {
        servers: Vec<ServerEntry>,
    }
    let doc: Doc = serde_json::from_str(json).map_err(|e| anyhow!("invalid server list: {e}"))?;
    // drop entries the game could never connect to
    Ok(doc.servers.into_iter().filter(|s| validate_address(&s.address).is_ok()).collect())
}

/// Accepts `host:port` (host = name or IPv4; `[v6]:port` too) with a port in 1..=65535.
pub fn validate_address(addr: &str) -> Result<(String, u16)> {
    let addr = addr.trim();
    let (host, port) = if let Some(rest) = addr.strip_prefix('[') {
        let (h, p) = rest.split_once("]:").ok_or_else(|| anyhow!("expected [address]:port"))?;
        (h.to_string(), p)
    } else {
        let (h, p) = addr.rsplit_once(':').ok_or_else(|| anyhow!("expected host:port"))?;
        (h.to_string(), p)
    };
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c == '/') {
        bail!("invalid host");
    }
    let port: u16 = port.parse().map_err(|_| anyhow!("invalid port"))?;
    if port == 0 {
        bail!("invalid port");
    }
    Ok((host, port))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub search: String,
    pub only_favorites: bool,
    pub hide_full: bool,
    pub hide_password: bool,
}

/// Filters by the query, then sorts: favourites first, then more players, then name.
pub fn filter_and_sort(entries: &[ServerEntry], favorites: &[String], q: &Query) -> Vec<ServerEntry> {
    let needle = q.search.trim().to_lowercase();
    let fav = |e: &ServerEntry| favorites.iter().any(|f| f == &e.address || f == &e.id);
    let mut v: Vec<ServerEntry> = entries
        .iter()
        .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle) || e.address.to_lowercase().contains(&needle) || e.tags.iter().any(|t| t.to_lowercase().contains(&needle)))
        .filter(|e| !q.only_favorites || fav(e))
        .filter(|e| !q.hide_full || !e.is_full())
        .filter(|e| !q.hide_password || !e.password)
        .cloned()
        .collect();
    v.sort_by(|a, b| fav(b).cmp(&fav(a)).then(b.players.cmp(&a.players)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, addr: &str, players: u32, max: u32) -> ServerEntry {
        ServerEntry { id: name.into(), name: name.into(), address: addr.into(), players, max_players: max, ..Default::default() }
    }

    #[test]
    fn validates_addresses() {
        assert_eq!(validate_address("127.0.0.1:8001").unwrap(), ("127.0.0.1".into(), 8001));
        assert_eq!(validate_address(" play.example.org:9 ").unwrap().1, 9);
        assert_eq!(validate_address("[::1]:8001").unwrap(), ("::1".into(), 8001));
        for bad in ["", "nohost", ":8001", "a:0", "a:70000", "a b:1", "http://x:1"] {
            assert!(validate_address(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn filters_and_sorts_with_favorites_first() {
        let all = vec![e("Zeta", "z:1", 5, 10), e("Alpha", "a:1", 5, 10), e("Full", "f:1", 10, 10), e("Busy", "b:1", 9, 10)];
        let sorted = filter_and_sort(&all, &["z:1".to_string()], &Query::default());
        assert_eq!(sorted.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Zeta", "Full", "Busy", "Alpha"]);
        let q = Query { hide_full: true, ..Default::default() };
        assert!(filter_and_sort(&all, &[], &q).iter().all(|s| s.name != "Full"));
        let q = Query { search: "ALP".into(), ..Default::default() };
        assert_eq!(filter_and_sort(&all, &[], &q).len(), 1);
        let q = Query { only_favorites: true, ..Default::default() };
        assert_eq!(filter_and_sort(&all, &["b:1".to_string()], &q)[0].name, "Busy");
    }

    #[test]
    fn parses_a_directory_and_drops_unusable_entries() {
        let json = r#"{"servers":[
            {"id":"a","name":"EU #1","address":"eu1.example.org:8001","players":12,"max_players":100,"official":true,"tags":["training"]},
            {"id":"b","name":"broken","address":"not-an-address"}
        ]}"#;
        let v = parse_directory(json).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].name.as_str(), v[0].players, v[0].official), ("EU #1", 12, true));
        assert!(parse_directory("{}").is_err());
    }

    #[test]
    fn local_server_is_a_normal_entry() {
        let l = local_entry();
        assert!(validate_address(&l.address).is_ok());
        let src = StaticSource { label: "local".into(), entries: vec![l.clone()] };
        assert_eq!(src.fetch().unwrap(), vec![l]);
    }

    #[test]
    fn only_the_local_server_can_be_joined() {
        assert!(can_join(&local_entry()).is_ok());
        for source in [Source::Saved, Source::Lan, Source::Directory] {
            let mut x = e("x", "10.0.0.2:8001", 0, 0);
            x.source = source;
            assert!(can_join(&x).is_err(), "{source:?}");
        }
    }

    #[test]
    fn merge_keeps_the_first_entry_per_address() {
        let local = [local_entry()];
        let saved = [e("Mine", "127.0.0.1:8001", 0, 0), e("Friend", "Host.lan:8001", 0, 0)];
        let mut lan = e("Friend (LAN)", "host.lan:8001", 1, 10);
        lan.source = Source::Lan;
        let mut other = e("Other", "10.0.0.9:8001", 0, 0);
        other.source = Source::Lan;
        let v = merge(&[&local, &saved, &[lan, other]]);
        assert_eq!(v.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Local server (this computer)", "Friend", "Other"]);
        assert_eq!(v[0].source, Source::Local);
    }

    #[test]
    fn a_directory_cannot_claim_to_be_this_computer() {
        let v = parse_directory(r#"{"servers":[{"name":"x","address":"127.0.0.1:8001","source":"Local"}]}"#).unwrap();
        assert_eq!(v[0].source, Source::Saved);
        assert!(can_join(&v[0]).is_err());
    }
}
