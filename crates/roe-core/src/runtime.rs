//! Prepares what the server needs: Python with the pinned deps, and the .NET 8 SDK.
//! Linux: the system Python 3.11+ with a venv, as play-server.sh does.
//! Windows: the portable Python and .NET in `.tools`, the same layout as upstream's bootstrap.ps1
//! (so play.cmd and the launcher can share one folder), installed without PowerShell.
use crate::Sink;
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub python: PathBuf,
    pub dotnet: PathBuf,
    /// Set when .NET lives in `.tools/dotnet` (needs DOTNET_ROOT).
    pub dotnet_root: Option<PathBuf>,
}

/// The portable Python upstream pins for Windows.
pub const PYTHON_EMBED_URL: &str = "https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip";
pub const PYTHON_EMBED_SHA256: &str = "4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3";
/// Embeddable Python ignores PYTHONPATH; this file sets its search path (same lines as bootstrap.ps1).
pub const PYTHON_PTH: &str = "python312.zip\r\n.\r\nLib\\site-packages\r\n..\\..\\tools\r\n..\\..\\server\\directory\r\nimport site\r\n";
pub const GET_PIP_URL: &str = "https://bootstrap.pypa.io/get-pip.py";
/// Microsoft's permanent link to the newest .NET 8 SDK for Windows x64.
pub const DOTNET_SDK_ZIP_URL: &str = "https://aka.ms/dotnet/8.0/dotnet-sdk-win-x64.zip";

/// The Python the server runs with, inside the server folder.
pub fn server_python(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join(".tools/python/python.exe")
    } else {
        dir.join(".tools/venv/bin/python")
    }
}

fn out_of(cmd: &mut Command) -> Option<String> {
    let o = crate::process::hide_window(cmd).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// A Python >= 3.11 on PATH. Not on Windows: there `python` may be the Microsoft Store stub,
/// and the server always uses its own portable Python.
pub fn find_python() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    for name in ["python3", "python3.13", "python3.12", "python3.11", "python"] {
        let ok = out_of(Command::new(name).args(["-c", "import sys;print(sys.version_info>=(3,11))"]));
        if ok.as_deref() == Some("True") {
            let path = out_of(Command::new(name).args(["-c", "import sys;print(sys.executable)"]));
            return Some(PathBuf::from(path.unwrap_or_else(|| name.to_string())));
        }
    }
    None
}

/// A system .NET with an 8.x SDK, else the copy in `.tools/dotnet`.
pub fn find_dotnet(tools: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    if let Some(list) = out_of(Command::new("dotnet").arg("--list-sdks")) {
        if list.lines().any(|l| l.starts_with("8.")) {
            let p = out_of(Command::new("sh").args(["-c", "command -v dotnet"]))?;
            return Some((PathBuf::from(p), None));
        }
    }
    let local = tools.join("dotnet");
    local.join("dotnet").is_file().then(|| (local.join("dotnet"), Some(local)))
}

/// Runs a command, forwarding stdout+stderr lines to the sink.
pub fn run_logged(cmd: &mut Command, sink: &Sink) -> Result<()> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    crate::process::hide_window(cmd);
    let mut child = cmd.spawn().with_context(|| format!("running {:?}", cmd.get_program()))?;
    let err = child.stderr.take().unwrap();
    let s2 = sink.clone();
    let t = std::thread::spawn(move || {
        for l in BufReader::new(err).lines().map_while(Result::ok) {
            s2(l);
        }
    });
    for l in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        sink(l);
    }
    let _ = t.join();
    let st = child.wait()?;
    if !st.success() {
        bail!("{:?} failed ({st})", cmd.get_program());
    }
    Ok(())
}

fn sha256_hex(p: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(p)?)))
}

/// Idempotent: does nothing when everything is already in place.
pub fn ensure(dir: &Path, sink: &Sink) -> Result<Prepared> {
    if cfg!(windows) {
        ensure_portable(dir, sink)
    } else {
        ensure_venv(dir, sink)
    }
}

/// Saves a download to `dest` (streamed: the .NET SDK is about 280 MB).
fn download(url: &str, dest: &Path) -> Result<()> {
    let resp = ureq::AgentBuilder::new()
        .user_agent("roe-launcher")
        .build()
        .get(url)
        .call()
        .map_err(|e| anyhow!("downloading {url}: {e}"))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let part = dest.with_extension("part");
    let mut out = fs::File::create(&part)?;
    std::io::copy(&mut resp.into_reader(), &mut out).with_context(|| format!("downloading {url}"))?;
    drop(out);
    fs::rename(&part, dest)?;
    Ok(())
}

/// Unpacks a zip into `dest`. Entries that would land outside `dest` are refused.
pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<usize> {
    let mut archive = zip::ZipArchive::new(fs::File::open(zip_path)?).with_context(|| format!("{} is not a valid zip", zip_path.display()))?;
    fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { bail!("unsafe path in {}: {}", zip_path.display(), entry.name()) };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut fs::File::create(&out)?)?;
        }
    }
    Ok(archive.len())
}

/// Windows: portable Python + pip + deps, and the .NET 8 SDK, all inside `.tools`.
fn ensure_portable(dir: &Path, sink: &Sink) -> Result<Prepared> {
    let tools = dir.join(".tools");
    let downloads = tools.join("downloads");
    let py_home = tools.join("python");
    let python = server_python(dir);
    if !python.is_file() {
        sink("Installing a portable Python 3.12 into .tools (about 11 MB)…".into());
        let zip = downloads.join("python-3.12.10-embed-amd64.zip");
        download(PYTHON_EMBED_URL, &zip)?;
        let got = sha256_hex(&zip)?;
        if got != PYTHON_EMBED_SHA256 {
            let _ = fs::remove_file(&zip);
            bail!("the downloaded Python does not match its checksum (got {got})");
        }
        extract_zip(&zip, &py_home)?;
    }
    fs::write(py_home.join("python312._pth"), PYTHON_PTH)?;

    let req = dir.join("requirements.txt");
    // bootstrap.ps1 stores the hash in upper case
    let want = sha256_hex(&req)?.to_uppercase();
    let marker = py_home.join("roe-dependencies.sha256");
    let have = fs::read_to_string(&marker).ok().map(|s| s.trim().to_uppercase());
    if have.as_deref() != Some(want.as_str()) {
        let has_pip = crate::process::hide_window(Command::new(&python).args(["-B", "-c", "import importlib.util,sys;sys.exit(0 if importlib.util.find_spec('pip') else 1)"]))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !has_pip {
            sink("Setting up pip…".into());
            let get_pip = downloads.join("get-pip.py");
            download(GET_PIP_URL, &get_pip)?;
            run_logged(Command::new(&python).arg(&get_pip).args(["--no-warn-script-location", "--disable-pip-version-check"]), sink).context("setting up pip")?;
        }
        sink("Installing Python dependencies…".into());
        run_logged(
            Command::new(&python).args(["-m", "pip", "install", "--disable-pip-version-check", "--no-warn-script-location", "--only-binary=:all:", "-r"]).arg(&req),
            sink,
        )?;
        fs::write(&marker, format!("{want}\r\n"))?;
    }

    let dotnet_home = tools.join("dotnet");
    let dotnet = dotnet_home.join("dotnet.exe");
    let has_sdk = |d: &Path| out_of(Command::new(d).arg("--list-sdks")).is_some_and(|l| l.lines().any(|l| l.starts_with("8.")));
    if !(dotnet.is_file() && has_sdk(&dotnet)) {
        sink("Installing the .NET 8 SDK into .tools\\dotnet (about 280 MB)…".into());
        let zip = downloads.join("dotnet-sdk-8-win-x64.zip");
        download(DOTNET_SDK_ZIP_URL, &zip)?;
        let n = extract_zip(&zip, &dotnet_home)?;
        sink(format!("Unpacked {n} files."));
        let _ = fs::remove_file(&zip);
        if !has_sdk(&dotnet) {
            bail!("the .NET 8 SDK did not start from {}", dotnet.display());
        }
    }
    Ok(Prepared { python, dotnet, dotnet_root: Some(dotnet_home) })
}

/// Linux: system Python 3.11+ with a venv, and a system or `.tools` .NET 8 SDK.
fn ensure_venv(dir: &Path, sink: &Sink) -> Result<Prepared> {
    let tools = dir.join(".tools");
    fs::create_dir_all(&tools)?;

    let sys_python = find_python().ok_or_else(|| anyhow!("Python 3.11 or newer is required (install python3 and python3-venv)"))?;
    let venv = tools.join("venv");
    let python = server_python(dir);
    if !python.is_file() {
        sink("Creating the Python environment…".into());
        run_logged(Command::new(&sys_python).args(["-m", "venv"]).arg(&venv), sink)
            .context("creating the venv (on Debian/Ubuntu install the python3-venv package)")?;
    }
    let req = dir.join("requirements.txt");
    let want = sha256_hex(&req)?;
    let marker = venv.join("roe-dependencies.sha256");
    if fs::read_to_string(&marker).ok().map(|s| s.trim().to_string()) != Some(want.clone()) {
        sink("Installing Python dependencies…".into());
        run_logged(
            Command::new(&python).args(["-m", "pip", "install", "--disable-pip-version-check", "--only-binary=:all:", "-r"]).arg(&req),
            sink,
        )?;
        fs::write(&marker, format!("{want}\n"))?;
    }

    let (dotnet, dotnet_root) = match find_dotnet(&tools) {
        Some(x) => x,
        None => {
            sink("Installing the .NET 8 SDK into .tools/dotnet (about 200 MB)…".into());
            let script = tools.join("dotnet-install.sh");
            let body = ureq::get("https://dot.net/v1/dotnet-install.sh")
                .call()
                .map_err(|e| anyhow!("downloading dotnet-install.sh: {e}"))?
                .into_string()?;
            fs::write(&script, body)?;
            let target = tools.join("dotnet");
            run_logged(Command::new("bash").arg(&script).args(["--channel", "8.0", "--no-path", "--install-dir"]).arg(&target), sink)?;
            (target.join("dotnet"), Some(target))
        }
    };
    Ok(Prepared { python, dotnet, dotnet_root })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, body) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn unpacks_zips_and_refuses_paths_outside_the_target() {
        let tmp = tempfile::tempdir().unwrap();
        let good = tmp.path().join("good.zip");
        make_zip(&good, &[("python.exe", b"MZ"), ("Lib/site-packages/x.py", b"x = 1")]);
        let dest = tmp.path().join("python");
        assert_eq!(extract_zip(&good, &dest).unwrap(), 2);
        assert_eq!(fs::read(dest.join("python.exe")).unwrap(), b"MZ");
        assert!(dest.join("Lib/site-packages/x.py").is_file());

        let evil = tmp.path().join("evil.zip");
        make_zip(&evil, &[("../escaped.txt", b"no")]);
        assert!(extract_zip(&evil, &tmp.path().join("t")).is_err());
        assert!(!tmp.path().join("escaped.txt").exists());
    }

    #[test]
    fn portable_python_path_file_matches_bootstrap_ps1() {
        let lines: Vec<&str> = PYTHON_PTH.split("\r\n").filter(|l| !l.is_empty()).collect();
        assert_eq!(lines, ["python312.zip", ".", r"Lib\site-packages", r"..\..\tools", r"..\..\server\directory", "import site"]);
    }
}
