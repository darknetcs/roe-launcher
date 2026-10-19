//! The player's in-game name. Upstream every player is called "BRING_BACK_ROE!!" in the lobby and on the map
//! selection, and "LOCAL" in the match (newspaper, stats). The name lives in two recorded replies:
//! - the lobby's first login packet (`LobbyPreData.cs`), AES-encrypted with the session key, which happens to be
//!   the text "BRING_BACK_ROE!!" too: the key and `lobby-ck.bin` are left alone, only the packet changes;
//! - the enter-room reply (`config/wire.json`, picked by `config/room.json`), field `profile.base.m014`.
//!
//! The same enter-room reply also names the map, `profile.base.m644` (16010 = Dione, see `maps`), so the map choice
//! is applied here too.
//!
//! Before every server start the launcher runs `PATCH_PY` with the server's own Python, reusing upstream's
//! decoder and AES code. It keeps the original of each file it edits (`*.roe-original`) and always patches from
//! that copy, so an empty name restores upstream exactly. A server update brings fresh files and the name is
//! simply applied again.
use crate::process::hide_window;
use crate::runtime::Prepared;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// Longest name: the original "BRING_BACK_ROE!!" is 16 characters, the most the game is known to show.
pub const MAX_LEN: usize = 16;
/// Every new player's name: the original server's, as a small Easter egg (it is also the session key's text).
/// With it the lobby needs no rebuild, and the match shows it too instead of "LOCAL".
pub const DEFAULT: &str = "BRING_BACK_ROE!!";

/// The trimmed name, or why it can't be used. Empty means "keep the original names".
/// Only plain ASCII for now: other characters have not been tested in the game.
pub fn validate(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.chars().count() > MAX_LEN {
        return Err(format!("At most {MAX_LEN} characters."));
    }
    if let Some(c) = name.chars().find(|c| !(c.is_ascii_graphic() || *c == ' ')) {
        return Err(format!("\"{c}\" can't be used: only English letters, digits, spaces and symbols for now."));
    }
    if name.contains("  ") {
        return Err("No double spaces.".into());
    }
    Ok(name.to_string())
}

/// What `apply` changed.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Applied {
    /// The lobby packet changed: the lobby is rebuilt on this start (a few seconds).
    pub lobby: bool,
    /// The room reply changed.
    pub room: bool,
}

/// Puts `name` (already validated; empty = originals) and the map ID into the server files in `dir`.
/// `map` = `crate::maps::DIONE` keeps upstream's map.
pub fn apply(p: &Prepared, dir: &Path, name: &str, map: u32) -> Result<Applied> {
    let mut c = Command::new(&p.python);
    c.args(["-I", "-X", "utf8", "-B", "-c", PATCH_PY]).arg(dir).arg(name).arg(map.to_string()).current_dir(dir);
    hide_window(&mut c);
    let out = c.output().context("running the server's Python")?;
    let text = String::from_utf8_lossy(&out.stdout);
    let last = text.lines().last().unwrap_or("").trim();
    if !out.status.success() || !last.starts_with('{') {
        let err = String::from_utf8_lossy(&out.stderr);
        let why = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(last).trim().to_string();
        bail!("could not set the player name and map: {why}");
    }
    serde_json::from_str(last).map_err(|e| anyhow!("could not set the player name and map: {e} ({last})"))
}

/// Run as `python -c PATCH_PY <server dir> <name> <map id>`; prints `{"lobby": bool, "room": bool}` as its last line.
/// Errors (an upstream layout it doesn't recognise) end it with a message and touch nothing more.
pub const PATCH_PY: &str = r#"
import sys, re, json, struct, hashlib, shutil
from pathlib import Path
root, name, map_id = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
sys.path.insert(0, str(root / 'tools'))
from roe_protocol_2022 import encrypt, decrypt
from roe_room_2023 import decode_enter_room_response

def original(rel):
    f = root / rel
    o = f.with_name(f.name + '.roe-original')
    if not o.exists():
        shutil.copy2(f, o)
    return f, o.read_text(encoding='utf-8')

def put(f, text):
    if f.read_text(encoding='utf-8') == text:
        return False
    f.write_text(text, encoding='utf-8')
    return True

def text_field(s):
    s = s.encode('utf-8') + b'\0'
    return struct.pack('>I', len(s)) + s

# lobby: the first recorded login packet carries the name
key = (root / 'private/candidate-001.key').read_bytes()
lobby_f, lobby_src = original('search/Ring-of-Elysium-Lobby-Emulator/LobbyEmulator/Logic/LobbyPreData.cs')
blob = re.search(r'dt\.Add\("([0-9A-F]+)"\)', lobby_src)
if not blob:
    sys.exit('the lobby has no recorded login packet any more (upstream changed it)')
plain = decrypt(bytes.fromhex(blob.group(1)), key)
old = text_field(key.decode('ascii'))
if plain.count(old) != 1:
    sys.exit('the lobby login packet has a new layout (upstream changed it)')
if name:
    want = plain.replace(old, text_field(name))
    now = re.search(r'dt\.Add\("([0-9A-F]+)"\)', lobby_f.read_text(encoding='utf-8'))
    try:
        same = now and decrypt(bytes.fromhex(now.group(1)), key) == want
    except Exception:
        same = False
    # encryption pads with random bytes: only re-encrypt when the content differs
    lobby_text = lobby_f.read_text(encoding='utf-8') if same else lobby_src.replace(blob.group(1), encrypt(want, key).hex().upper(), 1)
else:
    lobby_text = lobby_src
lobby = put(lobby_f, lobby_text)
if lobby:
    # windows_launch.py builds the lobby again when its DLL is missing
    for d in ('bin', 'obj'):
        shutil.rmtree(root / 'server/lobby/LobbyEmulator.Sdk' / d, ignore_errors=True)

# match: the enter-room reply's profile.base.m014 = name ("LOCAL" when empty), profile.base.m644 = map
room_f, room_src = original('config/room.json')
wire_f, wire_src = original('config/wire.json')
wire = json.loads(wire_src)
token = json.loads(room_src)['room_file'].removeprefix('@wire/')
raw = bytes.fromhex(wire[token]['hex'])
def fields(b):
    return {f['name']: f for f in decode_enter_room_response(b)['fields']}
field, map_field = fields(raw).get('profile.base.m014'), fields(raw).get('profile.base.m644')
if not field or struct.unpack_from('>I', raw, field['offset'] - 4)[0] != field['length'] or not map_field:
    sys.exit('the room reply has a new layout (upstream changed it)')
new = raw
if name:
    at = field['offset']
    new = new[:at - 4] + text_field(name) + new[at + field['length']:]
if map_id != struct.unpack_from('>I', raw, map_field['offset'])[0]:
    # the name may have moved it
    at = fields(new)['profile.base.m644']['offset']
    new = new[:at] + struct.pack('>I', map_id) + new[at + 4:]
if new != raw:
    check = fields(new)
    if (name and check['profile.base.m014']['value'] != name.encode('utf-8') + b'\0') or \
            struct.unpack_from('>I', new, check['profile.base.m644']['offset'])[0] != map_id:
        sys.exit('the new room reply did not read back correctly')
    new_token = hashlib.sha256(new).hexdigest()
    wire[new_token] = dict(wire[token], hex=new.hex(), size=len(new), source='roe-launcher player name and map')
    room_text = json.dumps({'room_file': '@wire/' + new_token})
    wire_text = json.dumps(wire, indent=2) + '\n'
else:
    room_text, wire_text = room_src, wire_src
room = put(wire_f, wire_text) | put(room_f, room_text)
print(json.dumps({'lobby': lobby, 'room': room}))
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(validate("  darknetcs "), Ok("darknetcs".into()));
        assert_eq!(validate(""), Ok(String::new()));
        assert_eq!(validate(DEFAULT), Ok(DEFAULT.into()));
        assert_eq!(DEFAULT.len(), MAX_LEN);
        assert_eq!(validate("Big Boss [PL]"), Ok("Big Boss [PL]".into()));
        assert!(validate("seventeen_chars_x").is_err());
        assert!(validate("Łukasz").is_err());
        assert!(validate("tab\there").is_err());
        assert!(validate("two  spaces").is_err());
    }

    /// Runs the patch on a copy of a real server (ROE_SERVER_DIR, default the launcher's own copy):
    /// sets a name, sets it again (nothing changes), changes it, then clears it (files back to upstream).
    #[test]
    #[ignore]
    fn live_patch_round_trip() {
        let src = std::env::var_os("ROE_SERVER_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| crate::util::data_dir().join("server"));
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("server");
        for rel in ["tools", "server/directory", "private", "config", "search", "server/lobby/LobbyEmulator.Sdk"] {
            copy_dir(&src.join(rel), &dir.join(rel));
        }
        let python = if cfg!(windows) { src.join(".tools/python/python.exe") } else { src.join(".tools/venv/bin/python3") };
        let p = Prepared { python, dotnet: "dotnet".into(), dotnet_root: None };
        const D: u32 = crate::maps::DIONE;
        let lobby_cs = dir.join("search/Ring-of-Elysium-Lobby-Emulator/LobbyEmulator/Logic/LobbyPreData.cs");
        // the source server may already carry a name: start from upstream's files
        apply(&p, &dir, "", D).unwrap();
        let upstream = [std::fs::read(&lobby_cs).unwrap(), std::fs::read(dir.join("config/room.json")).unwrap(), std::fs::read(dir.join("config/wire.json")).unwrap()];

        // the default keeps the lobby packet as upstream recorded it
        assert_eq!(apply(&p, &dir, DEFAULT, D).unwrap(), Applied { lobby: false, room: true });
        assert_eq!(apply(&p, &dir, "darknetcs", D).unwrap(), Applied { lobby: true, room: true });
        assert!(!dir.join("server/lobby/LobbyEmulator.Sdk/bin").exists());
        assert_eq!(apply(&p, &dir, "darknetcs", D).unwrap(), Applied { lobby: false, room: false });
        assert_eq!(apply(&p, &dir, "Maciej", D).unwrap(), Applied { lobby: true, room: true });
        // the map changes only the room reply; back to Dione restores it
        let room_before = std::fs::read(dir.join("config/room.json")).unwrap();
        assert_eq!(apply(&p, &dir, "Maciej", 16007).unwrap(), Applied { lobby: false, room: true });
        assert_eq!(apply(&p, &dir, "Maciej", 16007).unwrap(), Applied { lobby: false, room: false });
        assert_eq!(apply(&p, &dir, "Maciej", D).unwrap(), Applied { lobby: false, room: true });
        assert_eq!(std::fs::read(dir.join("config/room.json")).unwrap(), room_before);
        // a map alone, with no name
        assert_eq!(apply(&p, &dir, "", 16007).unwrap(), Applied { lobby: true, room: true });
        assert_eq!(apply(&p, &dir, "", D).unwrap(), Applied { lobby: false, room: true });
        assert_eq!(apply(&p, &dir, "", D).unwrap(), Applied { lobby: false, room: false });
        let now = [std::fs::read(&lobby_cs).unwrap(), std::fs::read(dir.join("config/room.json")).unwrap(), std::fs::read(dir.join("config/wire.json")).unwrap()];
        assert!(now == upstream, "clearing the name restores the upstream files");
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (f, t) = (e.path(), to.join(e.file_name()));
            if f.is_dir() {
                copy_dir(&f, &t);
            } else {
                std::fs::copy(&f, &t).unwrap();
            }
        }
    }
}
