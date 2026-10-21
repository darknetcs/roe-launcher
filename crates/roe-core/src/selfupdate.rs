//! Updates the launcher itself from its GitHub releases.
//!
//! The newest release tag comes from the GitHub API, or (when that is rate-limited) from where
//! `releases/latest` redirects. Release files have fixed names, so their URLs follow from the tag.
//! A download is installed only when it matches the release's `.sha256` file.
//!
//! Installing replaces the launcher's own file. Linux: the new file is renamed over the old one (the
//! running launcher keeps its copy until it restarts). Windows cannot overwrite a running program but
//! can rename it: the old exe becomes `roe-launcher.old.exe` and is deleted on the next start.
use crate::util::version_key;
use crate::Sink;
use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const REPO: &str = "darknetcs/roe-launcher";
pub const RELEASES_PAGE: &str = "https://github.com/darknetcs/roe-launcher/releases/latest";
/// This launcher's version (all crates share it).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// e.g. "v0.1.1"
    pub tag: String,
    /// Release notes (may be empty).
    pub notes: String,
}

impl Release {
    pub fn page(&self) -> String {
        format!("https://github.com/{REPO}/releases/tag/{}", self.tag)
    }

    /// Download URL of a file attached to this release.
    pub fn asset_url(&self, name: &str) -> String {
        format!("https://github.com/{REPO}/releases/download/{}/{name}", self.tag)
    }

    pub fn is_newer_than(&self, current: &str) -> bool {
        is_newer(&self.tag, current)
    }
}

/// The release package for this platform.
pub fn asset_name() -> &'static str {
    if cfg!(windows) {
        "roe-launcher-windows-x86_64.zip"
    } else {
        "roe-launcher-linux-x86_64.tar.gz"
    }
}

/// "v0.1.10" > "0.1.9"; a pre-release suffix ("-rc1") counts as its numbers.
pub fn is_newer(tag: &str, current: &str) -> bool {
    version_key(tag.trim_start_matches('v')) > version_key(current.trim_start_matches('v'))
}

/// The newest published release.
pub fn latest() -> Result<Release> {
    match latest_via_api() {
        Ok(r) => Ok(r),
        Err(api) => latest_via_redirect().map_err(|e| anyhow!("could not check for launcher updates ({api}; {e})")),
    }
}

fn latest_via_api() -> Result<Release> {
    let v: serde_json::Value = crate::update::agent(15)
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .call()
        .map_err(|e| anyhow!("GitHub API: {e}"))?
        .into_json()?;
    let tag = v["tag_name"].as_str().ok_or_else(|| anyhow!("GitHub API: no tag in the answer"))?;
    Ok(Release { tag: tag.to_string(), notes: v["body"].as_str().unwrap_or_default().to_string() })
}

/// `releases/latest` redirects to `.../releases/tag/<tag>`; this needs no API quota.
fn latest_via_redirect() -> Result<Release> {
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(15)).user_agent("roe-launcher").build();
    let resp = match agent.get(RELEASES_PAGE).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => bail!("release page: {e}"),
    };
    let location = resp.header("location").ok_or_else(|| anyhow!("release page: no redirect"))?;
    tag_from_url(location).map(|tag| Release { tag, notes: String::new() }).ok_or_else(|| anyhow!("release page: unexpected redirect {location}"))
}

fn tag_from_url(url: &str) -> Option<String> {
    let tag = url.split("/releases/tag/").nth(1)?.split(['?', '#', '/']).next()?;
    (!tag.is_empty()).then(|| tag.to_string())
}

/// The checksum for `name` from a `sha256sum`-style file ("<hex>  <name>").
pub fn checksum_for(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let (hash, file) = (parts.next()?, parts.next()?);
        (file.trim_start_matches('*') == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())).then(|| hash.to_lowercase())
    })
}

/// Why this launcher cannot replace itself, if it cannot.
pub fn unsupported_reason(exe: &Path) -> Option<String> {
    let in_build_dir = exe.ancestors().any(|a| a.file_name().is_some_and(|n| n == "target")) && exe.ancestors().any(|a| a.join("Cargo.toml").is_file());
    if in_build_dir {
        return Some("this launcher was started from a source build (cargo run); update the source instead".into());
    }
    let dir = exe.parent()?;
    let probe = dir.join(".roe-launcher-write-test");
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            None
        }
        Err(_) => Some(format!("the launcher's folder ({}) is not writable; download the new version from GitHub", dir.display())),
    }
}

/// Downloads `release` for this platform, checks it, and puts it in place of `exe`.
pub fn install(release: &Release, exe: &Path, sink: &Sink) -> Result<()> {
    if let Some(why) = unsupported_reason(exe) {
        bail!(why);
    }
    let dir = exe.parent().ok_or_else(|| anyhow!("unexpected launcher path {}", exe.display()))?;
    let name = asset_name();
    sink(format!("Downloading launcher {}…", release.tag));
    let sums = crate::update::agent(30)
        .get(&release.asset_url(&format!("{name}.sha256")))
        .call()
        .map_err(|e| anyhow!("downloading the checksum: {e}"))?
        .into_string()?;
    let want = checksum_for(&sums, name).ok_or_else(|| anyhow!("the release has no checksum for {name}; not installing it"))?;

    // everything is written next to the launcher, so the final rename stays on one disk
    let archive = dir.join(format!(".{name}.download"));
    let mut body = Vec::new();
    crate::update::agent(300)
        .get(&release.asset_url(name))
        .call()
        .map_err(|e| anyhow!("downloading {name}: {e}"))?
        .into_reader()
        .take(200 * 1024 * 1024)
        .read_to_end(&mut body)?;
    fs::write(&archive, &body)?;
    let result = (|| {
        let got = crate::runtime::sha256_hex(&archive)?;
        if got != want {
            bail!("the download does not match its checksum (expected {want}, got {got}); not installing it");
        }
        let fresh = dir.join(if cfg!(windows) { "roe-launcher.new.exe" } else { ".roe-launcher.new" });
        extract_launcher(&archive, &fresh)?;
        swap_in(&fresh, exe)?;
        // the documents (licences, privacy policy, README…) of the same version; a problem here never undoes the update
        match install_package_files(&archive, dir) {
            Ok(n) => sink(format!("Updated {n} documents and files next to the launcher.")),
            Err(e) => sink(format!("The launcher was updated, but its documents could not be: {e:#}")),
        }
        Ok(())
    })();
    let _ = fs::remove_file(&archive);
    result?;
    sink(format!("Launcher {} installed. Restart the launcher to use it.", release.tag));
    Ok(())
}

/// Takes the launcher binary out of a release package (`roe-launcher/roe-launcher[.exe]`).
pub fn extract_launcher(archive: &Path, out: &Path) -> Result<()> {
    let wanted = if cfg!(windows) { "roe-launcher.exe" } else { "roe-launcher" };
    let is_it = |p: &str| p.trim_end_matches('/').rsplit('/').next() == Some(wanted) && p.matches('/').count() <= 1;
    let file_name = archive.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let bytes = if file_name.contains(".zip") {
        let mut z = zip::ZipArchive::new(fs::File::open(archive)?).context("opening the zip")?;
        let mut found = None;
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            if e.is_file() && is_it(e.name()) {
                let mut v = Vec::new();
                e.read_to_end(&mut v)?;
                found = Some(v);
                break;
            }
        }
        found
    } else {
        let mut t = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
        let mut found = None;
        for e in t.entries()? {
            let mut e = e?;
            let path = e.path()?.to_string_lossy().to_string();
            if e.header().entry_type().is_file() && is_it(&path) {
                let mut v = Vec::new();
                e.read_to_end(&mut v)?;
                found = Some(v);
                break;
            }
        }
        found
    };
    let bytes = bytes.ok_or_else(|| anyhow!("{wanted} is not in the downloaded package"))?;
    if bytes.len() < 1024 * 1024 {
        bail!("the {wanted} in the package is too small to be the launcher");
    }
    fs::write(out, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(out, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// A file the package may put next to the launcher: a plain name at the package's top level (no folders, `..` or
/// hidden names) that isn't the launcher itself (that one is swapped in separately).
fn package_file_name(path: &str) -> Option<String> {
    let rest = path.strip_prefix("roe-launcher/")?;
    let ok = !rest.is_empty()
        && !rest.starts_with('.')
        && !rest.contains(['/', '\\'])
        && rest != "roe-launcher"
        && rest != "roe-launcher.exe";
    ok.then(|| rest.to_string())
}

/// Every other file of a release package: (name, contents). At most 64 files of 10 MB each.
pub fn package_files(archive: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    const MAX: u64 = 10 * 1024 * 1024;
    let mut out = Vec::new();
    let mut take = |name: Option<String>, size: u64, read: &mut dyn Read| -> Result<()> {
        if let Some(n) = name {
            if size > MAX || out.len() >= 64 {
                bail!("unexpected file {n} in the package");
            }
            let mut v = Vec::new();
            read.take(MAX).read_to_end(&mut v)?;
            out.push((n, v));
        }
        Ok(())
    };
    let file_name = archive.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if file_name.contains(".zip") {
        let mut z = zip::ZipArchive::new(fs::File::open(archive)?).context("opening the zip")?;
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            if e.is_file() {
                let (name, size) = (package_file_name(e.name()), e.size());
                take(name, size, &mut e)?;
            }
        }
    } else {
        let mut t = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
        for e in t.entries()? {
            let mut e = e?;
            if e.header().entry_type().is_file() {
                let (name, size) = (package_file_name(&e.path()?.to_string_lossy()), e.size());
                take(name, size, &mut e)?;
            }
        }
    }
    Ok(out)
}

/// Writes the package's other files next to the launcher (each via a temporary name, then renamed over the old
/// one). Returns how many were written.
pub fn install_package_files(archive: &Path, dir: &Path) -> Result<usize> {
    let files = package_files(archive)?;
    for (name, bytes) in &files {
        let tmp = dir.join(format!(".{name}.new"));
        fs::write(&tmp, bytes).with_context(|| format!("writing {name}"))?;
        fs::rename(&tmp, dir.join(name)).with_context(|| format!("replacing {name}"))?;
    }
    Ok(files.len())
}

/// Where Windows keeps the replaced launcher until the next start.
pub fn old_path(exe: &Path) -> PathBuf {
    exe.with_file_name("roe-launcher.old.exe")
}

/// Puts `fresh` where `exe` is.
pub fn swap_in(fresh: &Path, exe: &Path) -> Result<()> {
    if cfg!(windows) {
        let old = old_path(exe);
        let _ = fs::remove_file(&old);
        fs::rename(exe, &old).context("moving the running launcher aside")?;
        if let Err(e) = fs::rename(fresh, exe) {
            let _ = fs::rename(&old, exe); // put the working one back
            return Err(e).context("moving the new launcher into place");
        }
    } else {
        fs::rename(fresh, exe).context("moving the new launcher into place")?;
    }
    Ok(())
}

/// Deletes what a previous update left behind (Windows: the replaced exe).
pub fn cleanup(exe: &Path) {
    let _ = fs::remove_file(old_path(exe));
}

/// The running launcher's path, without Windows' `\\?\` prefix. Call it at start: once the file has
/// been replaced, Linux reports the running program as "<path> (deleted)" (that suffix is removed too).
pub fn current_exe() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?;
    let p = match p.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(s) => PathBuf::from(s),
        None => p,
    };
    Some(crate::util::plain_path(fs::canonicalize(&p).unwrap_or(p)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
    }

    #[test]
    fn reads_tags_and_checksums() {
        assert_eq!(tag_from_url("https://github.com/darknetcs/roe-launcher/releases/tag/v0.1.1").as_deref(), Some("v0.1.1"));
        assert_eq!(tag_from_url("https://github.com/x/y/releases"), None);
        let h = "2149c54279ebf11382eb413e4a337fd27964a23c99aa466058b1011c3611666c";
        let text = format!("{h}  roe-launcher-windows-x86_64.zip\n");
        assert_eq!(checksum_for(&text, "roe-launcher-windows-x86_64.zip").as_deref(), Some(h));
        assert_eq!(checksum_for(&text, "roe-launcher-linux-x86_64.tar.gz"), None);
        assert_eq!(checksum_for("nonsense  roe-launcher-windows-x86_64.zip", "roe-launcher-windows-x86_64.zip"), None);
    }

    fn fake_binary() -> Vec<u8> {
        vec![0x7f; 2 * 1024 * 1024]
    }

    #[test]
    fn takes_the_launcher_out_of_both_package_kinds() {
        let tmp = tempfile::tempdir().unwrap();
        let wanted = if cfg!(windows) { "roe-launcher.exe" } else { "roe-launcher" };
        // tar.gz like the Linux package
        let tgz = tmp.path().join("pkg.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(fs::File::create(&tgz).unwrap(), flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            for (name, data) in [("roe-launcher/README.md", b"readme".to_vec()), (format!("roe-launcher/{wanted}").as_str(), fake_binary())] {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o755);
                h.set_cksum();
                b.append_data(&mut h, name, data.as_slice()).unwrap();
            }
            b.into_inner().unwrap().finish().unwrap();
        }
        let out = tmp.path().join("out1");
        extract_launcher(&tgz, &out).unwrap();
        assert_eq!(fs::read(&out).unwrap().len(), fake_binary().len());
        // zip like the Windows package
        let zp = tmp.path().join("pkg.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&zp).unwrap());
            w.start_file("roe-launcher/NOTICE.md", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"notice").unwrap();
            w.start_file(format!("roe-launcher/{wanted}"), zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&fake_binary()).unwrap();
            w.finish().unwrap();
        }
        let out = tmp.path().join("out2");
        extract_launcher(&zp, &out).unwrap();
        assert_eq!(fs::read(&out).unwrap().len(), fake_binary().len());
        // a package without the launcher is refused
        let empty = tmp.path().join("empty.zip");
        zip::ZipWriter::new(fs::File::create(&empty).unwrap()).finish().unwrap();
        assert!(extract_launcher(&empty, &tmp.path().join("out3")).is_err());
    }

    #[test]
    fn an_update_also_brings_the_documents_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let entries: Vec<(&str, Vec<u8>)> = vec![
            ("roe-launcher/PRIVACY.md", b"privacy v2".to_vec()),
            ("roe-launcher/THIRD-PARTY-LICENSES.html", b"licences".to_vec()),
            ("roe-launcher/roe-launcher", fake_binary()),
            ("roe-launcher/roe-launcher.exe", fake_binary()),
            ("roe-launcher/.hidden", b"x".to_vec()),
            ("roe-launcher/sub/deep.txt", b"x".to_vec()),
            ("other/README.md", b"x".to_vec()),
        ];
        let expect = vec!["PRIVACY.md".to_string(), "THIRD-PARTY-LICENSES.html".to_string()];
        // tar.gz like the Linux package
        let tgz = tmp.path().join("pkg.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(fs::File::create(&tgz).unwrap(), flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            for (name, data) in &entries {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, name, data.as_slice()).unwrap();
            }
            b.into_inner().unwrap().finish().unwrap();
        }
        let names: Vec<String> = package_files(&tgz).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, expect);
        // zip like the Windows package
        let zp = tmp.path().join("pkg.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&zp).unwrap());
            for (name, data) in &entries {
                w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        let names: Vec<String> = package_files(&zp).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, expect);
        // installing replaces the old copies and leaves other files alone
        let dir = tmp.path().join("launcher");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("PRIVACY.md"), "privacy v1").unwrap();
        fs::write(dir.join("my-notes.txt"), "mine").unwrap();
        assert_eq!(install_package_files(&zp, &dir).unwrap(), 2);
        assert_eq!(fs::read_to_string(dir.join("PRIVACY.md")).unwrap(), "privacy v2");
        assert_eq!(fs::read_to_string(dir.join("THIRD-PARTY-LICENSES.html")).unwrap(), "licences");
        assert_eq!(fs::read_to_string(dir.join("my-notes.txt")).unwrap(), "mine");
        assert!(!dir.join(".hidden").exists() && !dir.join("sub").exists() && !dir.join("deep.txt").exists());
        assert!(!dir.join(".PRIVACY.md.new").exists());
    }

    #[test]
    fn swaps_the_new_launcher_in() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join(if cfg!(windows) { "roe-launcher.exe" } else { "roe-launcher" });
        let fresh = tmp.path().join("fresh");
        fs::write(&exe, b"old").unwrap();
        fs::write(&fresh, b"new").unwrap();
        swap_in(&fresh, &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert!(!fresh.exists());
        cleanup(&exe);
        assert!(!old_path(&exe).exists());
    }

    #[test]
    fn source_builds_never_replace_themselves() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        let exe = tmp.path().join("target/release/roe-launcher");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        assert!(unsupported_reason(&exe).unwrap().contains("source build"));
        let installed = tmp.path().join("apps/roe-launcher");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        assert_eq!(unsupported_reason(&installed), None);
    }

    /// Against the real GitHub releases: `cargo test -p roe-core -- --ignored live_`
    #[test]
    #[ignore]
    fn live_installs_the_published_release_into_a_temp_folder() {
        let r = latest().unwrap();
        // the fallback (no API quota) must find the same release
        assert_eq!(latest_via_redirect().unwrap().tag, r.tag);
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join(if cfg!(windows) { "roe-launcher.exe" } else { "roe-launcher" });
        fs::write(&exe, b"old").unwrap();
        let sink: Sink = std::sync::Arc::new(|l| eprintln!("{l}"));
        install(&r, &exe, &sink).unwrap();
        assert!(fs::metadata(&exe).unwrap().len() > 1024 * 1024);
        // packages from v0.1.10 on carry the privacy policy, and updates now install it too
        assert!(tmp.path().join("PRIVACY.md").is_file());
        eprintln!("installed {} into {}", r.tag, exe.display());
    }
}
