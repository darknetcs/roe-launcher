//! Discord Rich Presence over Discord's local IPC (no SDK, standard library only).
//!
//! Discord listens on `discord-ipc-0..9`: a Unix socket on Linux (native, Flatpak, Snap and Vesktop each put it
//! in a different folder, all searched here) or a named pipe on Windows. Every message is a frame:
//! opcode (u32 LE), length (u32 LE), JSON. The client sends a handshake with the application ID, then
//! `SET_ACTIVITY` commands; an activity of `null` clears the presence.
//!
//! [`Presence`] runs that in a background thread: it connects when Discord is running, reconnects quietly when
//! Discord starts later or restarts, and only sends an activity when it changed.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::Duration;

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const RETRY: Duration = Duration::from_secs(15);

/// What friends see under "Playing <application name>".
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Activity {
    /// First line, e.g. "Training match".
    pub details: String,
    /// Second line, e.g. "10 practice targets".
    pub state: String,
    /// Shows "00:12 elapsed" from this Unix time (seconds).
    pub started: Option<u64>,
    /// Asset key uploaded in the Developer Portal (Rich Presence > Art Assets).
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    /// Up to two buttons: (label, URL).
    pub buttons: Vec<(String, String)>,
}

impl Activity {
    pub fn to_json(&self) -> Value {
        let mut a = json!({});
        let non_empty = |s: &str| (!s.is_empty()).then(|| s.chars().take(128).collect::<String>());
        if let Some(d) = non_empty(&self.details) {
            a["details"] = json!(d);
        }
        if let Some(s) = non_empty(&self.state) {
            a["state"] = json!(s);
        }
        if let Some(t) = self.started {
            a["timestamps"] = json!({ "start": t });
        }
        let mut assets = json!({});
        if let Some(i) = &self.large_image {
            assets["large_image"] = json!(i);
        }
        if let Some(t) = &self.large_text {
            assets["large_text"] = json!(t);
        }
        if assets.as_object().is_some_and(|o| !o.is_empty()) {
            a["assets"] = assets;
        }
        if !self.buttons.is_empty() {
            a["buttons"] = Value::Array(self.buttons.iter().take(2).map(|(label, url)| json!({ "label": label, "url": url })).collect());
        }
        a
    }
}

/// One IPC frame.
pub fn encode(op: u32, body: &Value) -> Vec<u8> {
    let payload = body.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

/// Reads one frame (opcode and JSON body).
pub fn read_frame(r: &mut impl Read) -> std::io::Result<(u32, Value)> {
    let mut head = [0u8; 8];
    r.read_exact(&mut head)?;
    let op = u32::from_le_bytes(head[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
    if len > 1 << 20 {
        return Err(std::io::Error::other("discord frame too large"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    let v = serde_json::from_slice(&body).unwrap_or(Value::Null);
    Ok((op, v))
}

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

/// Where Discord may listen, in the order to try.
pub fn ipc_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        for i in 0..10 {
            out.push(PathBuf::from(format!(r"\\.\pipe\discord-ipc-{i}")));
        }
        return out;
    }
    let mut bases: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    bases.push(PathBuf::from("/tmp"));
    // native, Flatpak, Snap and Vesktop Discord each use their own folder
    let subs = ["", "app/com.discordapp.Discord", "app/com.discordapp.DiscordCanary", "snap.discord", "snap.discord-canary", ".flatpak/dev.vencord.Vesktop/xdg-run", "app/dev.vencord.Vesktop"];
    for i in 0..10 {
        for base in &bases {
            for sub in subs {
                let p = base.join(sub).join(format!("discord-ipc-{i}"));
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn connect_any() -> Option<Box<dyn Stream>> {
    for p in ipc_paths() {
        #[cfg(unix)]
        if let Ok(s) = std::os::unix::net::UnixStream::connect(&p) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            return Some(Box::new(s));
        }
        #[cfg(windows)]
        if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(&p) {
            return Some(Box::new(f));
        }
    }
    None
}

/// Handshake; Ok when Discord answered READY, Err with Discord's reason otherwise (e.g. an unknown app ID).
fn handshake(s: &mut dyn Stream, client_id: &str) -> Result<(), String> {
    s.write_all(&encode(OP_HANDSHAKE, &json!({ "v": 1, "client_id": client_id }))).map_err(|e| e.to_string())?;
    let (op, v) = read_frame(&mut ReadAdapter(s)).map_err(|e| e.to_string())?;
    match (op, v["evt"].as_str()) {
        (OP_FRAME, Some("READY")) => Ok(()),
        (OP_CLOSE, _) => Err(format!("Discord refused: {}", v["message"].as_str().unwrap_or("closed"))),
        _ => Err(format!("unexpected answer from Discord: {v}")),
    }
}

fn set_activity(s: &mut dyn Stream, activity: Option<&Activity>, nonce: u64) -> std::io::Result<()> {
    let body = json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": std::process::id(), "activity": activity.map(Activity::to_json) },
        "nonce": nonce.to_string(),
    });
    s.write_all(&encode(OP_FRAME, &body))?;
    // Discord answers every command; read it so the socket never fills up
    read_frame(&mut ReadAdapter(s)).map(|_| ())
}

/// `read_frame` wants `impl Read`; this lends it a `&mut dyn Stream`.
struct ReadAdapter<'a>(&'a mut dyn Stream);
impl Read for ReadAdapter<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

/// The presence worker: send `Some(activity)` to show it, `None` to clear it. Dropping it clears the presence.
pub struct Presence {
    tx: Sender<Option<Activity>>,
}

impl Presence {
    /// `status` gets one line whenever the connection changes ("connected", "Discord is not running", errors).
    pub fn start(client_id: String, status: crate::Sink) -> Presence {
        let (tx, rx) = channel::<Option<Activity>>();
        std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || {
                let mut conn: Option<Box<dyn Stream>> = None;
                let mut wanted: Option<Activity> = None;
                let mut sent: Option<Option<Activity>> = None;
                let mut nonce = 0u64;
                let mut last_status = String::new();
                let mut say = |s: String| {
                    if s != last_status {
                        status(s.clone());
                        last_status = s;
                    }
                };
                loop {
                    match rx.recv_timeout(RETRY) {
                        Ok(a) => wanted = a,
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            if let Some(c) = conn.as_mut() {
                                let _ = set_activity(c.as_mut(), None, nonce + 1);
                            }
                            return;
                        }
                    }
                    if conn.is_none() {
                        match connect_any() {
                            None => {
                                say("Discord is not running".into());
                                continue;
                            }
                            Some(mut c) => match handshake(c.as_mut(), &client_id) {
                                Ok(()) => {
                                    say("connected to Discord".into());
                                    conn = Some(c);
                                    sent = None;
                                }
                                Err(e) => {
                                    say(e);
                                    continue;
                                }
                            },
                        }
                    }
                    if sent.as_ref() != Some(&wanted) {
                        nonce += 1;
                        let c = conn.as_mut().unwrap();
                        match set_activity(c.as_mut(), wanted.as_ref(), nonce) {
                            Ok(()) => sent = Some(wanted.clone()),
                            Err(_) => {
                                say("Discord closed the connection; retrying".into());
                                conn = None;
                            }
                        }
                    }
                }
            })
            .expect("spawn discord thread");
        Presence { tx }
    }

    pub fn set(&self, activity: Option<Activity>) {
        let _ = self.tx.send(activity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let f = encode(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY" }));
        assert_eq!(&f[0..4], &1u32.to_le_bytes());
        assert_eq!(u32::from_le_bytes(f[4..8].try_into().unwrap()) as usize, f.len() - 8);
        let (op, v) = read_frame(&mut &f[..]).unwrap();
        assert_eq!((op, v["cmd"].as_str()), (1, Some("SET_ACTIVITY")));
    }

    #[test]
    fn activity_json_leaves_out_what_is_empty() {
        let a = Activity {
            details: "Training match".into(),
            state: String::new(),
            started: Some(1_792_400_000),
            large_image: Some("logo".into()),
            large_text: None,
            buttons: vec![("A".into(), "https://a".into()), ("B".into(), "https://b".into()), ("C".into(), "https://c".into())],
        };
        let j = a.to_json();
        assert_eq!(j["details"], "Training match");
        assert!(j.get("state").is_none());
        assert_eq!(j["timestamps"]["start"], 1_792_400_000u64);
        assert_eq!(j["assets"]["large_image"], "logo");
        assert_eq!(j["buttons"].as_array().unwrap().len(), 2); // Discord allows two
    }

    #[test]
    fn looks_where_snap_and_flatpak_discord_listen() {
        let p: Vec<String> = ipc_paths().iter().map(|p| p.display().to_string()).collect();
        if cfg!(windows) {
            assert!(p[0].ends_with(r"\pipe\discord-ipc-0"));
        } else {
            assert!(p.iter().any(|x| x.ends_with("snap.discord/discord-ipc-0")));
            assert!(p.iter().any(|x| x.ends_with("app/com.discordapp.Discord/discord-ipc-0")));
            assert!(p.iter().any(|x| x == "/tmp/discord-ipc-0"));
        }
    }

    /// A fake Discord on a Unix socket: handshake, then one SET_ACTIVITY.
    #[cfg(unix)]
    #[test]
    fn talks_to_a_fake_discord() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let (op, hello) = read_frame(&mut s).unwrap();
            assert_eq!((op, hello["client_id"].as_str()), (OP_HANDSHAKE, Some("123")));
            s.write_all(&encode(OP_FRAME, &json!({ "evt": "READY", "cmd": "DISPATCH" }))).unwrap();
            let (_, cmd) = read_frame(&mut s).unwrap();
            s.write_all(&encode(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY", "nonce": cmd["nonce"] }))).unwrap();
            cmd
        });
        let mut c: Box<dyn Stream> = Box::new(std::os::unix::net::UnixStream::connect(&path).unwrap());
        handshake(c.as_mut(), "123").unwrap();
        let a = Activity { details: "In the launcher".into(), ..Default::default() };
        set_activity(c.as_mut(), Some(&a), 7).unwrap();
        let cmd = server.join().unwrap();
        assert_eq!(cmd["args"]["activity"]["details"], "In the launcher");
        assert_eq!(cmd["nonce"], "7");
        assert_eq!(cmd["args"]["pid"], std::process::id());
    }

    /// Against the Discord app running on this computer: `cargo test -p roe-core -- --ignored live_discord`.
    /// A made-up ID must reach Discord and be refused by it (proving the socket was found).
    #[test]
    #[ignore]
    fn live_discord_answers() {
        let mut c = connect_any().expect("no Discord IPC socket found");
        let r = handshake(c.as_mut(), "1");
        eprintln!("Discord said: {r:?}");
        assert!(r.is_err());
    }

    /// Sends a launcher-like activity with the app ID in ROE_DISCORD_APP_ID and prints Discord's reply.
    #[test]
    #[ignore]
    fn live_discord_set_activity() {
        let id = std::env::var("ROE_DISCORD_APP_ID").expect("set ROE_DISCORD_APP_ID");
        let mut c = connect_any().expect("no Discord IPC socket found");
        handshake(c.as_mut(), &id).expect("handshake");
        let a = Activity {
            details: "In the launcher".into(),
            state: "Ready to play".into(),
            large_image: Some("logo".into()),
            large_text: Some("Ring of Elysium".into()),
            buttons: vec![("Join the Discord".into(), "https://discord.gg/p9vHf4T2D".into()), ("Get the launcher".into(), "https://github.com/darknetcs/roe-launcher".into())],
            ..Default::default()
        };
        let body = json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": a.to_json() }, "nonce": "1" });
        c.write_all(&encode(OP_FRAME, &body)).unwrap();
        let (op, reply) = read_frame(&mut ReadAdapter(c.as_mut())).unwrap();
        eprintln!("REPLY op={op}: {reply}");
        std::thread::sleep(Duration::from_secs(20));
    }
}
