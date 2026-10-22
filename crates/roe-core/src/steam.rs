//! Finds Steam roots, libraries and the Ring of Elysium install.
use std::fs;
use std::path::{Path, PathBuf};

pub const APPID: u32 = 755790;
pub const GAME_FOLDER: &str = "Ring of Elysium";

/// Parses one `"key"  "value"` line of a Valve KeyValues (VDF) file.
fn quoted_pair(line: &str) -> Option<(String, String)> {
    let mut toks = Vec::new();
    let mut chars = line.trim().chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut s = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        s.push(n);
                    }
                }
                '"' => break,
                _ => s.push(c),
            }
        }
        toks.push(s);
    }
    (toks.len() == 2).then(|| (toks.remove(0), toks.remove(0)))
}

/// All values of `"key" "value"` lines for the given key.
pub fn vdf_values(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .filter_map(quoted_pair)
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v)
        .collect()
}

fn canon(p: &Path) -> Option<PathBuf> {
    fs::canonicalize(p).ok().map(crate::util::plain_path)
}

/// Where Steam may be installed on this platform, most likely first.
#[cfg(not(windows))]
fn candidate_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    vec![
        home.join(".steam/steam"),
        home.join(".local/share/Steam"),
        home.join(".steam/root"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join("snap/steam/common/.local/share/Steam"),
    ]
}

/// Windows: what Steam wrote to the registry, then the default folders.
#[cfg(windows)]
fn candidate_roots() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = registry_steam("SteamPath").into_iter().collect();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(p) = std::env::var_os(var) {
            v.push(PathBuf::from(p).join("Steam"));
        }
    }
    v.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    v
}

/// A path value under `HKCU\Software\Valve\Steam` ("SteamPath", "SteamExe"); Steam writes forward slashes.
#[cfg(windows)]
fn registry_steam(name: &str) -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (key, value) = (wide(r"Software\Valve\Steam"), wide(name));
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its size in bytes are passed together; the result is NUL-terminated.
    let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    let s = String::from_utf16_lossy(&buf[..len]);
    (!s.is_empty()).then(|| PathBuf::from(s.replace('/', "\\")))
}

/// `StateFlags` of the game's app manifest in any library (4 = fully installed; other values while Steam
/// is downloading or updating). None when no library has a manifest for it.
pub fn install_state(libs: &[PathBuf]) -> Option<u32> {
    libs.iter().find_map(|lib| {
        let text = fs::read_to_string(lib.join(format!("steamapps/appmanifest_{APPID}.acf"))).ok()?;
        vdf_values(&text, "StateFlags").into_iter().next()?.parse().ok()
    })
}

/// StateFlags bits (Steam's EAppState): the files are complete.
const FULLY_INSTALLED: u32 = 4;
/// StateFlags bits while Steam writes the game's files: update running, uninstalling, validating, adding files,
/// preallocating, downloading, staging, committing.
const WRITING: u32 = 256 | 2048 | 131072 | 262144 | 524288 | 1048576 | 2097152 | 4194304;

/// Steam has finished installing the game (or it was installed without Steam: no manifest).
/// StateFlags is a set of bits: "update required" (2) or "update paused" (512) next to "fully installed" (4) still
/// means complete files. That is common for this game, which gets no updates any more, so only a missing
/// "fully installed" bit or Steam writing files right now counts as unfinished.
pub fn install_finished(libs: &[PathBuf]) -> bool {
    install_state(libs).is_none_or(flags_finished)
}

fn flags_finished(flags: u32) -> bool {
    flags & FULLY_INSTALLED != 0 && flags & WRITING == 0
}

/// Opens Steam's install dialog for the game (only works when it is in the user's Steam library).
pub const INSTALL_URL: &str = "steam://install/755790";

/// The first of `names` found in `dirs` (a PATH search without running anything).
pub fn which_in(dirs: &[PathBuf], names: &[&str]) -> Option<PathBuf> {
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

#[cfg(not(windows))]
fn path_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    // Debian/Ubuntu put steam in /usr/games, which is not always on a desktop app's PATH
    for extra in ["/usr/games", "/usr/bin", "/usr/local/bin"] {
        if !v.iter().any(|d| d == Path::new(extra)) {
            v.push(PathBuf::from(extra));
        }
    }
    v
}

/// Ways to ask Steam to install the game, most direct first. Passing the steam:// link to Steam itself
/// works even when nothing is registered for steam:// links; the system's link handler is the fallback.
pub fn install_commands() -> Vec<(PathBuf, Vec<String>)> {
    let url = INSTALL_URL.to_string();
    let mut v: Vec<(PathBuf, Vec<String>)> = Vec::new();
    #[cfg(windows)]
    {
        let mut exes: Vec<PathBuf> = registry_steam("SteamExe").into_iter().collect();
        exes.extend(candidate_roots().into_iter().map(|r| r.join("steam.exe")));
        for exe in exes.into_iter().filter(|p| p.is_file()) {
            if !v.iter().any(|(p, _)| *p == exe) {
                v.push((exe, vec![url.clone()]));
            }
        }
        v.push((PathBuf::from("explorer"), vec![url]));
    }
    #[cfg(not(windows))]
    {
        let dirs = path_dirs();
        if let Some(steam) = which_in(&dirs, &["steam"]) {
            v.push((steam, vec![url.clone()]));
        }
        for root in steam_roots() {
            let script = root.join("steam.sh");
            if script.is_file() {
                v.push((script, vec![url.clone()]));
            }
        }
        let flatpak_steam = dirs::home_dir().is_some_and(|h| h.join(".var/app/com.valvesoftware.Steam").is_dir());
        if let (true, Some(flatpak)) = (flatpak_steam, which_in(&dirs, &["flatpak"])) {
            v.push((flatpak, vec!["run".into(), "com.valvesoftware.Steam".into(), url.clone()]));
        }
        v.push((PathBuf::from("xdg-open"), vec![url]));
    }
    v
}

/// Asks Steam to show the install dialog. Returns how it was asked, or every attempt's error.
pub fn request_install() -> Result<String, String> {
    let mut errors = Vec::new();
    for (program, args) in install_commands() {
        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        match cmd.spawn() {
            Ok(_) => return Ok(program.display().to_string()),
            Err(e) => errors.push(format!("{}: {e}", program.display())),
        }
    }
    Err(errors.join("; "))
}

/// Steam installations: native, Flatpak, and the usual symlinks (deduplicated).
pub fn steam_roots() -> Vec<PathBuf> {
    let candidates = candidate_roots();
    let mut out: Vec<PathBuf> = Vec::new();
    for c in candidates {
        if c.join("steamapps").is_dir() || c.join("compatibilitytools.d").is_dir() {
            if let Some(real) = canon(&c) {
                if !out.contains(&real) {
                    out.push(real);
                }
            }
        }
    }
    out
}

/// Steam roots + every library folder listed in libraryfolders.vdf.
pub fn libraries(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for r in roots {
        if !out.contains(r) {
            out.push(r.clone());
        }
        if let Ok(text) = fs::read_to_string(r.join("steamapps/libraryfolders.vdf")) {
            for v in vdf_values(&text, "path") {
                let p = PathBuf::from(v);
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// The game folder inside the given libraries (uses the appmanifest's installdir).
pub fn find_game(libs: &[PathBuf]) -> Option<PathBuf> {
    for lib in libs {
        let steamapps = lib.join("steamapps");
        let installdir = fs::read_to_string(steamapps.join(format!("appmanifest_{APPID}.acf")))
            .ok()
            .and_then(|t| vdf_values(&t, "installdir").into_iter().next());
        let mut names = Vec::new();
        if let Some(n) = installdir {
            names.push(n);
        }
        names.push(GAME_FOLDER.to_string());
        for n in names {
            let dir = steamapps.join("common").join(n);
            if crate::game::is_game_dir(&dir) {
                return Some(dir);
            }
        }
    }
    None
}

/// The game's own Steam Wine prefix, when the game lives in a Steam library.
pub fn steam_prefix_for(game_dir: &Path) -> Option<PathBuf> {
    // <lib>/steamapps/common/<game>  ->  <lib>/steamapps/compatdata/<appid>
    let steamapps = game_dir.parent()?.parent()?;
    if steamapps.file_name()? != "steamapps" {
        return None;
    }
    let p = steamapps.join("compatdata").join(APPID.to_string());
    p.is_dir().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vdf_lines() {
        let t = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/mnt/games/SteamLibrary\"\n\t\t\"label\"\t\t\"\"\n\t}\n}\n";
        assert_eq!(vdf_values(t, "path"), vec!["/mnt/games/SteamLibrary"]);
    }

    #[test]
    fn finds_game_via_appmanifest() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("SteamLibrary");
        let game = lib.join("steamapps/common/Ring of Elysium");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("Europa_Client.exe"), b"x").unwrap();
        fs::write(
            lib.join("steamapps/appmanifest_755790.acf"),
            "\"AppState\"\n{\n\t\"installdir\"\t\t\"Ring of Elysium\"\n}\n",
        )
        .unwrap();
        fs::create_dir_all(lib.join("steamapps/compatdata/755790")).unwrap();
        assert_eq!(find_game(&[lib.clone()]), Some(game.clone()));
        assert_eq!(steam_prefix_for(&game), Some(lib.join("steamapps/compatdata/755790")));
    }

    #[test]
    fn finds_programs_like_a_path_search() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(b.join("steam")).unwrap(); // a folder named steam is not a program
        fs::write(a.join("flatpak"), "").unwrap();
        assert_eq!(which_in(&[b.clone(), a.clone()], &["steam"]), None);
        assert_eq!(which_in(&[b, a.clone()], &["steam", "flatpak"]), Some(a.join("flatpak")));
    }

    #[test]
    fn the_last_resort_is_the_systems_link_handler() {
        let cmds = install_commands();
        let (last, args) = cmds.last().unwrap();
        assert_eq!(args.last().map(String::as_str), Some(INSTALL_URL));
        assert!(last.ends_with(if cfg!(windows) { "explorer" } else { "xdg-open" }));
        // every way passes the same link
        assert!(cmds.iter().all(|(_, a)| a.last().map(String::as_str) == Some(INSTALL_URL)));
    }

    #[test]
    fn waits_for_steam_to_finish_installing() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().to_path_buf();
        fs::create_dir_all(lib.join("steamapps")).unwrap();
        assert!(install_finished(&[lib.clone()])); // no manifest: a copy installed by hand
        let manifest = |flags: u32| format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"755790\"\n\t\"StateFlags\"\t\t\"{flags}\"\n}}\n");
        fs::write(lib.join("steamapps/appmanifest_755790.acf"), manifest(1026)).unwrap();
        assert_eq!(install_state(&[lib.clone()]), Some(1026));
        assert!(!install_finished(&[lib.clone()]));
        fs::write(lib.join("steamapps/appmanifest_755790.acf"), manifest(4)).unwrap();
        assert!(install_finished(&[lib]));
    }

    #[test]
    fn an_update_steam_cant_or_doesnt_run_still_counts_as_installed() {
        // fully installed + update required / update paused / both: the files are complete
        for flags in [4, 6, 516, 518] {
            assert!(flags_finished(flags), "{flags}");
        }
        // started but not installed yet, or Steam writing files now
        for flags in [1026, 2, 0, 4 | 256, 4 | 1048576, 4 | 131072, 4 | 2048] {
            assert!(!flags_finished(flags), "{flags}");
        }
    }
}
