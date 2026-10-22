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

/// The files Steam ships for the supported build ("size<TAB>path" lines), from complete installs. The game gets no
/// more updates, so the list never changes. Files the game writes itself are not in it.
const GAME_FILES: &str = include_str!("game_files.txt");

/// What a folder holds of the shipped game files. Only names and sizes are compared (no hashing: 45 GB), which
/// takes milliseconds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileCheck {
    pub total: usize,
    pub missing: Vec<String>,
    /// Present, but not the shipped size (damaged or from another build).
    pub different: Vec<String>,
}

impl FileCheck {
    pub fn complete(&self) -> bool {
        self.missing.is_empty() && self.different.is_empty()
    }
    pub fn problems(&self) -> usize {
        self.missing.len() + self.different.len()
    }
    /// More than half the game is missing: an exe copied on its own (or with a few files), not a game folder.
    pub fn mostly_missing(&self) -> bool {
        self.missing.len() * 2 > self.total
    }
}

pub fn check_files(dir: &Path) -> FileCheck {
    check_files_against(dir, GAME_FILES)
}

fn check_files_against(dir: &Path, list: &str) -> FileCheck {
    let mut c = FileCheck::default();
    for line in list.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let Some((size, rel)) = line.split_once('\t') else { continue };
        let Ok(size) = size.parse::<u64>() else { continue };
        c.total += 1;
        match std::fs::metadata(dir.join(rel)) {
            Ok(m) if m.is_file() && m.len() == size => {}
            Ok(m) if m.is_file() => c.different.push(rel.to_string()),
            _ => c.missing.push(rel.to_string()),
        }
    }
    c
}

/// The exe is there and so is most of the game (see `FileCheck::mostly_missing`).
fn is_whole_game(p: &Path) -> bool {
    is_game_dir(p) && !check_files(p).mostly_missing()
}

/// Does the folder contain the world data the server reads?
pub fn has_world_data(p: &Path) -> bool {
    p.join(WORLD_DATA).is_file()
}

/// How far below the chosen folder `find_game` looks for Europa_Client.exe, and at most how many folders.
const SEARCH_DEPTH: usize = 3;
const SEARCH_LIMIT: usize = 5000;

/// The folder with a different (DX12) build of the client, inside the game folder: never the game folder itself.
const XVERSION: &str = "XVersion";

/// Accepts the game folder, the exe, a Steam library root, or a parent folder.
pub fn resolve_input(input: &str) -> Option<PathBuf> {
    find_game(input).ok()
}

/// Like `resolve_input`, but says why nothing was found, in words the player can act on. Besides the usual
/// places (the folder itself, Steam's layout below it) it looks a few folders deeper, so a parent folder or a
/// renamed copy works too.
pub fn find_game(input: &str) -> Result<PathBuf, String> {
    let text = clean_input(input);
    if text.is_empty() {
        return Err("Enter the folder that contains Europa_Client.exe.".into());
    }
    let mut p = PathBuf::from(&text);
    if p.is_file() {
        if !p.file_name().map(|n| n.eq_ignore_ascii_case("Europa_Client.exe")).unwrap_or(false) {
            return Err(format!("{text} is a file. Choose the folder that contains Europa_Client.exe."));
        }
        p = p.parent().map(Path::to_path_buf).unwrap_or(p);
    }
    if !p.is_dir() {
        return Err(format!("{text} doesn't exist, or the launcher can't open it."));
    }
    if is_xversion(&p) {
        if let Some(parent) = p.parent().filter(|d| is_game_dir(d)) {
            p = parent.to_path_buf();
        }
    }
    let candidates = [
        p.clone(),
        p.join(crate::steam::GAME_FOLDER),
        p.join("common").join(crate::steam::GAME_FOLDER),
        p.join("steamapps/common").join(crate::steam::GAME_FOLDER),
    ];
    // an exe without most of the game (copied on its own) is not the game folder: say so if nothing better turns up
    let lone_exe = candidates.iter().chain([&p]).find(|c| is_game_dir(c) && !is_xversion(c)).cloned();
    match candidates.into_iter().find(|c| is_whole_game(c) && !is_xversion(c)).or_else(|| search_below(&p)) {
        // some drives (network shares, RAM disks) can't report a full path: the folder found is still right
        Some(c) => Ok(crate::util::plain_path(std::fs::canonicalize(&c).unwrap_or(c))),
        None if lone_exe.is_some() => {
            let dir = lone_exe.unwrap_or_default();
            let c = check_files(&dir);
            Err(format!(
                "{} has Europa_Client.exe, but not the rest of the game ({} of {} game files missing). Choose the \
                 whole game folder (with Steam: steamapps, common, Ring of Elysium).",
                dir.display(),
                c.missing.len(),
                c.total
            ))
        }
        None => Err(format!(
            "No Europa_Client.exe in {} or the folders below it. Choose the folder that contains Europa_Client.exe \
             (with Steam: steamapps, common, Ring of Elysium).",
            p.display()
        )),
    }
}

fn is_xversion(p: &Path) -> bool {
    p.file_name().map(|n| n.eq_ignore_ascii_case(XVERSION)).unwrap_or(false)
}

/// Breadth first, so the copy nearest to `root` wins. Skips XVersion and links (no loops).
fn search_below(root: &Path) -> Option<PathBuf> {
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0)]);
    let mut visited = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        if depth > 0 && is_whole_game(&dir) {
            return Some(dir);
        }
        if depth == SEARCH_DEPTH {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|d| !is_xversion(d))
            .collect();
        subs.sort();
        for d in subs {
            visited += 1;
            if visited > SEARCH_LIMIT {
                return None;
            }
            queue.push_back((d, depth + 1));
        }
    }
    None
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

    /// A complete game folder for tests: every shipped file, but empty (present, other size), so it's cheap.
    fn fake_game(dir: &Path) {
        for line in GAME_FILES.lines().filter(|l| !l.starts_with('#')) {
            let rel = line.split_once('\t').unwrap().1;
            let p = dir.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"").unwrap();
        }
    }

    #[test]
    fn resolves_exe_library_and_folder_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("lib/steamapps/common/Ring of Elysium");
        fake_game(&game);
        // the result never has Windows' `\\?\` prefix
        let real = crate::util::plain_path(fs::canonicalize(&game).unwrap());
        assert_eq!(resolve_input(game.to_str().unwrap()), Some(real.clone()));
        assert_eq!(resolve_input(game.join("Europa_Client.exe").to_str().unwrap()), Some(real.clone()));
        assert_eq!(resolve_input(tmp.path().join("lib").to_str().unwrap()), Some(real.clone()));
        assert_eq!(resolve_input(&format!("'{}'", game.display())), Some(real));
        assert_eq!(resolve_input("/definitely/not/here"), None);
    }

    #[test]
    fn finds_a_renamed_copy_below_the_chosen_folder_and_explains_misses() {
        let tmp = tempfile::tempdir().unwrap();
        let real = |p: &Path| crate::util::plain_path(fs::canonicalize(p).unwrap());
        // a renamed copy two levels below the chosen folder, with XVersion's DX12 exe inside it
        let game = tmp.path().join("Games/panicmode dont open");
        fake_game(&game);
        assert_eq!(find_game(tmp.path().to_str().unwrap()), Ok(real(&game)));
        // picking XVersion means the game folder above it
        assert_eq!(find_game(game.join("XVersion").to_str().unwrap()), Ok(real(&game)));

        // an XVersion folder alone is never taken as the game
        let only_dx12 = tmp.path().join("other");
        fs::create_dir_all(only_dx12.join("XVersion")).unwrap();
        fs::write(only_dx12.join("XVersion/Europa_Client.exe"), b"dx12").unwrap();
        let miss = find_game(only_dx12.to_str().unwrap()).unwrap_err();
        assert!(miss.starts_with("No Europa_Client.exe in "), "{miss}");

        // too deep (more than 3 folders below) is not searched
        let deep = tmp.path().join("deep/a/b/c/d");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("Europa_Client.exe"), b"x").unwrap();
        assert!(find_game(tmp.path().join("deep").to_str().unwrap()).is_err());

        // an exe copied on its own is not the game folder, also when the search could reach it
        let lone = tmp.path().join("lone");
        fs::create_dir_all(&lone).unwrap();
        fs::write(lone.join("Europa_Client.exe"), b"x").unwrap();
        let miss = find_game(lone.to_str().unwrap()).unwrap_err();
        assert!(miss.contains("has Europa_Client.exe, but not the rest of the game"), "{miss}");

        // every miss says what went wrong
        assert!(find_game("  ").unwrap_err().starts_with("Enter the folder"));
        assert!(find_game("/definitely/not/here").unwrap_err().contains("doesn't exist"));
        let note = tmp.path().join("readme.txt");
        fs::write(&note, b"x").unwrap();
        assert!(find_game(note.to_str().unwrap()).unwrap_err().contains("is a file"));
    }

    #[test]
    fn checks_the_shipped_files_by_name_and_size() {
        let tmp = tempfile::tempdir().unwrap();
        let list = "# comment\n3\ta.dat\n5\tsub/b.dat\n2\tc.dat\n";
        fs::create_dir_all(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("a.dat"), b"abc").unwrap();
        fs::write(tmp.path().join("sub/b.dat"), b"ab").unwrap();
        let c = check_files_against(tmp.path(), list);
        assert_eq!((c.total, c.missing.clone(), c.different.clone()), (3, vec!["c.dat".to_string()], vec!["sub/b.dat".to_string()]));
        assert!(!c.complete() && !c.mostly_missing() && c.problems() == 2);
        assert!(check_files_against(&tmp.path().join("nowhere"), list).mostly_missing());

        // the built-in list: the supported exe and the world data are in it, game-written files are not
        assert!(GAME_FILES.contains("27855816\tEuropa_Client.exe\n"));
        assert!(GAME_FILES.contains(WORLD_DATA));
        assert!(!GAME_FILES.to_lowercase().contains("userdata/") && !GAME_FILES.contains(".dmp"));
        let g = tmp.path().join("game");
        fake_game(&g);
        let c = check_files(&g);
        assert!(c.total > 250 && c.missing.is_empty() && !c.mostly_missing());
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
