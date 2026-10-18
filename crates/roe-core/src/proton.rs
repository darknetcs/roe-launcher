//! Detects installed Proton runtimes (GE-Proton, CachyOS, Proton-EM, UMU, Valve's Proton ...).
use crate::util::version_key;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Kind {
    GeProton,
    CachyOs,
    ProtonEm,
    UmuProton,
    Valve,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::GeProton, Kind::CachyOs, Kind::ProtonEm, Kind::UmuProton, Kind::Valve, Kind::Other];
    pub fn label(self) -> &'static str {
        match self {
            Kind::GeProton => "GE-Proton",
            Kind::CachyOs => "Proton-CachyOS",
            Kind::ProtonEm => "Proton-EM",
            Kind::UmuProton => "UMU-Proton",
            Kind::Valve => "Valve Proton",
            Kind::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Runtime {
    /// Folder name, e.g. "GE-Proton11-7" (the stable identifier).
    pub name: String,
    /// Human name from compatibilitytool.vdf when present.
    pub display: String,
    pub dir: PathBuf,
    /// The executable `proton` script.
    pub script: PathBuf,
    pub kind: Kind,
    /// Auto-updating aliases such as "Proton-GE Latest".
    pub alias: bool,
    pub key: Vec<u64>,
}

fn classify(name: &str, in_common: bool) -> Kind {
    let l = name.to_lowercase();
    if l.contains("cachyos") {
        Kind::CachyOs
    } else if l.starts_with("ge-proton") || l.starts_with("proton-ge") {
        Kind::GeProton
    } else if l.contains("proton-em") {
        Kind::ProtonEm
    } else if l.starts_with("umu-proton") {
        Kind::UmuProton
    } else if in_common && l.starts_with("proton") {
        Kind::Valve
    } else {
        Kind::Other
    }
}

#[cfg(not(unix))]
fn is_executable_file(p: &Path) -> bool {
    p.is_file()
}

#[cfg(unix)]
fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Builds a Runtime from a folder that contains a `proton` script.
pub fn runtime_from_dir(dir: &Path, in_common: bool) -> Option<Runtime> {
    let script = dir.join("proton");
    if !is_executable_file(&script) {
        return None;
    }
    let name = dir.file_name()?.to_string_lossy().to_string();
    let display = fs::read_to_string(dir.join("compatibilitytool.vdf"))
        .ok()
        .and_then(|t| crate::steam::vdf_values(&t, "display_name").into_iter().next())
        .unwrap_or_else(|| name.clone());
    let alias = name.to_lowercase().ends_with("latest");
    Some(Runtime {
        kind: classify(&name, in_common),
        key: version_key(&name),
        alias,
        name,
        display,
        dir: dir.to_path_buf(),
        script,
    })
}

fn scan(dir: &Path, in_common: bool, only_proton_prefix: bool, out: &mut Vec<Runtime>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        if only_proton_prefix && !e.file_name().to_string_lossy().starts_with("Proton") {
            continue;
        }
        if let Some(rt) = runtime_from_dir(&p, in_common) {
            if !out.iter().any(|r| r.dir == rt.dir) {
                out.push(rt);
            }
        }
    }
}

/// All installed runtimes, sorted: by kind, then non-aliases first, then newest first.
pub fn detect(roots: &[PathBuf], libraries: &[PathBuf]) -> Vec<Runtime> {
    let mut out = Vec::new();
    if !crate::NEEDS_PROTON {
        return out; // Windows runs the game directly
    }
    for r in roots {
        scan(&r.join("compatibilitytools.d"), false, false, &mut out);
    }
    for extra in ["/usr/share/steam/compatibilitytools.d", "/usr/local/share/steam/compatibilitytools.d"] {
        scan(Path::new(extra), false, false, &mut out);
    }
    for l in libraries {
        scan(&l.join("steamapps/common"), true, true, &mut out);
    }
    out.sort_by(|a, b| {
        let ka = Kind::ALL.iter().position(|k| *k == a.kind);
        let kb = Kind::ALL.iter().position(|k| *k == b.kind);
        ka.cmp(&kb)
            .then(a.alias.cmp(&b.alias))
            .then(b.key.cmp(&a.key))
            .then(a.name.cmp(&b.name))
    });
    out
}

/// Newest runtime of a kind (a real, versioned install is preferred over an alias).
pub fn latest_of(rts: &[Runtime], kind: Kind) -> Option<&Runtime> {
    rts.iter()
        .filter(|r| r.kind == kind && !r.alias)
        .max_by(|a, b| a.key.cmp(&b.key))
        .or_else(|| rts.iter().find(|r| r.kind == kind))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn make(root: &Path, name: &str, with_proton: bool) {
        let d = root.join(name);
        fs::create_dir_all(&d).unwrap();
        if with_proton {
            let s = d.join("proton");
            fs::write(&s, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&s, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Mirrors a real compatibilitytools.d listing, including odd names.
    #[test]
    fn detects_and_sorts_a_realistic_listing() {
        let tmp = tempfile::tempdir().unwrap();
        let ct = tmp.path().join("compatibilitytools.d");
        for n in [
            "GE-Proton10-34", "GE-Proton11-3", "GE-Proton11-5", "GE-Proton11-7", "GE-Proton8-32",
            "cachyos-11.0-20260702-slr", "cachyos-11.0-20261005-slr", "proton-cachyos-11.0-20260506-slr-x86_64_v3",
            "Proton-EM Latest", "Proton-GE Latest", "Proton-CachyOS Latest", "UMU-Proton-9.0-4e", "Roberta v0.1.0",
        ] {
            make(&ct, n, true);
        }
        make(&ct, "LegacyRuntime", false); // not a Proton: no `proton` script
        let rts = detect(&[tmp.path().to_path_buf()], &[]);

        assert!(rts.iter().all(|r| r.name != "LegacyRuntime"));
        let ge = latest_of(&rts, Kind::GeProton).unwrap();
        assert_eq!(ge.name, "GE-Proton11-7"); // not the "Latest" alias, not 10-34
        let cachy = latest_of(&rts, Kind::CachyOs).unwrap();
        assert_eq!(cachy.name, "cachyos-11.0-20261005-slr");
        assert!(rts.iter().find(|r| r.name == "Proton-GE Latest").unwrap().alias);
        assert_eq!(rts.iter().find(|r| r.name == "Roberta v0.1.0").unwrap().kind, Kind::Other);
        assert_eq!(rts.iter().find(|r| r.name == "UMU-Proton-9.0-4e").unwrap().kind, Kind::UmuProton);
        // GE-Proton group is sorted newest first, aliases last
        let ge_names: Vec<_> = rts.iter().filter(|r| r.kind == Kind::GeProton).map(|r| r.name.as_str()).collect();
        assert_eq!(ge_names, ["GE-Proton11-7", "GE-Proton11-5", "GE-Proton11-3", "GE-Proton10-34", "GE-Proton8-32", "Proton-GE Latest"]);
    }

    #[test]
    fn finds_valve_proton_in_steam_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join("steamapps/common");
        make(&common, "Proton 9.0 (Beta)", true);
        make(&common, "Ring of Elysium", true); // not a Proton name -> ignored
        let rts = detect(&[], &[tmp.path().to_path_buf()]);
        assert_eq!(rts.len(), 1);
        assert_eq!(rts[0].kind, Kind::Valve);
    }
}
