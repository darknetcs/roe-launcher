//! The match switches of RoE-Rivival's own launcher (`RoE-Launcher.ps1`): bots, storm and its pace, half health,
//! vehicles, and the reset of the game's settings. They read and write the same option values as Server Settings →
//! Match options (flag -> value, see `options`), so both always agree.
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The most bots the server takes (`--shared-bots 0..64`); its own launcher offers up to 48.
pub const BOTS_MAX: u32 = 64;
/// `--cycle-scale` choices of its launcher, with how long one storm calendar then takes.
pub const PACES: [(u32, &str); 5] =
    [(1, "Retail (~32 min)"), (2, "Slow ×2 (~64 min)"), (3, "Slow ×3 (~97 min)"), (5, "Slow ×5 (~2.7 h)"), (10, "Slow ×10 (~5.4 h)")];
/// One storm calendar at retail pace, in minutes (its launcher's "~32 min"). The helicopter comes at 3/4 of it
/// even without a storm (`--evac-delay` default).
const CALENDAR_MINUTES: u32 = 32;

type Values = BTreeMap<String, String>;

fn on(v: &Values, flag: &str) -> bool {
    v.get(flag).is_some_and(|x| x == "true")
}

fn set_on(v: &mut Values, flag: &str, yes: bool) {
    if yes {
        v.insert(flag.into(), "true".into());
    } else {
        v.remove(flag);
    }
}

/// Shared bots in the match; 0 = none.
pub fn bots(v: &Values) -> u32 {
    v.get("--shared-bots").and_then(|n| n.trim().parse().ok()).unwrap_or(0)
}

/// The server refuses bots without its room service, so switching them on switches that on too.
pub fn set_bots(v: &mut Values, n: u32) {
    if n == 0 {
        v.remove("--shared-bots");
    } else {
        v.insert("--shared-bots".into(), n.min(BOTS_MAX).to_string());
        set_on(v, "--room-service", true);
    }
}

/// The shrinking zone (the storm on Dione, ash on Europa).
pub fn storm(v: &Values) -> bool {
    !on(v, "--no-storm")
}

pub fn set_storm(v: &mut Values, yes: bool) {
    set_on(v, "--no-storm", !yes);
}

/// `--cycle-scale`: 1 = retail.
pub fn pace(v: &Values) -> u32 {
    v.get("--cycle-scale").and_then(|n| n.trim().parse().ok()).filter(|n| (1..=10).contains(n)).unwrap_or(1)
}

pub fn set_pace(v: &mut Values, scale: u32) {
    if scale <= 1 {
        v.remove("--cycle-scale");
    } else {
        v.insert("--cycle-scale".into(), scale.min(10).to_string());
    }
}

/// Everyone keeps at least half their health (`--hp-floor 500`): for testing.
pub fn half_health(v: &Values) -> bool {
    v.get("--hp-floor").is_some_and(|x| x.trim() == "500")
}

pub fn set_half_health(v: &mut Values, yes: bool) {
    if yes {
        v.insert("--hp-floor".into(), "500".into());
    } else {
        v.remove("--hp-floor");
    }
}

/// Cars, and boats on the lake.
pub fn fleet(v: &Values) -> bool {
    on(v, "--vehicle-fleet")
}

pub fn set_fleet(v: &mut Values, yes: bool) {
    set_on(v, "--vehicle-fleet", yes);
}

/// Seconds the server should run so one whole match fits: the calendar at this pace plus 10 minutes for the lobby,
/// landing and evacuation.
pub fn match_seconds(v: &Values) -> u32 {
    pace(v) * CALENDAR_MINUTES * 60 + 600
}

/// What its author found corrupts the glider landing (you glide forever or drop through the floor): `ECT_High` in
/// the game's `UserData/CurrentSettings6.xml`.
pub fn settings_poisoned(game_dir: &Path) -> bool {
    fs::read_to_string(game_dir.join("UserData/CurrentSettings6.xml")).is_ok_and(|t| t.contains("ECT_High"))
}

/// Copies the game's `UserData` into `backups/UserData-<unix time>` and removes it, so the game writes clean defaults
/// on its next start (graphics, keys and sound settings start over). Returns the backup, or None when there was
/// nothing to reset. Call it while the game is closed.
pub fn reset_game_settings(game_dir: &Path, backups: &Path) -> Result<Option<PathBuf>> {
    let user = game_dir.join("UserData");
    if !user.is_dir() {
        return Ok(None);
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let backup = backups.join(format!("UserData-{stamp}"));
    copy_dir(&user, &backup).with_context(|| format!("backing up {} to {}", user.display(), backup.display()))?;
    fs::remove_dir_all(&user).with_context(|| format!("removing {} (a game still running?)", user.display()))?;
    Ok(Some(backup))
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            fs::copy(e.path(), &dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switches_write_the_servers_options() {
        let mut v = crate::update::ROE_RIVIVAL.defaults();
        // its own launcher's start: 20 bots, no storm, vehicles
        assert_eq!((bots(&v), storm(&v), fleet(&v), half_health(&v), pace(&v)), (20, false, true, false, 1));
        set_bots(&mut v, 99);
        assert_eq!(bots(&v), BOTS_MAX);
        v.remove("--room-service");
        set_bots(&mut v, 8);
        assert_eq!(v.get("--room-service").map(String::as_str), Some("true"), "bots need the room service");
        set_bots(&mut v, 0);
        assert!(!v.contains_key("--shared-bots"));
        set_storm(&mut v, true);
        assert!(storm(&v) && !v.contains_key("--no-storm"));
        set_pace(&mut v, 5);
        assert_eq!(v.get("--cycle-scale").map(String::as_str), Some("5"));
        assert_eq!(match_seconds(&v), 5 * 32 * 60 + 600);
        set_pace(&mut v, 1);
        assert!(!v.contains_key("--cycle-scale"));
        set_half_health(&mut v, true);
        assert_eq!(v.get("--hp-floor").map(String::as_str), Some("500"));
        set_fleet(&mut v, false);
        assert!(!fleet(&v));
        let args = crate::options::to_args(&v, None).join(" ");
        assert!(args.contains("--hp-floor 500") && !args.contains("--no-storm") && !args.contains("--vehicle-fleet"), "{args}");
    }

    #[test]
    fn reset_keeps_a_backup_and_spots_the_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("game");
        assert_eq!(reset_game_settings(&game, &tmp.path().join("b")).unwrap(), None);
        fs::create_dir_all(game.join("UserData/sub")).unwrap();
        fs::write(game.join("UserData/CurrentSettings6.xml"), "<x eGraphicQuality=\"ECT_High\"/>").unwrap();
        fs::write(game.join("UserData/sub/keys.xml"), "k").unwrap();
        assert!(settings_poisoned(&game));
        let backup = reset_game_settings(&game, &tmp.path().join("b")).unwrap().unwrap();
        assert!(!game.join("UserData").exists() && !settings_poisoned(&game));
        assert_eq!(fs::read_to_string(backup.join("sub/keys.xml")).unwrap(), "k");
        assert!(fs::read_to_string(backup.join("CurrentSettings6.xml")).unwrap().contains("ECT_High"));
    }
}
