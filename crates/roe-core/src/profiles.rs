//! Local player profiles: the community server's `--profile <name>` (tools/local_account_2022.py). Each profile has
//! its own identity (actor id, derived from the name) and its own saved appearance in
//! `runtime/local-accounts/<name>/cosmetics.json`; "default" keeps the server's original `runtime/local-account/`.
//! One profile per server run: it's the server's, so LAN players joining it play with the host's choice.
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT: &str = "default";

/// The server's own rules (local_account_2022.py): 1-32 ASCII letters, digits, `_` and `-`, starting with a letter
/// or digit, not a Windows device name; case doesn't matter (stored in lower case). Returns the stored form.
pub fn validate(name: &str) -> Result<String, String> {
    let name = name.trim();
    let ok_chars = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let first_ok = name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric());
    if name.is_empty() || name.len() > 32 || !ok_chars || !first_ok {
        return Err("1 to 32 letters (a-z), digits, _ or -, starting with a letter or digit.".into());
    }
    let canonical = name.to_ascii_lowercase();
    let device = ["con", "prn", "aux", "nul"].contains(&canonical.as_str())
        || (canonical.len() == 4 && (canonical.starts_with("com") || canonical.starts_with("lpt")) && matches!(canonical.as_bytes()[3], b'1'..=b'9'));
    if device {
        return Err("That name is reserved by Windows.".into());
    }
    Ok(canonical)
}

/// Where a profile's appearance is saved on the server.
pub fn folder(server_dir: &Path, name: &str) -> PathBuf {
    if name == DEFAULT {
        server_dir.join("runtime/local-account")
    } else {
        server_dir.join("runtime/local-accounts").join(name)
    }
}

/// "default" first, then every profile the server has saved (folders in runtime/local-accounts), then `extra`
/// (made in the launcher, maybe not used yet), each once, sorted.
pub fn list(server_dir: &Path, extra: &[String]) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(server_dir.join("runtime/local-accounts"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| validate(&e.file_name().to_string_lossy()).ok())
                .collect()
        })
        .unwrap_or_default();
    v.extend(extra.iter().filter_map(|p| validate(p).ok()));
    v.retain(|p| p != DEFAULT);
    v.sort();
    v.dedup();
    v.insert(0, DEFAULT.into());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_servers_rules() {
        assert_eq!(validate(" Wojtek "), Ok("wojtek".into()));
        assert_eq!(validate("brother_2-x"), Ok("brother_2-x".into()));
        for bad in ["", "-x", "_x", "a b", "zażółć", "a/b", &"x".repeat(33)] {
            assert!(validate(bad).is_err(), "{bad}");
        }
        for reserved in ["CON", "nul", "com1", "LPT9"] {
            assert_eq!(validate(reserved), Err("That name is reserved by Windows.".into()));
        }
        assert!(validate("com10").is_ok() && validate("console").is_ok());
    }

    #[test]
    fn lists_saved_and_new_profiles_with_default_first() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(list(tmp.path(), &[]), vec!["default"]);
        fs::create_dir_all(tmp.path().join("runtime/local-accounts/maciej")).unwrap();
        fs::create_dir_all(tmp.path().join("runtime/local-accounts/bad name")).unwrap();
        let extra = vec!["Wojtek".to_string(), "maciej".to_string(), "default".to_string()];
        assert_eq!(list(tmp.path(), &extra), vec!["default", "maciej", "wojtek"]);
        assert_eq!(folder(tmp.path(), "default"), tmp.path().join("runtime/local-account"));
        assert_eq!(folder(tmp.path(), "wojtek"), tmp.path().join("runtime/local-accounts/wojtek"));
    }
}
