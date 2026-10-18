//! The launcher's own folder of user-added background images.
//! Images are *copied* here, so moving, renaming or deleting the original never loses the
//! setting. The folder sits next to (not inside) the server folder, so server updates,
//! which replace everything in that folder, never touch it.
use crate::util::data_dir;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_BYTES: u64 = 40 * 1024 * 1024;
pub const EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

pub fn dir() -> PathBuf {
    data_dir().join("backgrounds")
}

fn extension_of(p: &Path) -> Option<String> {
    let e = p.extension()?.to_string_lossy().to_lowercase();
    EXTENSIONS.contains(&e.as_str()).then_some(e)
}

/// A stored name must be a plain file name: no separators, no `..`.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.starts_with('.') && extension_of(Path::new(name)).is_some()
}

/// Copies an image into `dest` and returns the stored name (`<name>-<hash>.<ext>`).
/// Adding the same image twice yields the same name, so there are no duplicates.
pub fn import(src: &Path, dest: &Path) -> Result<String> {
    let ext = extension_of(src).with_context(|| format!("unsupported image type (use {})", EXTENSIONS.join(", ")))?;
    let meta = fs::metadata(src).with_context(|| format!("reading {}", src.display()))?;
    if !meta.is_file() {
        bail!("{} is not a file", src.display());
    }
    if meta.len() > MAX_BYTES {
        bail!("the image is larger than {} MB", MAX_BYTES / 1024 / 1024);
    }
    let bytes = fs::read(src)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let stem: String = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .take(40)
        .collect();
    let stem = if stem.is_empty() { "background".to_string() } else { stem };
    let name = format!("{stem}-{}.{ext}", &hash[..8]);
    fs::create_dir_all(dest)?;
    let target = dest.join(&name);
    if !target.exists() {
        fs::write(&target, &bytes)?;
    }
    Ok(name)
}

/// Full path of a stored image, or None when the name is unsafe or the file is gone.
pub fn path_of(dest: &Path, name: &str) -> Option<PathBuf> {
    if !is_plain_name(name) {
        return None;
    }
    let p = dest.join(name);
    p.is_file().then_some(p)
}

pub fn list(dest: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dest)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().map(String::from)).filter(|n| is_plain_name(n)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

pub fn remove(dest: &Path, name: &str) -> Result<()> {
    if !is_plain_name(name) {
        bail!("invalid background name");
    }
    match fs::remove_file(dest.join(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_copies_so_the_original_can_vanish() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("My Wallpaper!.PNG");
        fs::write(&src, b"fake image bytes").unwrap();
        let dest = tmp.path().join("backgrounds");
        let name = import(&src, &dest).unwrap();
        assert!(name.starts_with("My-Wallpaper--") && name.ends_with(".png"), "{name}");
        fs::remove_file(&src).unwrap(); // the user deletes / renames the original
        assert!(path_of(&dest, &name).is_some()); // the launcher still has its copy
        assert_eq!(list(&dest), vec![name]);
    }

    #[test]
    fn adding_the_same_image_twice_does_not_duplicate_it() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.jpg");
        fs::write(&src, b"same bytes").unwrap();
        let dest = tmp.path().join("b");
        assert_eq!(import(&src, &dest).unwrap(), import(&src, &dest).unwrap());
        assert_eq!(list(&dest).len(), 1);
    }

    #[test]
    fn rejects_bad_types_missing_files_and_path_tricks() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("b");
        let exe = tmp.path().join("x.exe");
        fs::write(&exe, b"x").unwrap();
        assert!(import(&exe, &dest).is_err());
        assert!(import(&tmp.path().join("nope.png"), &dest).is_err());
        fs::create_dir_all(&dest).unwrap();
        fs::write(tmp.path().join("secret.png"), b"s").unwrap();
        assert!(path_of(&dest, "../secret.png").is_none());
        assert!(remove(&dest, "../secret.png").is_err());
        assert!(tmp.path().join("secret.png").exists());
    }

    #[test]
    fn remove_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.jpg");
        fs::write(&src, b"1").unwrap();
        let dest = tmp.path().join("b");
        let n = import(&src, &dest).unwrap();
        remove(&dest, &n).unwrap();
        remove(&dest, &n).unwrap();
        assert!(list(&dest).is_empty());
    }
}
