//! Applies the bundled Russian -> English translation to a server checkout.
//! The translator and its dictionaries are embedded in the binary and run with Python
//! (already required by the server). New upstream strings that are not in the dictionary
//! stay Russian and are counted in the report.
use anyhow::{bail, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

const SCRIPT: &str = include_str!("../assets/translate_to_english.py");
const PY_MAP: &[u8] = include_bytes!("../assets/translations.json");
const TEXT_MAP: &[u8] = include_bytes!("../assets/text_translations.json");

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub files_changed: u32,
    pub untranslated: u32,
}

pub fn parse_report(out: &str) -> Report {
    let num = |marker: &str| {
        out.lines()
            .find_map(|l| l.split_once(marker))
            .and_then(|(_, rest)| rest.trim().split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    Report { files_changed: num("files changed:"), untranslated: num("UNTRANSLATED Cyrillic remaining:") }
}

pub fn apply(dir: &Path, python: &Path) -> Result<Report> {
    let tmp = std::env::temp_dir().join(format!("roe-translate-{}", std::process::id()));
    fs::create_dir_all(&tmp)?;
    fs::write(tmp.join("translate_to_english.py"), SCRIPT)?;
    fs::write(tmp.join("translations.json"), PY_MAP)?;
    fs::write(tmp.join("text_translations.json"), TEXT_MAP)?;
    let out = Command::new(python).arg("-X").arg("utf8").arg(tmp.join("translate_to_english.py")).arg(dir).output();
    let _ = fs::remove_dir_all(&tmp);
    let out = out?;
    if !out.status.success() {
        bail!("translation failed: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or(""));
    }
    Ok(parse_report(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_translator_summary() {
        let r = parse_report("files changed: 154\nUNTRANSLATED Cyrillic remaining: 371\n  README.md:5: ...\n");
        assert_eq!(r, Report { files_changed: 154, untranslated: 371 });
        assert_eq!(parse_report("garbage"), Report::default());
    }

    #[test]
    fn dictionaries_are_embedded() {
        assert!(PY_MAP.len() > 100_000 && SCRIPT.contains("UNTRANSLATED"));
    }
}
