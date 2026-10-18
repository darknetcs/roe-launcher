//! Builds and supervises the server (windows_launch.py --services-only).
use crate::process::{self, Handle};
use crate::runtime::Prepared;
use crate::util::{bound_udp, listening_tcp, split_args};
use crate::Sink;
use anyhow::{bail, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LOBBY_PORT: u16 = 9001;
pub const PROXY_PORT: u16 = 8001;
pub const UDP_PORT: u16 = 18001;

#[derive(Clone, Debug)]
pub struct Settings {
    pub dir: PathBuf,
    pub game_dir: PathBuf,
    pub duration: u32,
    pub extra_args: String,
    pub lan_tunnel: bool,
    /// Match options from the Server Settings page, already as arguments (see `options::to_args`).
    pub options: Vec<String>,
}

/// Newest `config/windows-smoke-*.json` (the name carries a date and changes upstream).
pub fn find_loot_catalog(dir: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir.join("config"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            n.starts_with("windows-smoke-") && n.ends_with(".json")
        })
        .collect();
    v.sort();
    v.pop()
}

/// What the launcher manages, the match options, then the user's extra arguments last.
pub fn build_args(s: &Settings, dotnet: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["--services-only", "--client"].map(String::from).to_vec();
    a.push(s.game_dir.display().to_string());
    a.extend(["--dotnet".into(), dotnet.display().to_string(), "--duration".into()]);
    a.push(s.duration.clamp(120, 7200).to_string());
    a.extend(s.options.iter().cloned());
    // no catalog chosen: the newest bundled one, as play.cmd does
    if !s.options.iter().any(|x| x == "--loot-catalog") {
        if let Some(c) = find_loot_catalog(&s.dir) {
            a.extend(["--loot-catalog".into(), c.display().to_string()]);
        }
    }
    if s.lan_tunnel {
        a.push("--lan-tunnel".into());
    }
    a.extend(split_args(&s.extra_args).iter().map(|x| cmd_style_switch(x)));
    a
}

/// Upstream's run-roe-2022.cmd takes PowerShell-style switches (`-PracticeTargets 10`), while
/// windows_launch.py wants `--practice-targets 10`. Turns the first into the second; everything else
/// (values, paths, `--options`, `-h`) is returned unchanged.
pub fn cmd_style_switch(arg: &str) -> String {
    let Some(name) = arg.strip_prefix('-') else { return arg.to_string() };
    let looks_like_switch = name.starts_with(|c: char| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_alphanumeric());
    if !looks_like_switch {
        return arg.to_string();
    }
    let cs: Vec<char> = name.chars().collect();
    let mut out = String::from("--");
    for (i, &c) in cs.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            let prev = cs[i - 1];
            let next_lower = cs.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            // a new word: "practiceTargets", "udp2", or the last capital of an acronym before a word ("LOSMode")
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() || (prev.is_ascii_uppercase() && next_lower) {
                out.push('-');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

pub fn build_command(p: &Prepared, s: &Settings) -> Command {
    let mut c = Command::new(&p.python);
    c.args(["-X", "utf8", "-B", "-u"]).arg(s.dir.join("tools/windows_launch.py")).args(build_args(s, &p.dotnet));
    c.current_dir(&s.dir);
    // `:` on Linux, `;` on Windows (where the portable Python reads python312._pth instead anyway)
    let mut paths = vec![s.dir.join("tools"), s.dir.join("server/directory")];
    if let Some(old) = std::env::var_os("PYTHONPATH").filter(|o| !o.is_empty()) {
        paths.extend(std::env::split_paths(&old));
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        c.env("PYTHONPATH", joined);
    }
    c.env("DOTNET_CLI_TELEMETRY_OPTOUT", "1").env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1");
    if let Some(root) = &p.dotnet_root {
        c.env("DOTNET_ROOT", root);
    }
    c
}

pub fn start(p: &Prepared, s: &Settings, sink: Sink) -> Result<Handle> {
    if !crate::game::has_world_data(&s.game_dir) {
        bail!("the game folder has no {} — is the game fully downloaded?", crate::game::WORLD_DATA);
    }
    if listening_tcp(PROXY_PORT) {
        bail!("port {PROXY_PORT} is already in use (another server running?)");
    }
    process::spawn_logged(build_command(p, s), sink)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ports {
    pub lobby: bool,
    pub proxy: bool,
    pub udp: bool,
}

impl Ports {
    pub fn all_up(&self) -> bool {
        self.lobby && self.proxy && self.udp
    }
}

pub fn ports() -> Ports {
    Ports { lobby: listening_tcp(LOBBY_PORT), proxy: listening_tcp(PROXY_PORT), udp: bound_udp(UDP_PORT) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_run_roe_cmd_style_switches() {
        for (cmd, py) in [
            ("-PracticeTargets", "--practice-targets"),
            ("-LiveControl", "--live-control"),
            ("-ReliableUDP", "--reliable-udp"),
            ("-TargetLOS", "--target-los"),
            ("-CycleScale", "--cycle-scale"),
            ("-Kit", "--kit"),
        ] {
            assert_eq!(cmd_style_switch(cmd), py);
        }
        for same in ["--practice-targets", "10", "-h", "-5", r"C:\x\loot.json", "/tmp/a", "-Odd-Name"] {
            assert_eq!(cmd_style_switch(same), same);
        }
        let s = Settings { dir: "/s".into(), game_dir: "/g".into(), duration: 1500, extra_args: "-PracticeTargets 10 -LiveControl".into(), lan_tunnel: false, options: vec![] };
        assert!(build_args(&s, Path::new("dotnet")).join(" ").ends_with("--practice-targets 10 --live-control"));
    }

    #[test]
    fn args_match_play_server_sh_and_pick_the_newest_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("config")).unwrap();
        for n in ["windows-smoke-20261015.json", "windows-smoke-20261101.json", "world.json"] {
            fs::write(tmp.path().join("config").join(n), "{}").unwrap();
        }
        let s = Settings {
            dir: tmp.path().to_path_buf(),
            game_dir: "/g/Ring of Elysium".into(),
            duration: 99999,
            extra_args: "--foo 'a b'".into(),
            lan_tunnel: true,
            options: crate::options::to_args(&crate::options::launcher_defaults(), None),
        };
        let a = build_args(&s, Path::new("/usr/bin/dotnet"));
        let joined = a.join(" ");
        assert!(joined.starts_with("--services-only --client /g/Ring of Elysium --dotnet /usr/bin/dotnet --duration 7200"));
        for part in ["--kit glider", "--mode cycle", "--cycle-scale 3", "--reliable-udp", "--idle-handoff", "--winter-doors", "--winter-windows", "--target armored"] {
            assert!(joined.contains(part), "{part}");
        }
        assert!(a.iter().any(|x| x.ends_with("windows-smoke-20261101.json")));
        // a catalog the user picked replaces the automatic one
        let mine = Settings { options: vec!["--loot-catalog".into(), "/my.json".into()], ..s.clone() };
        let a = build_args(&mine, Path::new("dotnet"));
        assert!(a.contains(&"/my.json".to_string()) && !a.iter().any(|x| x.ends_with("windows-smoke-20261101.json")));
        assert!(a.contains(&"--lan-tunnel".to_string()));
        assert_eq!(&a[a.len() - 2..], ["--foo", "a b"]);
    }
}
