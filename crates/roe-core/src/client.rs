//! Builds the command that starts Europa_Client.exe against the local server:
//! through Proton on Linux, directly on Windows.
use crate::process::{self, Handle};
use crate::util::split_args;
use crate::Sink;
use anyhow::{bail, Result};
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct Settings {
    pub game_dir: PathBuf,
    /// The `proton` script (unused on Windows).
    pub proton: PathBuf,
    /// The Wine prefix (unused on Windows).
    pub prefix: PathBuf,
    pub steam_root: Option<PathBuf>,
    pub server_addr: String,
    pub extra_args: String,
    pub extra_env: Vec<(String, String)>,
    pub proton_log: bool,
    /// `waitforexitandrun` (blocks until the prefix is empty, like Steam) or `run`.
    pub verb: String,
}

impl Settings {
    pub fn new(game_dir: PathBuf, proton: PathBuf, prefix: PathBuf) -> Self {
        Self {
            game_dir,
            proton,
            prefix,
            steam_root: crate::steam::steam_roots().into_iter().next(),
            server_addr: "127.0.0.1:8001".into(),
            extra_args: String::new(),
            extra_env: vec![],
            proton_log: false,
            verb: "waitforexitandrun".into(),
        }
    }
}

pub fn game_args(s: &Settings) -> Vec<String> {
    let mut a: Vec<String> = vec!["-language=en".into(), "-garena".into(), "-uid=1".into(), format!("-server={}", s.server_addr)];
    a.extend(split_args(&s.extra_args));
    a
}

/// The command for this platform.
pub fn build_command(s: &Settings) -> Command {
    if crate::NEEDS_PROTON {
        proton_command(s)
    } else {
        direct_command(s)
    }
}

/// Windows: the game itself, started inside its folder.
pub fn direct_command(s: &Settings) -> Command {
    let mut c = Command::new(s.game_dir.join("Europa_Client.exe"));
    c.args(game_args(s)).current_dir(&s.game_dir);
    for (k, v) in &s.extra_env {
        c.env(k, v);
    }
    c
}

/// Linux: `proton waitforexitandrun ./Europa_Client.exe ...` with the Steam compat variables.
pub fn proton_command(s: &Settings) -> Command {
    let mut c = Command::new(&s.proton);
    c.arg(&s.verb).arg("./Europa_Client.exe").args(game_args(s)).current_dir(&s.game_dir);
    let root = s.steam_root.clone().or_else(|| dirs::home_dir().map(|h| h.join(".steam/steam")));
    if let Some(r) = root {
        c.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", r);
    }
    c.env("STEAM_COMPAT_DATA_PATH", &s.prefix).env("PROTON_USE_XALIA", "0");
    if s.proton_log {
        c.env("PROTON_LOG", "1");
    }
    for (k, v) in &s.extra_env {
        c.env(k, v);
    }
    c
}

pub fn start(s: &Settings, sink: Sink) -> Result<Handle> {
    if crate::NEEDS_PROTON {
        if !s.proton.is_file() {
            bail!("Proton not found: {}", s.proton.display());
        }
        std::fs::create_dir_all(&s.prefix)?;
    } else if !crate::game::is_game_dir(&s.game_dir) {
        bail!("Europa_Client.exe not found in {}", s.game_dir.display());
    }
    process::spawn_logged(build_command(s), sink)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn command_matches_the_working_manual_invocation() {
        let mut s = Settings::new("/g/Ring of Elysium".into(), "/p/GE-Proton11-7/proton".into(), "/pfx/755790".into());
        s.steam_root = Some("/home/u/.steam/steam".into());
        s.extra_env = vec![("DXVK_HUD".into(), "fps".into())];
        s.extra_args = "-windowed".into();
        let c = proton_command(&s);
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(args, ["waitforexitandrun", "./Europa_Client.exe", "-language=en", "-garena", "-uid=1", "-server=127.0.0.1:8001", "-windowed"]);
        assert_eq!(c.get_current_dir().unwrap().to_str(), Some("/g/Ring of Elysium"));
        let env: std::collections::HashMap<_, _> = c
            .get_envs()
            .filter_map(|(k, v)| Some((k.to_string_lossy().to_string(), v?.to_string_lossy().to_string())))
            .collect();
        assert_eq!(env["STEAM_COMPAT_DATA_PATH"], "/pfx/755790");
        assert_eq!(env["STEAM_COMPAT_CLIENT_INSTALL_PATH"], "/home/u/.steam/steam");
        assert_eq!(env["PROTON_USE_XALIA"], "0");
        assert_eq!(env["DXVK_HUD"], "fps");
        assert!(!env.contains_key("PROTON_LOG"));
    }

    #[test]
    fn windows_starts_the_game_directly() {
        let mut s = Settings::new("/g/Ring of Elysium".into(), "".into(), "".into());
        s.extra_env = vec![("DXVK_HUD".into(), "fps".into())];
        let c = direct_command(&s);
        assert_eq!(Path::new(c.get_program()), Path::new("/g/Ring of Elysium/Europa_Client.exe"));
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(args, ["-language=en", "-garena", "-uid=1", "-server=127.0.0.1:8001"]);
        assert_eq!(c.get_current_dir().unwrap().to_str(), Some("/g/Ring of Elysium"));
        assert!(c.get_envs().all(|(k, _)| !k.to_string_lossy().starts_with("STEAM_COMPAT")));
    }
}
