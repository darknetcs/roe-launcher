use std::path::PathBuf;

/// Splits "GE-Proton11-7" into [11, 7] so versions sort numerically.
pub fn version_key(s: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            out.push(cur.parse().unwrap_or(0));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.parse().unwrap_or(0));
    }
    out
}

#[cfg(target_os = "linux")]
fn proc_has_port(files: &[&str], state: Option<&str>, port: u16) -> bool {
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        for line in text.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 4 {
                continue;
            }
            if let Some(want) = state {
                if cols[3] != want {
                    continue;
                }
            }
            if let Some(p) = cols[1].rsplit(':').next() {
                if u16::from_str_radix(p, 16) == Ok(port) {
                    return true;
                }
            }
        }
    }
    false
}

/// True when something is *listening* on the TCP port. Never connects (connecting would show
/// up as a client in the lobby). Linux reads /proc.
#[cfg(target_os = "linux")]
pub fn listening_tcp(port: u16) -> bool {
    proc_has_port(&["/proc/net/tcp", "/proc/net/tcp6"], Some("0A"), port)
}

/// True when a UDP socket is bound to the port.
#[cfg(target_os = "linux")]
pub fn bound_udp(port: u16) -> bool {
    proc_has_port(&["/proc/net/udp", "/proc/net/udp6"], None, port)
}

/// Elsewhere: the port is taken when binding it on 127.0.0.1 fails (what windows_launch.py checks
/// too). The test socket is closed at once; it never connects.
#[cfg(not(target_os = "linux"))]
pub fn listening_tcp(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

#[cfg(not(target_os = "linux"))]
pub fn bound_udp(port: u16) -> bool {
    std::net::UdpSocket::bind(("127.0.0.1", port)).is_err()
}

/// `canonicalize` on Windows returns `\\?\C:\...`, which games and scripts often reject.
/// Turns it back into `C:\...` (UNC shares `\\?\UNC\...` are kept as they are).
pub fn plain_path(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p,
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("roe-local")
}

/// `~/.local/share/roe-local` on Linux; `%LOCALAPPDATA%\roe-local` on Windows (not the roaming
/// profile: the server files with Python and .NET are over 1 GB).
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("roe-local")
}

/// Strips quotes, `\ ` escapes and expands `~` (what a terminal drag-and-drop produces).
pub fn clean_input(s: &str) -> String {
    let mut p = s.trim().to_string();
    for q in ['\'', '"'] {
        if p.len() >= 2 && p.starts_with(q) && p.ends_with(q) {
            p = p[1..p.len() - 1].to_string();
        }
    }
    // a terminal escapes spaces as `\ `; on Windows a backslash is the path separator
    if cfg!(not(windows)) {
        p = p.replace("\\ ", " ");
    }
    if p == "~" || p.starts_with("~/") {
        if let Some(h) = dirs::home_dir() {
            p = format!("{}{}", h.display(), &p[1..]);
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_keys_sort_numerically() {
        assert!(version_key("GE-Proton11-7") > version_key("GE-Proton11-3"));
        assert!(version_key("GE-Proton11-7") > version_key("GE-Proton10-34"));
        assert_eq!(version_key("proton-cachyos-11.0-20261005-slr"), vec![11, 0, 20261005]);
    }

    #[test]
    fn clean_input_handles_drag_and_drop() {
        assert_eq!(clean_input("'/a/b c'"), "/a/b c");
        if cfg!(windows) {
            assert_eq!(clean_input(r"C:\Games\ Ring"), r"C:\Games\ Ring"); // backslashes are separators there
        } else {
            assert_eq!(clean_input("/a/My\\ Game"), "/a/My Game");
        }
        assert_eq!(clean_input("  \"/x\"  "), "/x");
    }
}

/// Splits a command-line string into arguments, honouring single and double quotes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'') | (None, '"') => {
                quote = Some(c);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod split_tests {
    use super::split_args;
    #[test]
    fn splits_with_quotes() {
        assert_eq!(split_args("--a 1 --b 'x y' \"z w\""), ["--a", "1", "--b", "x y", "z w"]);
        assert!(split_args("   ").is_empty());
    }
}
