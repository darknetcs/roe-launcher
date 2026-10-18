//! Keeps the launcher's own copy of ring-of-elysium-local in sync with GitHub.
//! Downloads the exact commit as a tarball (no git needed), swaps it in atomically-ish and
//! keeps the expensive/user data (`.tools/` = Python venv + .NET, `runs/` = logs).
use crate::Sink;
use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const REPO: &str = "ROE-LIVE/ring-of-elysium-local";
pub const BRANCH: &str = "main";
/// Folders that survive an update.
const KEEP: [&str; 2] = [".tools", "runs"];
const INFO_FILE: &str = ".roe-launcher.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remote {
    pub sha: String,
    pub message: String,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InstallInfo {
    pub sha: String,
    pub installed_at: u64,
    pub translated: bool,
    pub untranslated: Option<u32>,
}

fn agent(secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(secs)).user_agent("roe-launcher").build()
}

/// Latest commit on the branch. Tries three sources so one rate limit never blocks updates:
/// the public Atom feed (no quota), the REST API (60 requests/hour/IP), git smart-HTTP refs.
pub fn remote_head() -> Result<Remote> {
    let mut errs = Vec::new();
    for (name, f) in [("atom feed", remote_via_atom as fn() -> Result<Remote>), ("GitHub API", remote_via_api), ("git refs", remote_via_refs)] {
        match f() {
            Ok(r) => return Ok(r),
            Err(e) => errs.push(format!("{name}: {e}")),
        }
    }
    bail!("could not reach GitHub ({})", errs.join("; "))
}

fn get_text(url: &str) -> Result<String> {
    Ok(agent(20).get(url).set("Accept", "*/*").call().map_err(|e| anyhow!("{e}"))?.into_string()?)
}

fn remote_via_atom() -> Result<Remote> {
    parse_atom(&get_text(&format!("https://github.com/{REPO}/commits/{BRANCH}.atom"))?)
}

fn remote_via_api() -> Result<Remote> {
    let v: serde_json::Value = agent(20)
        .get(&format!("https://api.github.com/repos/{REPO}/commits/{BRANCH}"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| anyhow!("{e}"))?
        .into_json()?;
    parse_remote(&v)
}

fn remote_via_refs() -> Result<Remote> {
    let text = get_text(&format!("https://github.com/{REPO}.git/info/refs?service=git-upload-pack"))?;
    parse_refs(&text)
}

fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&amp;", "&")
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

/// First `<entry>` of GitHub's commits Atom feed.
pub fn parse_atom(xml: &str) -> Result<Remote> {
    let entry = &xml[xml.find("<entry>").ok_or_else(|| anyhow!("no commits in feed"))?..];
    let sha = between(entry, "Grit::Commit/", "<").ok_or_else(|| anyhow!("no commit id in feed"))?.trim().to_string();
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("unexpected commit id in feed");
    }
    let message = unescape_xml(between(entry, "<title>", "</title>").unwrap_or("").trim());
    let date = between(entry, "<updated>", "</updated>").unwrap_or("").trim().to_string();
    Ok(Remote { sha, message, date })
}

/// `<sha> refs/heads/main` out of the smart-HTTP advertisement (no message/date available).
pub fn parse_refs(text: &str) -> Result<Remote> {
    let marker = format!(" refs/heads/{BRANCH}");
    let i = text.find(&marker).filter(|i| *i >= 40).ok_or_else(|| anyhow!("branch not found"))?;
    let sha = text[i - 40..i].to_string();
    if !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("unexpected ref advertisement");
    }
    Ok(Remote { sha, message: String::new(), date: String::new() })
}

pub fn parse_remote(v: &serde_json::Value) -> Result<Remote> {
    let sha = v["sha"].as_str().ok_or_else(|| anyhow!("unexpected GitHub response"))?.to_string();
    let message = v["commit"]["message"].as_str().unwrap_or("").lines().next().unwrap_or("").to_string();
    let date = v["commit"]["committer"]["date"].as_str().unwrap_or("").to_string();
    Ok(Remote { sha, message, date })
}

pub fn info_path(dir: &Path) -> PathBuf {
    dir.join(INFO_FILE)
}

pub fn read_info(dir: &Path) -> Option<InstallInfo> {
    serde_json::from_str(&fs::read_to_string(info_path(dir)).ok()?).ok()
}

pub fn write_info(dir: &Path, info: &InstallInfo) -> Result<()> {
    fs::write(info_path(dir), serde_json::to_string_pretty(info)?)?;
    Ok(())
}

/// A usable server checkout (what play-server.sh checks for).
pub fn is_installed(dir: &Path) -> bool {
    dir.join("requirements.txt").is_file() && dir.join("tools/windows_launch.py").is_file()
}

pub fn update_available(info: Option<&InstallInfo>, remote: &Remote) -> bool {
    info.map(|i| i.sha != remote.sha).unwrap_or(true)
}

/// Unpacks a GitHub tarball into `dest`, dropping the leading `<repo>-<sha>/` folder.
pub fn extract_tar_gz<R: Read>(reader: R, dest: &Path) -> Result<usize> {
    let mut archive = tar::Archive::new(GzDecoder::new(reader));
    let mut n = 0;
    for entry in archive.entries()? {
        let mut e = entry?;
        if matches!(e.header().entry_type(), tar::EntryType::XGlobalHeader | tar::EntryType::XHeader) {
            continue;
        }
        let path = e.path()?.into_owned();
        let rel: PathBuf = path.components().skip(1).collect();
        // refuse anything that is not a plain relative path (zip-slip protection)
        if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            continue;
        }
        let out = dest.join(&rel);
        if let Some(p) = out.parent() {
            fs::create_dir_all(p)?;
        }
        e.unpack(&out)?;
        n += 1;
    }
    Ok(n)
}

/// Replaces `dir` with a fresh extraction in `fresh`, carrying over the KEEP folders.
pub fn swap_in(fresh: &Path, dir: &Path) -> Result<()> {
    for k in KEEP {
        let old = dir.join(k);
        if old.exists() && !fresh.join(k).exists() {
            fs::rename(&old, fresh.join(k)).with_context(|| format!("keeping {k}"))?;
        }
    }
    let backup = dir.with_extension("old");
    let _ = fs::remove_dir_all(&backup);
    if dir.exists() {
        fs::rename(dir, &backup)?;
    }
    if let Err(e) = fs::rename(fresh, dir) {
        // roll back so the user is never left without a server folder
        let _ = fs::rename(&backup, dir);
        bail!("could not move the new files into place: {e}");
    }
    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

/// Downloads commit `sha` and installs it into `dir`.
pub fn install(dir: &Path, sha: &str, sink: &Sink) -> Result<()> {
    // An update replaces everything except `.tools/` and `runs/`: never do that to a git checkout.
    if dir.join(".git").exists() {
        bail!(
            "{} is a git checkout. An update replaces everything in the server folder, so choose a folder that belongs to the launcher (or remove its .git folder).",
            dir.display()
        );
    }
    let url = format!("https://codeload.github.com/{REPO}/tar.gz/{sha}");
    sink(format!("Downloading {REPO} @ {}…", &sha[..sha.len().min(10)]));
    let resp = agent(180).get(&url).call().map_err(|e| anyhow!("download failed: {e}"))?;
    let parent = dir.parent().ok_or_else(|| anyhow!("invalid server folder"))?;
    fs::create_dir_all(parent)?;
    let fresh = dir.with_extension("new");
    let _ = fs::remove_dir_all(&fresh);
    fs::create_dir_all(&fresh)?;
    let n = match extract_tar_gz(resp.into_reader(), &fresh) {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::remove_dir_all(&fresh);
            return Err(e.context("unpacking the download"));
        }
    };
    if !is_installed(&fresh) {
        let _ = fs::remove_dir_all(&fresh);
        bail!("the download does not look like ring-of-elysium-local");
    }
    swap_in(&fresh, dir)?;
    write_info(
        dir,
        &InstallInfo {
            sha: sha.to_string(),
            installed_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            translated: false,
            untranslated: None,
        },
    )?;
    sink(format!("Installed {n} files into {}", dir.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};

    fn tarball(files: &[(&str, &str)]) -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        for (p, c) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(c.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, p, c.as_bytes()).unwrap();
        }
        let raw = b.into_inner().unwrap();
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        std::io::Write::write_all(&mut gz, &raw).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn extracts_and_strips_the_top_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tarball(&[("repo-abc/requirements.txt", "x"), ("repo-abc/tools/windows_launch.py", "y"), ("repo-abc/a/b/c.txt", "z")]);
        let n = extract_tar_gz(&data[..], tmp.path()).unwrap();
        assert_eq!(n, 3);
        assert!(is_installed(tmp.path()));
        assert_eq!(fs::read_to_string(tmp.path().join("a/b/c.txt")).unwrap(), "z");
    }

    #[test]
    fn swap_keeps_tools_and_runs_but_replaces_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("server");
        fs::create_dir_all(dir.join(".tools/venv")).unwrap();
        fs::create_dir_all(dir.join("runs/r1")).unwrap();
        fs::write(dir.join("stale.txt"), "old").unwrap();
        let fresh = tmp.path().join("server.new");
        fs::create_dir_all(&fresh).unwrap();
        fs::write(fresh.join("requirements.txt"), "new").unwrap();
        swap_in(&fresh, &dir).unwrap();
        assert!(dir.join(".tools/venv").is_dir() && dir.join("runs/r1").is_dir());
        assert!(!dir.join("stale.txt").exists());
        assert_eq!(fs::read_to_string(dir.join("requirements.txt")).unwrap(), "new");
        assert!(!tmp.path().join("server.old").exists() && !fresh.exists());
    }

    #[test]
    fn parses_the_atom_feed_and_the_refs_advertisement() {
        let atom = "<feed>\n<updated>x</updated>\n<entry>\n<id>tag:github.com,2008:Grit::Commit/0636aaaf53d37505d8e9cc8c9cecf81084674c0e</id>\n<title>\n  Fix A &amp; B\n</title>\n<updated>2026-10-16T17:44:57Z</updated>\n</entry><entry><id>tag:github.com,2008:Grit::Commit/ffffffffffffffffffffffffffffffffffffffff</id></entry></feed>";
        let r = parse_atom(atom).unwrap();
        assert_eq!((r.sha.as_str(), r.message.as_str(), r.date.as_str()), ("0636aaaf53d37505d8e9cc8c9cecf81084674c0e", "Fix A & B", "2026-10-16T17:44:57Z"));
        let refs = "001e# service=git-upload-pack\n0000015a0636aaaf53d37505d8e9cc8c9cecf81084674c0e HEAD multi_ack\n003f1111111111111111111111111111111111111111 refs/heads/dev\n003f0636aaaf53d37505d8e9cc8c9cecf81084674c0e refs/heads/main\n";
        assert_eq!(parse_refs(refs).unwrap().sha, "0636aaaf53d37505d8e9cc8c9cecf81084674c0e");
        assert!(parse_atom("<feed></feed>").is_err());
    }

    #[test]
    fn refuses_to_overwrite_a_git_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join("my-edit.txt"), "precious").unwrap();
        let sink: Sink = std::sync::Arc::new(|_| {});
        let err = install(tmp.path(), "0123456789abcdef0123456789abcdef01234567", &sink).unwrap_err();
        assert!(err.to_string().contains("git checkout"));
        assert_eq!(fs::read_to_string(tmp.path().join("my-edit.txt")).unwrap(), "precious");
    }

    #[test]
    fn parses_github_commit_json() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"sha":"0636aaa1","commit":{"message":"Fix magazine capacity\n\nbody","committer":{"date":"2026-10-16T17:44:57Z"}}}"#,
        )
        .unwrap();
        let r = parse_remote(&v).unwrap();
        assert_eq!((r.sha.as_str(), r.message.as_str(), r.date.as_str()), ("0636aaa1", "Fix magazine capacity", "2026-10-16T17:44:57Z"));
        assert!(update_available(None, &r));
        assert!(!update_available(Some(&InstallInfo { sha: "0636aaa1".into(), ..Default::default() }), &r));
    }
}
