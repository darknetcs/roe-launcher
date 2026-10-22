//! Launcher settings, stored as TOML in ~/.config/roe-local/launcher.toml.
//! Also imports the answers saved by the old shell scripts (~/.config/roe-local/config).
use crate::servers::ServerEntry;
use crate::proton::{self, Kind, Runtime};
use crate::util::{config_dir, data_dir};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value")]
pub enum ProtonChoice {
    /// Always use the newest installed runtime of this kind (e.g. newest GE-Proton).
    Latest(Kind),
    /// A specific runtime by folder name.
    Named(String),
    /// Any folder containing a `proton` script.
    Path(PathBuf),
}

/// Which background the launcher shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum Background {
    /// One of the images shipped inside the launcher (1-based).
    Bundled(u8),
    /// A file the user added; the name of the copy in the launcher's own backgrounds folder.
    Custom(String),
    /// Plain gradient.
    None,
}

/// Animated weather drawn over the background.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    #[default]
    Off,
    /// Falling snow (the winter map).
    Snow,
    /// Blowing dust (the desert map).
    Sandstorm,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub preset: String,
    pub accent: [u8; 3],
    /// Text colour; None = the default light text.
    pub text_color: Option<[u8; 3]>,
    pub background: Background,
    pub bg_dim: f32,
    pub panel_opacity: f32,
    pub effect: Effect,
    pub ui_scale: f32,
    /// Old versions had only snow, as a switch. Only read, to migrate it into `effect`.
    #[serde(default, skip_serializing)]
    pub snow: bool,
    /// Old versions stored a path here. Only read, to migrate it into `background`.
    #[serde(default, skip_serializing)]
    pub bg_image: Option<PathBuf>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            preset: "Obsidian".into(),
            accent: [255, 158, 0],
            text_color: None,
            background: Background::Bundled(1),
            bg_dim: 0.55,
            panel_opacity: 0.70,
            effect: Effect::Off,
            ui_scale: 1.0,
            snow: false,
            bg_image: None,
        }
    }
}

/// What the game connects to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "address")]
pub enum Connection {
    /// The server the launcher runs itself (127.0.0.1:8001).
    Local,
    /// A server somewhere else (`host:port`); the launcher does not start a server then.
    Remote(String),
}

impl Connection {
    pub const LOCAL_ADDRESS: &'static str = "127.0.0.1:8001";

    pub fn address(&self) -> String {
        match self {
            Connection::Local => Self::LOCAL_ADDRESS.to_string(),
            Connection::Remote(a) => a.clone(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Connection::Local)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub game_dir: Option<PathBuf>,
    /// The player chose `game_dir` themselves (Browse or typed): Steam's install state is not checked for it,
    /// until Auto-detect hands the choice back to Steam.
    pub game_dir_manual: bool,
    pub proton: ProtonChoice,
    pub compat_data: Option<PathBuf>,
    /// Where the launcher keeps its own copy of the server files.
    pub server_dir: Option<PathBuf>,
    pub check_updates_on_start: bool,
    /// Look for a newer launcher on GitHub when it starts.
    pub check_launcher_updates: bool,
    pub translate_after_update: bool,
    pub stop_server_on_game_exit: bool,
    pub proton_log: bool,
    /// Extra environment for the client: one KEY=VALUE per line.
    pub extra_env: String,
    pub client_extra_args: String,
    pub server_extra_args: String,
    pub server_duration: u32,
    /// Session logs (`<server>/runs/<id>`) to keep; older ones are deleted at start. 0 = keep all.
    pub keep_runs: u32,
    /// Server Settings: match options, flag -> value ("true" = switch on). Missing = server default.
    pub server_options: BTreeMap<String, String>,
    /// Where the game connects. Always `Local` until other servers can be joined.
    pub connection: Connection,
    /// Server Browser: addresses of favourite servers.
    pub favorite_servers: Vec<String>,
    /// Server Browser: servers the user added by hand.
    pub saved_servers: Vec<ServerEntry>,
    /// Server Browser: optional URL of a `{"servers":[...]}` list; empty = off.
    pub server_directory_url: String,
    /// Server Browser: search the local network for other launchers.
    pub lan_search: bool,
    /// Answer those searches while the local server runs (off by default: it opens a UDP port).
    pub lan_visible: bool,
    /// Let other launchers on the network join this computer's server (`relay`); implies being visible. The
    /// server then runs with `--lan-tunnel` and this computer can't play at the same time. Off by default.
    pub lan_host: bool,
    /// Computers asked directly on every Server Browser refresh (IP addresses), e.g. Hamachi or Radmin VPN peers the
    /// network search can't see.
    pub lan_hosts: Vec<String>,
    /// The name other launchers see; empty = a default.
    pub lan_name: String,
    /// Discord Rich Presence ("Playing Ring of Elysium"); off until the player switches it on.
    pub discord_presence: bool,
    /// The in-game name (lobby, map selection, newspaper, stats); empty = upstream's names (the match then says
    /// "LOCAL"). New players start with `playername::DEFAULT`. See `playername`.
    pub player_name: String,
    /// Keep Tencent's crash and telemetry collector in the game folder from starting (see `telemetry`). Off by default.
    pub block_tencent_reporter: bool,
    /// The server profile (`--profile`, see `profiles`): own character and appearance. "default" = the original.
    pub profile: String,
    /// Profiles made in the launcher (the server lists the ones it has saved itself).
    pub profiles: Vec<String>,
    /// Each profile's player name, so switching profiles also switches the name.
    pub profile_names: std::collections::BTreeMap<String, String>,
    /// The map (`maps::MAPS`); anything not selectable falls back to Dione when the server starts.
    pub map_id: u32,
    pub theme: Theme,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            game_dir: None,
            game_dir_manual: false,
            proton: ProtonChoice::Latest(Kind::GeProton),
            compat_data: None,
            server_dir: None,
            check_updates_on_start: true,
            check_launcher_updates: true,
            translate_after_update: true,
            stop_server_on_game_exit: true,
            proton_log: false,
            extra_env: String::new(),
            client_extra_args: String::new(),
            server_extra_args: String::new(),
            server_duration: 1500,
            keep_runs: 20,
            server_options: crate::options::launcher_defaults(),
            connection: Connection::Local,
            favorite_servers: Vec::new(),
            saved_servers: Vec::new(),
            server_directory_url: String::new(),
            lan_search: true,
            lan_visible: false,
            lan_host: false,
            lan_hosts: Vec::new(),
            lan_name: String::new(),
            discord_presence: false,
            player_name: crate::playername::DEFAULT.into(),
            block_tencent_reporter: false,
            profile: crate::profiles::DEFAULT.into(),
            profiles: Vec::new(),
            profile_names: Default::default(),
            map_id: crate::maps::DIONE,
            theme: Theme::default(),
        }
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("launcher.toml")
}

impl Config {
    pub fn load() -> Config {
        Self::load_from(&config_dir())
    }

    pub fn load_from(dir: &Path) -> Config {
        let path = dir.join("launcher.toml");
        if let Ok(text) = fs::read_to_string(&path) {
            match toml::from_str::<Config>(&text) {
                Ok(mut c) => {
                    if std::mem::take(&mut c.theme.snow) && c.theme.effect == Effect::Off {
                        c.theme.effect = Effect::Snow;
                    }
                    return c;
                }
                // unreadable (e.g. edited by hand): keep it, because the defaults are autosaved over it soon
                Err(_) => {
                    let _ = fs::copy(&path, dir.join("launcher.toml.broken"));
                }
            }
        }
        Self::import_legacy(&dir.join("config")).unwrap_or_default()
    }

    /// Reads the KEY=VALUE file written by play-roe.sh / play-server.sh.
    fn import_legacy(path: &Path) -> Option<Config> {
        let text = fs::read_to_string(path).ok()?;
        let mut c = Config::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k.trim() {
                "GAME_DIR" => c.game_dir = Some(PathBuf::from(v.trim())),
                "COMPAT_DATA" => c.compat_data = Some(PathBuf::from(v.trim())),
                "PROTON" => {
                    // the script stores the path of the `proton` file itself
                    let dir = Path::new(v.trim()).parent().map(Path::to_path_buf);
                    if let Some(d) = dir {
                        let name = d.file_name().map(|n| n.to_string_lossy().to_string());
                        c.proton = match name {
                            Some(n) => ProtonChoice::Named(n),
                            None => ProtonChoice::Path(d),
                        };
                    }
                }
                _ => {}
            }
        }
        Some(c)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&config_dir())
    }

    pub fn save_to(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        let text = toml::to_string_pretty(self).context("serialising config")?;
        let tmp = dir.join("launcher.toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, dir.join("launcher.toml"))?;
        Ok(())
    }

    pub fn server_dir_or_default(&self) -> PathBuf {
        self.server_dir.clone().unwrap_or_else(|| data_dir().join("server"))
    }

    /// Turns the saved choice into a concrete runtime (None when it is not installed).
    pub fn resolve_proton(&self, installed: &[Runtime]) -> Option<Runtime> {
        match &self.proton {
            ProtonChoice::Latest(k) => proton::latest_of(installed, *k).cloned(),
            ProtonChoice::Named(n) => installed.iter().find(|r| &r.name == n).cloned(),
            ProtonChoice::Path(p) => {
                let dir = if p.is_file() { p.parent()?.to_path_buf() } else { p.clone() };
                proton::runtime_from_dir(&dir, false)
            }
        }
    }

    /// The Wine prefix: explicit setting, else the game's Steam prefix, else a private one.
    pub fn resolve_prefix(&self, game_dir: &Path) -> PathBuf {
        self.compat_data
            .clone()
            .or_else(|| crate::steam::steam_prefix_for(game_dir))
            .unwrap_or_else(|| data_dir().join("prefix"))
    }

    pub fn env_pairs(&self) -> Vec<(String, String)> {
        parse_env(&self.extra_env)
    }
}

/// `KEY=VALUE` per line; blank lines and `#` comments are ignored.
pub fn parse_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = Config::default();
        c.game_dir = Some("/mnt/x/Ring of Elysium".into());
        c.proton = ProtonChoice::Latest(Kind::CachyOs);
        c.theme.accent = [1, 2, 3];
        c.theme.text_color = Some([20, 20, 20]);
        c.theme.background = Background::Custom("lake-1234abcd.jpg".into());
        c.connection = Connection::Remote("203.0.113.5:8001".into());
        c.extra_env = "PROTON_LOG=1\n# note\nDXVK_HUD=fps".into();
        c.saved_servers = vec![
            ServerEntry { id: "saved-1".into(), name: "Friend".into(), address: "192.168.1.20:8001".into(), tags: vec!["lan".into()], ..Default::default() },
            ServerEntry { id: "saved-2".into(), name: "Bare".into(), address: "h:1".into(), region: Some("EU".into()), ..Default::default() },
        ];
        c.server_directory_url = "https://example.org/servers.json".into();
        c.lan_visible = true;
        c.server_options.insert("--practice-targets".into(), "10".into());
        c.server_options.insert("--live-control".into(), "true".into());
        c.save_to(tmp.path()).unwrap();
        let back = Config::load_from(tmp.path());
        assert_eq!(back, c);
        assert_eq!(back.env_pairs(), vec![("PROTON_LOG".into(), "1".into()), ("DXVK_HUD".into(), "fps".into())]);
    }

    #[test]
    fn imports_the_shell_scripts_saved_answers() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("config"),
            "GAME_DIR=/mnt/games/SteamLibrary/steamapps/common/Ring of Elysium\nPROTON=/home/u/.steam/steam/compatibilitytools.d/GE-Proton11-7/proton\n",
        )
        .unwrap();
        let c = Config::load_from(tmp.path());
        assert_eq!(c.game_dir, Some("/mnt/games/SteamLibrary/steamapps/common/Ring of Elysium".into()));
        assert_eq!(c.proton, ProtonChoice::Named("GE-Proton11-7".into()));
    }

    #[test]
    fn new_defaults_are_obsidian_orange_no_snow_background_one() {
        let t = Theme::default();
        assert_eq!((t.preset.as_str(), t.accent, t.effect), ("Obsidian", [255, 158, 0], Effect::Off));
        assert_eq!(t.background, Background::Bundled(1));
        assert_eq!(t.text_color, None);
        assert!((t.panel_opacity - 0.70).abs() < f32::EPSILON && (t.bg_dim - 0.55).abs() < f32::EPSILON);
        assert!(Config::default().connection.is_local());
        assert_eq!(Config::default().connection.address(), "127.0.0.1:8001");
    }

    #[test]
    fn old_configs_with_a_background_path_still_load() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("launcher.toml"), "[theme]\npreset = \"Aurora\"\nbg_image = \"/old/wall.png\"\n").unwrap();
        let c = Config::load_from(tmp.path());
        assert_eq!(c.theme.preset, "Aurora");
        assert_eq!(c.theme.bg_image, Some(PathBuf::from("/old/wall.png"))); // migrated by the app
        assert_eq!(c.theme.background, Background::Bundled(1));
        // and the legacy field is never written back
        c.save_to(tmp.path()).unwrap();
        assert!(!fs::read_to_string(tmp.path().join("launcher.toml")).unwrap().contains("bg_image"));
    }

    #[test]
    fn old_snow_switch_becomes_the_snow_effect() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("launcher.toml"), "[theme]\nsnow = true\n").unwrap();
        let c = Config::load_from(tmp.path());
        assert_eq!(c.theme.effect, Effect::Snow);
        c.save_to(tmp.path()).unwrap();
        let text = fs::read_to_string(tmp.path().join("launcher.toml")).unwrap();
        assert!(text.contains("effect = \"Snow\"") && !text.contains("snow ="), "{text}");
        assert_eq!(Config::load_from(tmp.path()).theme.effect, Effect::Snow);
    }

    #[test]
    fn a_broken_file_is_kept_before_the_defaults_replace_it() {
        let tmp = tempfile::tempdir().unwrap();
        let broken = "discord_presence = true\ndiscord_presence = false\n"; // a key twice: invalid TOML
        fs::write(tmp.path().join("launcher.toml"), broken).unwrap();
        let c = Config::load_from(tmp.path());
        assert!(!c.discord_presence);
        assert_eq!(fs::read_to_string(tmp.path().join("launcher.toml.broken")).unwrap(), broken);
    }

    #[test]
    fn players_start_as_the_original_server_but_keep_their_own_name() {
        let tmp = tempfile::tempdir().unwrap();
        // a file saved before the setting existed
        fs::write(tmp.path().join("launcher.toml"), "check_updates_on_start = false\n").unwrap();
        assert_eq!(Config::load_from(tmp.path()).player_name, "BRING_BACK_ROE!!");
        fs::write(tmp.path().join("launcher.toml"), "player_name = \"darknetcs\"\n").unwrap();
        assert_eq!(Config::load_from(tmp.path()).player_name, "darknetcs");
        fs::write(tmp.path().join("launcher.toml"), "player_name = \"\"\n").unwrap();
        assert_eq!(Config::load_from(tmp.path()).player_name, "");
    }

    #[test]
    fn unknown_or_partial_files_fall_back_to_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("launcher.toml"), "check_updates_on_start = false\n").unwrap();
        let c = Config::load_from(tmp.path());
        assert!(!c.check_updates_on_start);
        assert_eq!(c.server_duration, 1500);
        assert_eq!(c.keep_runs, 20);
        assert_eq!(c.server_options, crate::options::launcher_defaults());
    }
}
