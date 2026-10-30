//! Keeps the launcher's own copy of the chosen server (`Source`) in sync with GitHub.
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

/// A server the launcher can install: laid out like ROE-LIVE's repository (`tools/windows_launch.py`), from GitHub or,
/// with `kit`, as a zip its author shares (RoE-Rivival: Google Drive links posted on Discord).
#[derive(Clone, Copy, Debug)]
pub struct Source {
    /// Saved in the config.
    pub id: &'static str,
    pub name: &'static str,
    pub repo: &'static str,
    pub branch: &'static str,
    /// The default folder under the launcher's data folder (ROE-LIVE keeps "server", from before there was a choice).
    pub folder: &'static str,
    /// The launcher's player name and map choice (`playername`) fit this server's files.
    pub names_and_map: bool,
    /// Files in the repository that are not unpacked (a zipped copy of the repository itself).
    pub skip: &'static [&'static str],
    pub about: &'static str,
    /// The maps the launcher offers (`maps`).
    pub maps: &'static [crate::maps::Map],
    /// The server takes the map as this option (`--map ID`) instead of the launcher's room-reply patch.
    pub map_option: Option<&'static str>,
    /// Match options for a new player and for "Reset to defaults" (flag -> value, "true" = switch on).
    pub default_options: &'static [(&'static str, &'static str)],
    /// With no loot catalog chosen, start with the newest bundled `config/windows-smoke-*.json` (Dione's loot).
    pub auto_loot_catalog: bool,
    /// Small fixes so the server starts with the launcher's Python: (file, text, replacement), applied after
    /// each download while the text is still there.
    pub fixes: &'static [(&'static str, &'static str, &'static str)],
    /// Installed from its author's zip instead of GitHub: the kit the launcher knows, and where the current one is
    /// announced (a file in the launcher's own repository, updated when the author posts a new kit).
    pub kit: Option<Kit>,
    pub kit_manifest: Option<&'static str>,
    /// A public Google Drive folder where the author puts each new kit (`RoE-kit*.zip`); the newest one is used.
    pub kit_folder: Option<&'static str>,
    /// Where people can see the server: its repository, or the author's folder.
    pub page: &'static str,
    /// Server Settings shows its own launcher's match switches (`switches`).
    pub switches: bool,
}

/// A server zip on a file host. `sha256` is checked after the download, so a changed file is never installed.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct KitInfo {
    pub url: String,
    /// Empty when not known before the download (a kit from the author's folder).
    #[serde(default)]
    pub sha256: String,
    /// What it is, in words ("Kit of 2026-10-29: lz4 fix").
    pub label: String,
    /// When its author posted it (shown as the date).
    #[serde(default)]
    pub posted: String,
    /// Tells this kit apart when there is no `sha256` (folder kits: file id and upload time).
    #[serde(default)]
    pub version: String,
    /// Manifest only: use this kit even when the author's folder has a newer one (to hold back a broken kit).
    #[serde(default)]
    pub pin: bool,
}

/// The kit compiled into the launcher (`KitInfo` with static text).
#[derive(Clone, Copy, Debug)]
pub struct Kit {
    pub url: &'static str,
    pub sha256: &'static str,
    pub label: &'static str,
    pub posted: &'static str,
}

impl Kit {
    pub fn info(&self) -> KitInfo {
        KitInfo { url: self.url.into(), sha256: self.sha256.into(), label: self.label.into(), posted: self.posted.into(), ..Default::default() }
    }
}

pub const ROE_LIVE: Source = Source {
    id: "roe-live",
    name: "ROE-LIVE by general_rj45",
    repo: "ROE-LIVE/ring-of-elysium-local",
    branch: "main",
    folder: "server",
    names_and_map: true,
    skip: &[],
    about: "The original community server, from GitHub: Training on Dione, your own player name, and the map picker's \
            experimental maps.",
    maps: &crate::maps::MAPS,
    map_option: None,
    default_options: &[
        ("--kit", "glider"),
        ("--mode", "cycle"),
        ("--cycle-scale", "3"),
        ("--reliable-udp", "true"),
        ("--idle-handoff", "true"),
        ("--winter-doors", "true"),
        ("--winter-windows", "true"),
        ("--target", "armored"),
    ],
    auto_loot_catalog: true,
    fixes: &[],
    kit: None,
    kit_manifest: None,
    kit_folder: None,
    page: "https://github.com/ROE-LIVE/ring-of-elysium-local",
    switches: false,
};

pub const ROE_RIVIVAL: Source = Source {
    id: "roe-rivival",
    name: "RoE-Rivival by livegaming2042",
    repo: "rapster2042/RoE-Rivival",
    branch: "main",
    folder: "server-roe-rivival",
    names_and_map: false,
    skip: &["RoE-kit.zip"],
    about: "livegaming2042's server, built on ROE-LIVE: bots that fight, armour, killfeed, vehicles, and Dione, Europa \
            and Vera in every mode. Shared as a zip in his Google Drive folder; the launcher fetches the newest kit there. Its own \
            lobby keeps the original player name. Its two-client duo mode is not offered.",
    maps: &crate::maps::RIVIVAL_MAPS,
    map_option: Some("--map"),
    // what its own launcher (RoE-Launcher.ps1) starts with
    default_options: &[
        ("--mode", "cycle"),
        ("--room-service", "true"),
        ("--winter-doors", "true"),
        ("--winter-windows", "true"),
        ("--loot-randomize", "true"),
        ("--live-control", "true"),
        ("--no-storm", "true"),
        ("--shared-bots", "20"),
        ("--vehicle-fleet", "true"),
    ],
    auto_loot_catalog: false,
    // Python 3.12 (Windows) accepts a lone % in argparse help, 3.14 (Linux) refuses to start
    fixes: &[("tools/windows_launch.py", "(500=50% —", "(500=50%% —")],
    // livegaming2042's post of 2026-10-29 16:05 on the community Discord ("only pulls modules from its own folders")
    kit: Some(Kit {
        url: "https://drive.usercontent.google.com/download?id=1eaHaiRn4kwPyPHHXu5AxwgKTZhWaO_N7&export=download&confirm=t",
        sha256: "21d06aa7c92f264222d02f6e21b94316882beb54bbc81f8d4b1727c96af42d1f",
        label: "Kit of 2026-10-29 (Python modules fix)",
        posted: "2026-10-29",
    }),
    kit_manifest: Some("https://raw.githubusercontent.com/darknetcs/roe-launcher/main/sources/roe-rivival.json"),
    // shared by livegaming2042 on 2026-10-29 for new kits (he can't commit to GitHub reliably)
    kit_folder: Some("1cNJ0osudbt5VrQLhQFCvNvaV25eR96jR"),
    page: "https://drive.google.com/drive/folders/1cNJ0osudbt5VrQLhQFCvNvaV25eR96jR",
    switches: true,
};

pub const KOWALSKI: Source = Source {
    id: "kowalski",
    name: "ROE-LIVE fork by |NLG|Kowalski",
    repo: "streamnestapp/ring-of-elysium-local",
    // his work is on the default branch; his main is still ROE-LIVE's (2026-10-30)
    branch: "mapas-locais",
    folder: "server-kowalski",
    names_and_map: true,
    skip: &[],
    about: "|NLG|Kowalski's fork of ROE-LIVE, from GitHub: work on real multiplayer — a shared room for several PCs \
            (Match options → Shared room, with your LAN host) with player-versus-player damage (Shared PvP), others see \
            you swim, glide and aim, airdrops and drowning. Training on Dione, your own player name.",
    maps: &crate::maps::MAPS,
    map_option: None,
    default_options: ROE_LIVE.default_options,
    auto_loot_catalog: true,
    fixes: &[],
    kit: None,
    kit_manifest: None,
    kit_folder: None,
    page: "https://github.com/streamnestapp/ring-of-elysium-local",
    switches: false,
};

/// The server a new player starts with (owner's choice, 2026-10-29).
pub const DEFAULT: Source = ROE_RIVIVAL;

/// In the order the launcher offers them.
pub const SOURCES: [Source; 3] = [ROE_RIVIVAL, ROE_LIVE, KOWALSKI];

/// The source saved as `id`; the default for anything unknown.
pub fn source(id: &str) -> Source {
    SOURCES.iter().find(|s| s.id == id).copied().unwrap_or(DEFAULT)
}

/// The same source = the same id.
impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Source {}

impl Source {
    pub fn url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }

    pub fn defaults(&self) -> std::collections::BTreeMap<String, String> {
        self.default_options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// Applies `fixes` in `dir`; returns the files changed.
    pub fn apply_fixes(&self, dir: &Path) -> Result<Vec<&'static str>> {
        let mut changed = Vec::new();
        for (file, from, to) in self.fixes {
            let path = dir.join(file);
            let Ok(text) = fs::read_to_string(&path) else { continue };
            if text.contains(from) {
                fs::write(&path, text.replace(from, to))?;
                changed.push(*file);
            }
        }
        Ok(changed)
    }
}

/// Test switch: `ROE_SERVER_KIT=<zip>` installs the chosen source from a local zip of its files instead of GitHub
/// (RoE-Rivival's newest code is shared only as `RoE-kit.zip`). Its "commit" is `kit-` and the zip's SHA-256.
pub const KIT_ENV: &str = "ROE_SERVER_KIT";
/// What a kit carries for its packer only: bundled runtimes, local data and logs, their settings, the kit itself.
const KIT_SKIP: [&str; 8] = [".tools", "runtime", "runs", "logs", "private", "state", "launcher-settings.json", "RoE-kit.zip"];

pub fn local_kit() -> Option<PathBuf> {
    std::env::var_os(KIT_ENV).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// ROE-LIVE's `requirements.txt`, for a fork that leaves the file out (RoE-Rivival ships the same packages only
/// inside its zip).
const FALLBACK_REQUIREMENTS: &str = "cryptography==50.0.1\nlz4==4.4.5\ncffi==2.1.1\npycparser==3.0\n";
/// The session key every one of these servers uses (`private/candidate-001.key`, the text "BRING_BACK_ROE!!"): in
/// ROE-LIVE's repository, left out of RoE-Rivival's (its .gitignore drops `private/`).
const SESSION_KEY: &[u8] = b"BRING_BACK_ROE!!";
/// Folders that survive an update: the folders upstream's .gitignore treats as the user's local data.
/// `runtime/` holds the saved profiles (local-account/, local-accounts/NAME/ with the chosen character and
/// cosmetics) and caches; `reports/` the exported test reports; `runs/` the session logs; `.tools/` Python/.NET.
pub const KEEP: [&str; 4] = [".tools", "runs", "runtime", "reports"];
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
    /// The repository it came from; empty = ROE-LIVE (installed before there was a choice).
    #[serde(default)]
    pub repo: String,
    pub installed_at: u64,
    pub translated: bool,
    pub untranslated: Option<u32>,
}

pub(crate) fn agent(secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(secs)).user_agent("roe-launcher").build()
}

/// Latest commit on the branch. Tries three sources so one rate limit never blocks updates:
/// the public Atom feed (no quota), the REST API (60 requests/hour/IP), git smart-HTTP refs.
pub fn remote_head(src: &Source) -> Result<Remote> {
    if let Some(kit) = local_kit() {
        return remote_via_kit(&kit);
    }
    if let Some(kit) = current_kit(src) {
        return Ok(kit_remote(&kit));
    }
    let mut errs = Vec::new();
    for (name, f) in [("atom feed", remote_via_atom as fn(&Source) -> Result<Remote>), ("GitHub API", remote_via_api), ("git refs", remote_via_refs)] {
        match f(src) {
            Ok(r) => return Ok(r),
            Err(e) => errs.push(format!("{name}: {e}")),
        }
    }
    bail!("could not reach GitHub ({})", errs.join("; "))
}

/// The kit `src` should run, first that can be read: a kit pinned in its manifest, the newest kit in its author's
/// folder, the manifest's kit, the one compiled in. None = not a kit source.
pub fn current_kit(src: &Source) -> Option<KitInfo> {
    let built_in = src.kit?.info();
    let announced = src.kit_manifest.and_then(|url| {
        let v: KitInfo = serde_json::from_str(&get_text(url).ok()?).ok()?;
        (v.sha256.len() == 64 && v.sha256.chars().all(|c| c.is_ascii_hexdigit()) && v.url.starts_with("https://")).then_some(v)
    });
    if let Some(pinned) = announced.clone().filter(|k| k.pin) {
        return Some(pinned);
    }
    Some(src.kit_folder.and_then(|f| folder_kit(f).ok()).or(announced).unwrap_or(built_in))
}

/// A kit's "commit": `kit-` and the start of its SHA-256 (the same for a local copy of the same zip), or for a
/// folder kit its file id and upload time.
pub fn kit_remote(kit: &KitInfo) -> Remote {
    let id = if kit.sha256.len() >= 24 { kit.sha256.to_lowercase()[..24].to_string() } else { kit.version.clone() };
    Remote { sha: format!("kit-{id}"), message: kit.label.clone(), date: kit.posted.clone() }
}

/// (file id, name) of each file in a public folder's `embeddedfolderview` page.
pub fn folder_entries(page: &str) -> Vec<(String, String)> {
    page.split("<div class=\"flip-entry\" id=\"entry-")
        .skip(1)
        .filter_map(|e| {
            let id = e.split('"').next()?.to_string();
            let name = between(e, "<div class=\"flip-entry-title\">", "</div>")?;
            Some((id, unescape_xml(name)))
        })
        .collect()
}

/// The newest `RoE-kit*.zip` in a public Google Drive folder. The folder page shows only relative dates, so each
/// candidate's upload time comes from its download's `Last-Modified`.
pub fn folder_kit(folder: &str) -> Result<KitInfo> {
    let page = get_text(&format!("https://drive.google.com/embeddedfolderview?id={folder}"))?;
    let mut best: Option<(u64, String, String)> = None;
    for (id, name) in folder_entries(&page) {
        let lower = name.to_lowercase();
        if !(lower.starts_with("roe-kit") && lower.ends_with(".zip")) {
            continue;
        }
        let url = drive_download_url(&id);
        let Ok(resp) = agent(20).head(&url).call() else { continue };
        let Some(when) = resp.header("last-modified").and_then(http_date) else { continue };
        if best.as_ref().map_or(true, |b| when > b.0) {
            best = Some((when, id, name));
        }
    }
    let (when, id, name) = best.ok_or_else(|| anyhow!("no RoE-kit zip in the folder"))?;
    let (date, time) = ymd_hm(when);
    Ok(KitInfo {
        url: drive_download_url(&id),
        sha256: String::new(),
        label: format!("{name} uploaded {date} {time} UTC"),
        posted: date,
        version: format!("{}-{when}", &id[..id.len().min(8)]),
        pin: false,
    })
}

fn drive_download_url(id: &str) -> String {
    format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t")
}

/// Seconds since 1970 of an HTTP date ("Thu, 29 Oct 2026 17:56:07 GMT").
pub fn http_date(s: &str) -> Option<u64> {
    let p: Vec<&str> = s.split_whitespace().collect();
    let (day, year): (i64, i64) = (p.get(1)?.parse().ok()?, p.get(3)?.parse().ok()?);
    let name = *p.get(2)?;
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|m| *m == name)? as i64 + 1;
    let hms: Vec<i64> = p.get(4)?.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let days = 365 * y + y / 4 - y / 100 + y / 400 + (153 * m + 2) / 5 + day - 1 - 719468;
    u64::try_from(days * 86400 + hms.first()? * 3600 + hms.get(1)? * 60 + hms.get(2)?).ok()
}

/// ("2026-10-29", "17:56") of seconds since 1970, UTC.
fn ymd_hm(secs: u64) -> (String, String) {
    let days = (secs / 86400) as i64 + 719468;
    let era = days.div_euclid(146097);
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = secs % 86400;
    (format!("{y:04}-{m:02}-{d:02}"), format!("{:02}:{:02}", t / 3600, t % 3600 / 60))
}

/// Downloads `kit` to `dest` and checks its SHA-256. Google Drive answers a large file with a "can't scan it for
/// viruses" page first; its form gives the real download.
pub fn download_kit(kit: &KitInfo, dest: &Path, sink: &Sink) -> Result<()> {
    let mut resp = agent(900).get(&kit.url).call().map_err(|e| anyhow!("download failed: {e}"))?;
    if resp.content_type().starts_with("text/html") {
        let page = resp.into_string()?;
        let field = |name: &str| between(&page, &format!("name=\"{name}\" value=\""), "\"").map(str::to_string);
        let (Some(id), Some(uuid)) = (field("id"), field("uuid")) else { bail!("the download link answered with a web page instead of the kit") };
        let url = format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t&uuid={uuid}");
        resp = agent(900).get(&url).call().map_err(|e| anyhow!("download failed: {e}"))?;
    }
    let total: Option<u64> = resp.header("content-length").and_then(|v| v.parse().ok());
    let mut reader = resp.into_reader();
    let mut out = fs::File::create(dest)?;
    let (mut got, mut next) = (0u64, 50u64 << 20);
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut out, &buf[..n])?;
        got += n as u64;
        if got >= next {
            next += 50 << 20;
            sink(match total {
                Some(t) => format!("  {} of {} MB", got >> 20, t >> 20),
                None => format!("  {} MB", got >> 20),
            });
        }
    }
    drop(out);
    let have = crate::runtime::sha256_hex(dest)?;
    if kit.sha256.is_empty() {
        // a folder kit: nothing announced to compare with; unpacking still refuses anything that isn't a server
        sink(format!("Kit downloaded, SHA-256 {have}."));
        return Ok(());
    }
    if !have.eq_ignore_ascii_case(&kit.sha256) {
        let _ = fs::remove_file(dest);
        bail!("the downloaded kit is not the one announced (SHA-256 {have}): its author may have replaced the file; try again later");
    }
    Ok(())
}

fn remote_via_kit(zip: &Path) -> Result<Remote> {
    let sha = crate::runtime::sha256_hex(zip).with_context(|| format!("reading {} ({KIT_ENV})", zip.display()))?;
    Ok(Remote { sha: format!("kit-{}", &sha[..24]), message: format!("Local kit {} ({KIT_ENV})", zip.display()), date: String::new() })
}

/// Unpacks a kit zip into `dest`, without `KIT_SKIP` and `__pycache__`. Entries outside `dest` are refused.
pub fn extract_kit(zip_path: &Path, dest: &Path) -> Result<usize> {
    let mut archive = zip::ZipArchive::new(fs::File::open(zip_path)?).with_context(|| format!("{} is not a valid zip", zip_path.display()))?;
    let mut n = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { bail!("unsafe path in {}: {}", zip_path.display(), entry.name()) };
        let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
        if KIT_SKIP.contains(&first.as_str()) || rel.components().any(|c| c.as_os_str() == "__pycache__") || entry.is_dir() {
            continue;
        }
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut entry, &mut fs::File::create(&out)?)?;
        n += 1;
    }
    Ok(n)
}

fn get_text(url: &str) -> Result<String> {
    Ok(agent(20).get(url).set("Accept", "*/*").call().map_err(|e| anyhow!("{e}"))?.into_string()?)
}

fn remote_via_atom(src: &Source) -> Result<Remote> {
    parse_atom(&get_text(&format!("https://github.com/{}/commits/{}.atom", src.repo, src.branch))?)
}

fn remote_via_api(src: &Source) -> Result<Remote> {
    let v: serde_json::Value = agent(20)
        .get(&format!("https://api.github.com/repos/{}/commits/{}", src.repo, src.branch))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| anyhow!("{e}"))?
        .into_json()?;
    parse_remote(&v)
}

fn remote_via_refs(src: &Source) -> Result<Remote> {
    let text = get_text(&format!("https://github.com/{}.git/info/refs?service=git-upload-pack", src.repo))?;
    parse_refs(&text, src.branch)
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

/// `<sha> refs/heads/<branch>` out of the smart-HTTP advertisement (no message/date available).
pub fn parse_refs(text: &str, branch: &str) -> Result<Remote> {
    let marker = format!(" refs/heads/{branch}");
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
    missing(dir).is_none()
}

/// Why the server folder isn't usable, in words for the log; None = it is.
pub fn missing(dir: &Path) -> Option<String> {
    if !dir.is_dir() {
        return Some(format!("{} doesn't exist yet", dir.display()));
    }
    ["requirements.txt", "tools/windows_launch.py"]
        .iter()
        .find(|f| !dir.join(f).is_file())
        .map(|f| format!("{} is incomplete: {f} is missing", dir.display()))
}

/// What an update leaves next to the server folder when it can't clean up (on Windows: a file still in use, e.g.
/// by antivirus): `<dir>.old` (the previous files; the user's data was moved out) and an unpacked `<dir>.new`.
/// Removes them and returns a line for the log per folder, removed or not. `<dir>.new` only when it is over an
/// hour old, so a download running in another launcher window is left alone. Call it while no server runs.
pub fn remove_leftovers(dir: &Path) -> Vec<String> {
    let hour_old = |p: &Path| {
        fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() > 3600)
    };
    let mut out = Vec::new();
    for (ext, check_age) in [("old", false), ("new", true)] {
        let p = dir.with_extension(ext);
        if !p.is_dir() || (check_age && !hour_old(&p)) {
            continue;
        }
        out.push(match fs::remove_dir_all(&p) {
            Ok(()) => format!("Removed {}, left over from an earlier server update.", p.display()),
            Err(e) => format!(
                "Could not remove {} ({e}), left over from a server update. It holds no saved data: delete it by hand, \
                 or the launcher tries again next start.",
                p.display()
            ),
        });
    }
    out
}

pub fn update_available(info: Option<&InstallInfo>, remote: &Remote) -> bool {
    info.map(|i| i.sha != remote.sha).unwrap_or(true)
}

/// Unpacks a GitHub tarball into `dest`, dropping the leading `<repo>-<sha>/` folder and the `skip` paths.
pub fn extract_tar_gz<R: Read>(reader: R, dest: &Path, skip: &[&str]) -> Result<usize> {
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
        if skip.iter().any(|s| rel == Path::new(s)) {
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

/// Adds what a fork's repository leaves out but the launcher needs: `requirements.txt` and the session key.
/// Returns what was added. Only for a server checkout (`tools/windows_launch.py`).
pub fn fill_in(dir: &Path) -> Result<Vec<&'static str>> {
    let mut added = Vec::new();
    if !dir.join("tools/windows_launch.py").is_file() {
        return Ok(added);
    }
    let req = dir.join("requirements.txt");
    if !req.exists() {
        fs::write(&req, FALLBACK_REQUIREMENTS)?;
        added.push("requirements.txt");
    }
    let key = dir.join("private/candidate-001.key");
    if !key.exists() {
        fs::create_dir_all(dir.join("private"))?;
        fs::write(&key, SESSION_KEY)?;
        added.push("private/candidate-001.key");
    }
    Ok(added)
}

/// Downloads commit `sha` of `src` and installs it into `dir`.
pub fn install(dir: &Path, src: &Source, sha: &str, sink: &Sink) -> Result<()> {
    // An update replaces everything except `.tools/` and `runs/`: never do that to a git checkout.
    if dir.join(".git").exists() {
        bail!(
            "{} is a git checkout. An update replaces everything in the server folder, so choose a folder that belongs to the launcher (or remove its .git folder).",
            dir.display()
        );
    }
    // never swap one server for another in the same folder: the kept data (profiles, runtime) belongs to the first
    if let Some(had) = read_info(dir).map(|i| if i.repo.is_empty() { ROE_LIVE.repo.to_string() } else { i.repo }).filter(|r| r != src.repo) {
        bail!("{} holds {had}, not {}. Choose another folder for this server.", dir.display(), src.repo);
    }
    let repo = src.repo;
    let parent = dir.parent().ok_or_else(|| anyhow!("invalid server folder"))?;
    fs::create_dir_all(parent)?;
    let fresh = dir.with_extension("new");
    let _ = fs::remove_dir_all(&fresh);
    fs::create_dir_all(&fresh)?;
    let unpacked = if let (true, Some(local)) = (sha.starts_with("kit-"), local_kit()) {
        sink(format!("Installing {} from the local kit {} ({KIT_ENV}, for testing)…", src.name, local.display()));
        extract_kit(&local, &fresh)
    } else if sha.starts_with("kit-") {
        let kit = current_kit(src).filter(|k| kit_remote(k).sha == sha).ok_or_else(|| anyhow!("the server kit changed meanwhile: check for updates again"))?;
        sink(format!("Downloading {} — {} — from its author's link (about 250 MB)…", src.name, kit.label));
        let zip = dir.with_extension("kit.zip");
        let got = download_kit(&kit, &zip, sink).and_then(|()| extract_kit(&zip, &fresh));
        let _ = fs::remove_file(&zip);
        got
    } else {
        let url = format!("https://codeload.github.com/{repo}/tar.gz/{sha}");
        sink(format!("Downloading {repo} @ {}…", &sha[..sha.len().min(10)]));
        match agent(180).get(&url).call() {
            Ok(resp) => extract_tar_gz(resp.into_reader(), &fresh, src.skip),
            Err(e) => Err(anyhow!("download failed: {e}")),
        }
    };
    let n = match unpacked.and_then(|n| Ok((n, fill_in(&fresh)?, src.apply_fixes(&fresh)?))) {
        Ok((n, added, fixed)) => {
            if !added.is_empty() {
                sink(format!("{repo} doesn't include {}: added ROE-LIVE's.", added.join(" and ")));
            }
            if !fixed.is_empty() {
                sink(format!("Fixed {} so it starts with this Python.", fixed.join(", ")));
            }
            n
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&fresh);
            return Err(e.context("unpacking the download"));
        }
    };
    if !is_installed(&fresh) {
        let _ = fs::remove_dir_all(&fresh);
        bail!("the download does not look like a server for the launcher (no tools/windows_launch.py)");
    }
    swap_in(&fresh, dir)?;
    // swap_in tries to delete the previous files; say so when that failed instead of leaving a silent copy
    let old = dir.with_extension("old");
    if old.exists() {
        sink(format!(
            "The update worked, but {} could not be removed yet (a file in it may still be in use). It holds no saved data; \
             the launcher tries again next start.",
            old.display()
        ));
    }
    write_info(
        dir,
        &InstallInfo {
            sha: sha.to_string(),
            repo: repo.to_string(),
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
        let n = extract_tar_gz(&data[..], tmp.path(), &[]).unwrap();
        assert_eq!(n, 3);
        assert!(is_installed(tmp.path()));
        assert_eq!(fs::read_to_string(tmp.path().join("a/b/c.txt")).unwrap(), "z");
    }

    #[test]
    fn swap_keeps_the_users_data_but_replaces_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("server");
        fs::create_dir_all(dir.join(".tools/venv")).unwrap();
        fs::create_dir_all(dir.join("runs/r1")).unwrap();
        fs::create_dir_all(dir.join("runtime/local-account")).unwrap();
        fs::write(dir.join("runtime/local-account/cosmetics.json"), "{\"outfit\":7}").unwrap();
        fs::create_dir_all(dir.join("reports")).unwrap();
        fs::write(dir.join("reports/w19.zip"), "r").unwrap();
        fs::write(dir.join("stale.txt"), "old").unwrap();
        let fresh = tmp.path().join("server.new");
        fs::create_dir_all(&fresh).unwrap();
        fs::write(fresh.join("requirements.txt"), "new").unwrap();
        swap_in(&fresh, &dir).unwrap();
        assert!(dir.join(".tools/venv").is_dir() && dir.join("runs/r1").is_dir());
        // the saved profile (character and cosmetics) survives the update
        assert_eq!(fs::read_to_string(dir.join("runtime/local-account/cosmetics.json")).unwrap(), "{\"outfit\":7}");
        assert!(dir.join("reports/w19.zip").is_file());
        assert!(!dir.join("stale.txt").exists());
        assert_eq!(fs::read_to_string(dir.join("requirements.txt")).unwrap(), "new");
        assert!(!tmp.path().join("server.old").exists() && !fresh.exists());
    }

    #[test]
    fn says_why_the_server_folder_is_unusable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("server");
        assert!(missing(&dir).unwrap().ends_with("doesn't exist yet"));
        fs::create_dir_all(dir.join("tools")).unwrap();
        fs::write(dir.join("requirements.txt"), "x").unwrap();
        assert!(missing(&dir).unwrap().ends_with("tools/windows_launch.py is missing"));
        fs::write(dir.join("tools/windows_launch.py"), "x").unwrap();
        assert_eq!(missing(&dir), None);
        assert!(is_installed(&dir));
    }

    #[test]
    fn cleans_up_what_an_update_left_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("server");
        fs::create_dir_all(&dir).unwrap();
        assert!(remove_leftovers(&dir).is_empty());
        fs::create_dir_all(tmp.path().join("server.old/tools")).unwrap();
        fs::write(tmp.path().join("server.old/tools/windows_launch.py"), "x").unwrap();
        // a fresh server.new may be another window's download in progress: left alone
        fs::create_dir_all(tmp.path().join("server.new")).unwrap();
        let log = remove_leftovers(&dir);
        assert_eq!(log.len(), 1, "{log:?}");
        assert!(log[0].starts_with("Removed ") && log[0].contains("server.old"));
        assert!(!tmp.path().join("server.old").exists() && tmp.path().join("server.new").exists());
        assert!(dir.is_dir());
    }

    #[test]
    fn parses_the_atom_feed_and_the_refs_advertisement() {
        let atom = "<feed>\n<updated>x</updated>\n<entry>\n<id>tag:github.com,2008:Grit::Commit/0636aaaf53d37505d8e9cc8c9cecf81084674c0e</id>\n<title>\n  Fix A &amp; B\n</title>\n<updated>2026-10-16T17:44:57Z</updated>\n</entry><entry><id>tag:github.com,2008:Grit::Commit/ffffffffffffffffffffffffffffffffffffffff</id></entry></feed>";
        let r = parse_atom(atom).unwrap();
        assert_eq!((r.sha.as_str(), r.message.as_str(), r.date.as_str()), ("0636aaaf53d37505d8e9cc8c9cecf81084674c0e", "Fix A & B", "2026-10-16T17:44:57Z"));
        let refs = "001e# service=git-upload-pack\n0000015a0636aaaf53d37505d8e9cc8c9cecf81084674c0e HEAD multi_ack\n003f1111111111111111111111111111111111111111 refs/heads/dev\n003f0636aaaf53d37505d8e9cc8c9cecf81084674c0e refs/heads/main\n";
        assert_eq!(parse_refs(refs, "main").unwrap().sha, "0636aaaf53d37505d8e9cc8c9cecf81084674c0e");
        assert!(parse_atom("<feed></feed>").is_err());
    }

    #[test]
    fn refuses_to_overwrite_a_git_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join("my-edit.txt"), "precious").unwrap();
        let sink: Sink = std::sync::Arc::new(|_| {});
        let err = install(tmp.path(), &ROE_LIVE, "0123456789abcdef0123456789abcdef01234567", &sink).unwrap_err();
        assert!(err.to_string().contains("git checkout"));
        assert_eq!(fs::read_to_string(tmp.path().join("my-edit.txt")).unwrap(), "precious");
    }

    #[test]
    fn skips_the_listed_files() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tarball(&[("repo-abc/RoE-kit.zip", "big"), ("repo-abc/tools/windows_launch.py", "y"), ("repo-abc/sub/RoE-kit.zip", "kept")]);
        assert_eq!(extract_tar_gz(&data[..], tmp.path(), ROE_RIVIVAL.skip).unwrap(), 2);
        assert!(!tmp.path().join("RoE-kit.zip").exists());
        assert!(tmp.path().join("sub/RoE-kit.zip").is_file());
    }

    #[test]
    fn a_fork_without_requirements_or_key_gets_roe_lives() {
        let tmp = tempfile::tempdir().unwrap();
        // not a server checkout: nothing is added
        assert!(fill_in(tmp.path()).unwrap().is_empty());
        fs::create_dir_all(tmp.path().join("tools")).unwrap();
        fs::write(tmp.path().join("tools/windows_launch.py"), "x").unwrap();
        assert!(!is_installed(tmp.path()));
        assert_eq!(fill_in(tmp.path()).unwrap(), ["requirements.txt", "private/candidate-001.key"]);
        assert!(is_installed(tmp.path()));
        assert!(fs::read_to_string(tmp.path().join("requirements.txt")).unwrap().contains("cryptography=="));
        assert_eq!(fs::read(tmp.path().join("private/candidate-001.key")).unwrap(), b"BRING_BACK_ROE!!");
        // the repository's own files are never replaced
        fs::write(tmp.path().join("requirements.txt"), "own").unwrap();
        assert!(fill_in(tmp.path()).unwrap().is_empty());
        assert_eq!(fs::read_to_string(tmp.path().join("requirements.txt")).unwrap(), "own");
    }

    #[test]
    fn refuses_a_folder_that_holds_the_other_server() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("keep.txt"), "x").unwrap();
        // installed before there was a choice: no repo recorded = ROE-LIVE
        write_info(tmp.path(), &InstallInfo { sha: "a".into(), ..Default::default() }).unwrap();
        let sink: Sink = std::sync::Arc::new(|_| {});
        let err = install(tmp.path(), &ROE_RIVIVAL, "0123456789abcdef0123456789abcdef01234567", &sink).unwrap_err();
        assert!(err.to_string().contains("holds ROE-LIVE/ring-of-elysium-local"), "{err}");
        assert!(tmp.path().join("keep.txt").is_file());
    }

    #[test]
    fn a_kit_is_unpacked_without_its_packers_own_files() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("RoE-kit.zip");
        let mut z = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in [
            ("tools/windows_launch.py", "x"),
            ("server/directory/a.py", "y"),
            ("server/directory/__pycache__/a.cpython-312.pyc", "c"),
            (".tools/python/python.exe", "p"),
            ("private/candidate-001.key", "k"),
            ("logs/services.out.log", "l"),
            ("runtime/zone-set-state.json", "r"),
            ("launcher-settings.json", "{}"),
        ] {
            z.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut z, body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let dest = tmp.path().join("out");
        assert_eq!(extract_kit(&zip_path, &dest).unwrap(), 2);
        assert!(dest.join("tools/windows_launch.py").is_file() && dest.join("server/directory/a.py").is_file());
        for gone in [".tools", "private", "logs", "runtime", "launcher-settings.json", "server/directory/__pycache__"] {
            assert!(!dest.join(gone).exists(), "{gone}");
        }
        assert_eq!(remote_via_kit(&zip_path).unwrap().sha.len(), "kit-".len() + 24);
    }

    #[test]
    fn a_kit_is_known_by_its_hash() {
        let kit = ROE_RIVIVAL.kit.unwrap().info();
        assert_eq!(kit.sha256.len(), 64);
        let r = kit_remote(&kit);
        assert_eq!(r.sha, format!("kit-{}", &kit.sha256[..24]));
        assert_eq!(r.date, "2026-10-29");
        let parsed: KitInfo = serde_json::from_str(include_str!("../../../sources/roe-rivival.json")).unwrap();
        assert!(parsed.url.starts_with("https://") && parsed.sha256.len() == 64, "the published manifest parses");
        assert!(ROE_LIVE.kit.is_none() && current_kit(&ROE_LIVE).is_none());
        assert!(!parsed.pin, "the published manifest doesn't hold back the author's folder");
    }

    #[test]
    fn reads_the_authors_drive_folder() {
        let page = r#"<div class="flip-entries"><div class="flip-entry" id="entry-1Yfd5hp-ZSJln9dGMy2dqEc7QGHT22eTs" tabindex="0" role="link"><div class="flip-entry-info"><a href="https://drive.google.com/file/d/1Yfd5hp-ZSJln9dGMy2dqEc7QGHT22eTs/view?usp=drive_web" target="_blank"><div class="flip-entry-title">RoE-kit.zip</div></a></div><div class="flip-entry-last-modified"><div>10:56 am</div></div></div><div class="flip-entry" id="entry-abc"><div class="flip-entry-title">notes &amp; readme.txt</div></div></div>"#;
        assert_eq!(
            folder_entries(page),
            [("1Yfd5hp-ZSJln9dGMy2dqEc7QGHT22eTs".to_string(), "RoE-kit.zip".to_string()), ("abc".to_string(), "notes & readme.txt".to_string())]
        );
        let t = http_date("Thu, 29 Oct 2026 17:56:07 GMT").unwrap();
        assert_eq!(ymd_hm(t), ("2026-10-29".to_string(), "17:56".to_string()));
        assert_eq!(http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert!(http_date("Feb 29 2026 nonsense").is_none());
        // a folder kit has no hash: its file id and upload time tell it apart
        let k = KitInfo { version: "1Yfd5hp--1793296567".into(), ..Default::default() };
        assert_eq!(kit_remote(&k).sha, "kit-1Yfd5hp--1793296567");
    }

    #[test]
    #[ignore = "reads livegaming2042's Google Drive folder"]
    fn live_folder_has_a_kit() {
        let k = folder_kit(ROE_RIVIVAL.kit_folder.unwrap()).unwrap();
        println!("{k:?}");
        assert!(k.url.contains("drive.usercontent.google.com") && k.version.contains('-') && k.posted.len() == 10);
    }

    #[test]
    #[ignore = "downloads about 250 MB from Google Drive"]
    fn live_kit_download_matches_its_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let sink: Sink = std::sync::Arc::new(|l| println!("{l}"));
        let kit = current_kit(&ROE_RIVIVAL).unwrap();
        download_kit(&kit, &tmp.path().join("kit.zip"), &sink).unwrap();
        assert!(extract_kit(&tmp.path().join("kit.zip"), &tmp.path().join("out")).unwrap() > 100);
    }

    #[test]
    fn the_python_fix_is_applied_once() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("tools")).unwrap();
        let f = tmp.path().join("tools/windows_launch.py");
        fs::write(&f, "help='Минимум HP участника (500=50% — наблюдение без смерти)'").unwrap();
        assert_eq!(ROE_RIVIVAL.apply_fixes(tmp.path()).unwrap(), ["tools/windows_launch.py"]);
        assert!(fs::read_to_string(&f).unwrap().contains("(500=50%% —"));
        assert!(ROE_RIVIVAL.apply_fixes(tmp.path()).unwrap().is_empty());
        assert!(ROE_LIVE.apply_fixes(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn unknown_sources_fall_back_to_the_default() {
        assert_eq!(source("roe-live"), ROE_LIVE);
        assert_eq!(source("nope"), ROE_RIVIVAL);
        assert_eq!(source(""), DEFAULT);
        let ids: Vec<_> = SOURCES.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["roe-rivival", "roe-live", "kowalski"]);
        // people tell the servers apart by their authors
        assert!(ROE_RIVIVAL.name.ends_with("by livegaming2042") && ROE_LIVE.name.ends_with("by general_rj45"));
        assert!(KOWALSKI.name.ends_with("by |NLG|Kowalski") && KOWALSKI.kit.is_none() && KOWALSKI.names_and_map);
        let folders: std::collections::HashSet<_> = SOURCES.iter().map(|s| s.folder).collect();
        assert_eq!(folders.len(), SOURCES.len(), "each source needs its own folder");
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
