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
    let mut v: Vec<PathBuf> = registry_steam_path().into_iter().collect();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(p) = std::env::var_os(var) {
            v.push(PathBuf::from(p).join("Steam"));
        }
    }
    v.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    v
}

/// `HKCU\Software\Valve\Steam\SteamPath` (Steam writes it with forward slashes).
#[cfg(windows)]
fn registry_steam_path() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (key, value) = (wide(r"Software\Valve\Steam"), wide("SteamPath"));
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
}
