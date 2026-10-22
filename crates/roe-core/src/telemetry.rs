//! Tencent's crash and telemetry collector in the game folder (TQM64\TQMCenter_64.exe, "Bugly/TQM"): the client
//! starts it, and it can upload crash dumps and system details to Tencent (gamer.qq.com/TQM, pc.bugly.qcloud.com).
//! The game doesn't need it. Blocking keeps the file as it is and only stops it from starting:
//! - Windows: a "deny execute" permission for the current user (icacls), as MLVP's telemetry switch does;
//! - Linux: no read permission, because Wine ignores the executable flag of Windows programs but can't load a file it
//!   can't read. Names and sizes stay the same, so the game file check is not affected.
#[cfg(windows)]
use anyhow::bail;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Both copies: the DX9 client's (the one the launcher runs) and the DX12 client's.
pub const COLLECTORS: [&str; 2] = ["TQM64/TQMCenter_64.exe", "XVersion/TQM64/TQMCenter_64.exe"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// No collector in this game folder.
    NotFound,
    Allowed,
    Blocked,
    /// One copy blocked, the other not (e.g. Steam restored one).
    Mixed,
}

fn present(game_dir: &Path) -> Vec<PathBuf> {
    COLLECTORS.iter().map(|c| game_dir.join(c)).filter(|p| p.is_file()).collect()
}

pub fn state(game_dir: &Path) -> State {
    let files = present(game_dir);
    let blocked = files.iter().filter(|p| is_blocked(p)).count();
    match (files.len(), blocked) {
        (0, _) => State::NotFound,
        (n, b) if b == n => State::Blocked,
        (_, 0) => State::Allowed,
        _ => State::Mixed,
    }
}

/// Blocks (or allows again) every copy. Returns how many were changed.
pub fn set_blocked(game_dir: &Path, block: bool) -> Result<usize> {
    let mut changed = 0;
    for p in present(game_dir) {
        if is_blocked(&p) != block {
            set(&p, block)?;
            changed += 1;
        }
    }
    Ok(changed)
}

#[cfg(unix)]
fn is_blocked(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.permissions().mode() & 0o444 == 0).unwrap_or(false)
}

#[cfg(unix)]
fn set(p: &Path, block: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(p)?.permissions().mode();
    let new = if block { mode & !0o444 } else { mode | 0o444 };
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(new))
        .map_err(|e| anyhow::anyhow!("could not change the permissions of {}: {e}", p.display()))
}

#[cfg(windows)]
fn icacls(args: &[&std::ffi::OsStr]) -> Result<String> {
    let mut cmd = std::process::Command::new("icacls");
    cmd.args(args);
    crate::process::hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| anyhow::anyhow!("could not run icacls: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        bail!("{}", text.trim());
    }
    Ok(text)
}

#[cfg(windows)]
fn user() -> String {
    match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(d), Ok(u)) if !d.is_empty() => format!("{d}\\{u}"),
        (_, Ok(u)) => u,
        _ => String::new(),
    }
}

#[cfg(windows)]
fn is_blocked(p: &Path) -> bool {
    // icacls lists entries like "PC\user:(DENY)(X)"; those markers aren't translated
    icacls(&[p.as_os_str()]).map(|t| t.lines().any(|l| l.contains("(DENY)") && l.contains("(X)"))).unwrap_or(false)
}

#[cfg(windows)]
fn set(p: &Path, block: bool) -> Result<()> {
    let user = user();
    if user.is_empty() {
        bail!("could not tell the current Windows user");
    }
    let grant = format!("{user}:(X)");
    let args: Vec<&std::ffi::OsStr> = if block {
        vec![p.as_os_str(), "/deny".as_ref(), grant.as_ref()]
    } else {
        vec![p.as_os_str(), "/remove:d".as_ref(), user.as_ref()]
    };
    icacls(&args).map(|_| ()).map_err(|e| anyhow::anyhow!("Windows refused to change {} ({e})", p.display()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn blocks_and_allows_both_copies_without_touching_the_files() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(state(tmp.path()), State::NotFound);
        for c in COLLECTORS {
            let p = tmp.path().join(c);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, b"MZ tqm").unwrap();
        }
        assert_eq!(state(tmp.path()), State::Allowed);
        assert_eq!(set_blocked(tmp.path(), true).unwrap(), 2);
        assert_eq!(state(tmp.path()), State::Blocked);
        assert_eq!(set_blocked(tmp.path(), true).unwrap(), 0); // already blocked
        let first = tmp.path().join(COLLECTORS[0]);
        assert_eq!(fs::metadata(&first).unwrap().len(), 6); // size kept: the game file check stays green
        set(&first, false).unwrap();
        assert_eq!(state(tmp.path()), State::Mixed);
        assert_eq!(set_blocked(tmp.path(), false).unwrap(), 1);
        assert_eq!(state(tmp.path()), State::Allowed);
        assert_eq!(fs::read(&first).unwrap(), b"MZ tqm");
    }
}
