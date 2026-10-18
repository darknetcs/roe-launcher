//! Validation of the Ring of Elysium client folder.
use crate::util::clean_input;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// The server only supports this exact Europa_Client.exe (the 2022 build).
pub const EXPECTED_SHA256: &str = "2dca9b1e122e21363e14eeeb2372598c773205a907ed8d2b2d54a7229ad6ba18";
pub const WORLD_DATA: &str = "SFC/World_EU_ICELAND_4096_RemainingFiles.sfc";

pub fn is_game_dir(p: &Path) -> bool {
    p.join("Europa_Client.exe").is_file()
}

/// Does the folder contain the world data the server reads?
pub fn has_world_data(p: &Path) -> bool {
    p.join(WORLD_DATA).is_file()
}

/// Accepts the game folder, the exe, a Steam library root, or a parent folder.
pub fn resolve_input(input: &str) -> Option<PathBuf> {
    let mut p = PathBuf::from(clean_input(input));
    if p.is_file() && p.file_name().map(|n| n == "Europa_Client.exe").unwrap_or(false) {
        p = p.parent()?.to_path_buf();
    }
    let candidates = [
        p.clone(),
        p.join(crate::steam::GAME_FOLDER),
        p.join("common").join(crate::steam::GAME_FOLDER),
        p.join("steamapps/common").join(crate::steam::GAME_FOLDER),
    ];
    candidates.into_iter().find(|c| is_game_dir(c)).and_then(|c| std::fs::canonicalize(c).ok()).map(crate::util::plain_path)
}

pub fn sha256_file(p: &Path) -> io::Result<String> {
    let mut f = File::open(p)?;
    let mut h = Sha256::new();
    io::copy(&mut f, &mut h)?;
    Ok(format!("{:x}", h.finalize()))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Build {
    Supported,
    Different(String),
}

pub fn check_build(dir: &Path) -> io::Result<Build> {
    let sha = sha256_file(&dir.join("Europa_Client.exe"))?;
    Ok(if sha == EXPECTED_SHA256 { Build::Supported } else { Build::Different(sha) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolves_exe_library_and_folder_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("lib/steamapps/common/Ring of Elysium");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("Europa_Client.exe"), b"x").unwrap();
        // the result never has Windows' `\\?\` prefix
        let real = crate::util::plain_path(fs::canonicalize(&game).unwrap());
        assert_eq!(resolve_input(game.to_str().unwrap()), Some(real.clone()));
        assert_eq!(resolve_input(game.join("Europa_Client.exe").to_str().unwrap()), Some(real.clone()));
        assert_eq!(resolve_input(tmp.path().join("lib").to_str().unwrap()), Some(real.clone()));
        assert_eq!(resolve_input(&format!("'{}'", game.display())), Some(real));
        assert_eq!(resolve_input("/definitely/not/here"), None);
    }

    #[test]
    fn hashes_files() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("Europa_Client.exe");
        fs::write(&f, b"abc").unwrap();
        assert_eq!(sha256_file(&f).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(matches!(check_build(tmp.path()).unwrap(), Build::Different(_)));
    }
}
