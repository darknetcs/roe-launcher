//! The server's match options, read from the installed `windows_launch.py` itself, so the Server Settings
//! page always offers exactly what this upstream version supports (upstream adds options almost daily).
//!
//! A small Python helper replaces `argparse.ArgumentParser.parse_args`, runs the script, prints every
//! option as JSON and exits before anything starts. The result is cached in `.tools/` next to a hash of
//! the script, so it is only read again after an update.
//!
//! Saved choices are a map `flag -> value` ("true" for switches that are on). A flag that is not in the
//! map is not passed, so the server uses its own default.
use crate::Sink;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Run with the server's Python: `python -X utf8 -B -c HELPER <path of windows_launch.py>`.
const HELPER: &str = r#"
import argparse, json, runpy, sys
script = sys.argv[1]
def dump(self, *args, **kwargs):
    out = []
    for a in self._actions:
        if isinstance(a, argparse._HelpAction) or not a.option_strings or a.help == argparse.SUPPRESS:
            continue
        d = a.default
        if d is argparse.SUPPRESS:
            d = None
        elif d is not None and not isinstance(d, (bool, int, float)):
            d = str(d)
        out.append({
            "flag": max(a.option_strings, key=len),
            "switch": isinstance(a, (argparse._StoreTrueAction, argparse._StoreFalseAction)),
            "type": getattr(a.type, "__name__", None) if a.type else None,
            "choices": [str(c) for c in a.choices] if a.choices is not None else None,
            "default": d,
            "help": (a.help or "").replace("%%", "%"),
            "required": bool(a.required),
        })
    sys.stdout.write(json.dumps(out))
    sys.stdout.flush()
    raise SystemExit(0)
argparse.ArgumentParser.parse_args = dump
sys.argv = [script]
runpy.run_path(script, run_name="__main__")
"#;

/// Options the launcher sets itself, or that cannot work with how it starts the server: never shown.
/// `--debug-console` belongs to the game client and windows_launch.py refuses it with `--services-only`.
/// `--map` is RoE-Rivival's map, set by the launcher's map picker (`Source::map_option`).
pub const MANAGED: [&str; 11] = [
    "--client",
    "--dotnet",
    "--key-file",
    "--verify-only",
    "--remote-peer",
    "--lan-tunnel",
    "--services-only",
    "--duration",
    "--debug-console",
    "--profile",
    "--map",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerOption {
    pub flag: String,
    /// On/off (`store_true`).
    pub switch: bool,
    /// Python type name of the value: "int", "Path", "str"... (None = text).
    #[serde(rename = "type")]
    pub ty: Option<String>,
    pub choices: Option<Vec<String>>,
    pub default: Option<serde_json::Value>,
    pub help: String,
    #[serde(default)]
    pub required: bool,
}

impl ServerOption {
    /// The server's default as text ("" when there is none).
    pub fn default_text(&self) -> String {
        match &self.default {
            None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(_)) => String::new(),
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
        }
    }

    /// The help text in English: upstream's own, unless it is missing or still Russian.
    pub fn help_en(&self) -> String {
        let russian = self.help.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
        if !self.help.is_empty() && !russian {
            // the launcher's font has no arrow glyphs
            return self.help.replace('→', "->").replace('←', "<-");
        }
        match self.flag.as_str() {
            "--practice-targets" => "Number of passive targets, 1..10; requires Target".into(),
            "--bots" => "One scripted bot (BOT05), no attack or evacuation; needs Target set to none, so not with targets or practice targets".into(),
            _ => self.help.clone(),
        }
    }

    pub fn is_int(&self) -> bool {
        self.ty.as_deref() == Some("int")
    }
}

/// What the launcher has always started ROE-LIVE with (the same as upstream's play.cmd); see `Source::defaults`.
/// `--loot-catalog` is not here: when unset, the newest bundled catalog is picked automatically.
pub fn launcher_defaults() -> BTreeMap<String, String> {
    crate::update::ROE_LIVE.defaults()
}

/// The JSON printed by the helper, minus what the launcher manages or the user cannot leave out.
pub fn parse_schema(json: &str) -> Result<Vec<ServerOption>> {
    let all: Vec<ServerOption> = serde_json::from_str(json).map_err(|e| anyhow!("unexpected option list from windows_launch.py: {e}"))?;
    Ok(all.into_iter().filter(|o| !o.required && !MANAGED.contains(&o.flag.as_str())).collect())
}

#[derive(Serialize, Deserialize)]
struct Cache {
    script_sha256: String,
    options: Vec<ServerOption>,
}

/// The options of the installed `tools/windows_launch.py`, from the cache when the script is unchanged.
pub fn read_schema(dir: &Path, python: &Path, sink: &Sink) -> Result<Vec<ServerOption>> {
    let script = dir.join("tools/windows_launch.py");
    let sha = format!("{:x}", Sha256::digest(fs::read(&script).with_context(|| format!("reading {}", script.display()))?));
    let cache_path = dir.join(".tools/roe-launcher-options.json");
    if let Ok(c) = fs::read_to_string(&cache_path).map_err(anyhow::Error::from).and_then(|t| Ok(serde_json::from_str::<Cache>(&t)?)) {
        if c.script_sha256 == sha {
            // filter again: a cache written by an older launcher may hold options now managed
            return Ok(c.options.into_iter().filter(|o| !MANAGED.contains(&o.flag.as_str())).collect());
        }
    }
    if !python.is_file() {
        bail!("the server's Python is not installed yet (it is set up on the first start)");
    }
    sink("Reading the server's match options…".into());
    let out = crate::process::hide_window(Command::new(python).args(["-X", "utf8", "-B", "-c", HELPER]).arg(&script).current_dir(dir))
        .output()
        .context("running windows_launch.py to list its options")?;
    if !out.status.success() {
        bail!("windows_launch.py could not list its options: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let options = parse_schema(&String::from_utf8_lossy(&out.stdout))?;
    if let Ok(t) = serde_json::to_string(&Cache { script_sha256: sha, options: options.clone() }) {
        let _ = fs::create_dir_all(dir.join(".tools"));
        let _ = fs::write(&cache_path, t);
    }
    Ok(options)
}

/// Saved choices as command-line arguments. With a schema, choices this server version no longer
/// knows are dropped (an update may remove an option); without one, everything saved is passed.
pub fn to_args(values: &BTreeMap<String, String>, schema: Option<&[ServerOption]>) -> Vec<String> {
    let mut out = Vec::new();
    for (flag, value) in values {
        if MANAGED.contains(&flag.as_str()) {
            continue;
        }
        let known = schema.map(|s| s.iter().find(|o| &o.flag == flag));
        let switch = match known {
            Some(None) => continue,
            Some(Some(o)) => o.switch,
            None => value == "true" || value == "false",
        };
        if switch {
            if value == "true" {
                out.push(flag.clone());
            }
        } else if !value.trim().is_empty() {
            out.push(flag.clone());
            out.push(value.trim().to_string());
        }
    }
    out
}

/// "--practice-targets" -> "Practice targets", with the usual acronyms in capitals.
pub fn label_of(flag: &str) -> String {
    let words: Vec<String> = flag
        .trim_start_matches('-')
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| match w {
            "udp" | "los" | "hp" | "npc" | "ai" | "id" => w.to_uppercase(),
            _ => w.to_string(),
        })
        .collect();
    let mut s = words.join(" ");
    if let Some(first) = s.get(0..1) {
        s.replace_range(0..1, &first.to_uppercase());
    }
    s
}

/// Sections of the Server Settings page, in order. New upstream options land in "Other".
pub const GROUPS: [&str; 6] = ["Match", "Targets & bots", "Map & world", "Vehicles", "Network", "Other"];

pub fn group_of(flag: &str) -> &'static str {
    let f = flag.trim_start_matches('-');
    let has = |words: &[&str]| words.iter().any(|w| f.contains(w));
    if has(&["target", "bot", "live-control", "armor"]) {
        "Targets & bots"
    } else if has(&["vehicle", "jeep", "boat", "snowmotor"]) {
        "Vehicles"
    } else if has(&["udp", "handoff"]) {
        "Network"
    } else if has(&["winter", "loot", "airdrop", "evacuation", "departure", "climb", "start-hour", "temperature"]) {
        "Map & world"
    } else if has(&["kit", "mode", "cycle-scale", "weapon", "profile", "death", "frag", "validate-movement"]) {
        "Match"
    } else {
        "Other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
        {"flag":"--client","switch":false,"type":"Path","choices":null,"default":null,"help":"x","required":true},
        {"flag":"--kit","switch":false,"type":null,"choices":["snowboard","glider","climb"],"default":"snowboard","help":"","required":false},
        {"flag":"--practice-targets","switch":false,"type":"int","choices":["1","2","3"],"default":1,"help":"Targets 1..10","required":false},
        {"flag":"--live-control","switch":true,"type":null,"choices":null,"default":false,"help":"Local live commands","required":false},
        {"flag":"--duration","switch":false,"type":"int","choices":null,"default":3600,"help":"","required":false},
        {"flag":"--debug-console","switch":true,"type":null,"choices":null,"default":false,"help":"client only","required":false}
    ]"#;

    #[test]
    fn reads_the_option_list_and_hides_what_the_launcher_manages() {
        let s = parse_schema(SAMPLE).unwrap();
        assert_eq!(s.iter().map(|o| o.flag.as_str()).collect::<Vec<_>>(), ["--kit", "--practice-targets", "--live-control"]);
        assert_eq!(s[1].default_text(), "1");
        assert!(s[1].is_int() && s[2].switch && s[2].default_text().is_empty());
        let ru = ServerOption { help: "Число пассивных мишеней1..10; требует Target".into(), ..s[1].clone() };
        assert_eq!(ru.help_en(), "Number of passive targets, 1..10; requires Target");
        assert_eq!(s[2].help_en(), "Local live commands");
        let arrow = ServerOption { help: "Single BOT05 Citori/12ga→safe".into(), ..s[2].clone() };
        assert_eq!(arrow.help_en(), "Single BOT05 Citori/12ga->safe");
    }

    #[test]
    fn turns_choices_into_arguments() {
        let schema = parse_schema(SAMPLE).unwrap();
        let mut v = BTreeMap::new();
        v.insert("--practice-targets".to_string(), "10".to_string());
        v.insert("--live-control".to_string(), "true".to_string());
        v.insert("--kit".to_string(), String::new()); // empty = server default
        v.insert("--removed-upstream".to_string(), "true".to_string());
        v.insert("--duration".to_string(), "5".to_string()); // managed by the launcher
        assert_eq!(to_args(&v, Some(&schema)), ["--live-control", "--practice-targets", "10"]);
        // without a schema (Python not set up yet) everything saved is passed
        assert!(to_args(&v, None).contains(&"--removed-upstream".to_string()));
    }

    #[test]
    fn the_defaults_match_what_the_launcher_always_used() {
        let a = to_args(&launcher_defaults(), None).join(" ");
        for part in ["--kit glider", "--mode cycle", "--cycle-scale 3", "--reliable-udp", "--idle-handoff", "--winter-doors", "--winter-windows", "--target armored"] {
            assert!(a.contains(part), "{part} missing from {a}");
        }
    }

    #[test]
    fn labels_and_groups() {
        assert_eq!(label_of("--practice-targets"), "Practice targets");
        assert_eq!(label_of("--reliable-udp"), "Reliable UDP");
        assert_eq!(label_of("--target-los"), "Target LOS");
        assert_eq!(group_of("--practice-targets"), "Targets & bots");
        assert_eq!(group_of("--live-control"), "Targets & bots");
        assert_eq!(group_of("--kit"), "Match");
        assert_eq!(group_of("--winter-doors"), "Map & world");
        assert_eq!(group_of("--vehicle-profile"), "Vehicles");
        assert_eq!(group_of("--idle-handoff"), "Network");
        assert_eq!(group_of("--brand-new-thing"), "Other");
    }

    /// The real helper against a small stand-in for windows_launch.py (needs python3 on PATH).
    #[test]
    fn the_helper_reads_a_real_argparse_script() {
        let Some(py) = crate::runtime::find_python() else { return };
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("tools")).unwrap();
        fs::write(
            tmp.path().join("tools/windows_launch.py"),
            "import argparse, sys\n\
             def main():\n\
             \x20   p = argparse.ArgumentParser()\n\
             \x20   p.add_argument('--client', required=True)\n\
             \x20   p.add_argument('--practice-targets', type=int, choices=range(1, 11), default=1, help='1..10 targets, 100%% fake')\n\
             \x20   p.add_argument('--live-control', action='store_true')\n\
             \x20   p.add_argument('--secret', help=argparse.SUPPRESS)\n\
             \x20   p.parse_args()\n\
             \x20   sys.exit('must not get here')\n\
             if __name__ == '__main__':\n\
             \x20   main()\n",
        )
        .unwrap();
        let sink: Sink = std::sync::Arc::new(|_| {});
        let s = read_schema(tmp.path(), &py, &sink).unwrap();
        assert_eq!(s.iter().map(|o| o.flag.as_str()).collect::<Vec<_>>(), ["--practice-targets", "--live-control"]);
        assert_eq!(s[0].choices.as_ref().unwrap().len(), 10);
        assert_eq!(s[0].help, "1..10 targets, 100% fake");
        // second call comes from the cache
        assert!(tmp.path().join(".tools/roe-launcher-options.json").is_file());
        assert_eq!(read_schema(tmp.path(), Path::new("/nonexistent/python"), &sink).unwrap(), s);
    }
}
