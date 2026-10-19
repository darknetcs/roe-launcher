//! Handheld gaming computers (Steam Deck, Linux handhelds, Windows handhelds) and Steam's Game Mode.
//!
//! `detect()` gathers the hints this computer gives (environment, OS name, hardware model) and
//! `classify()` decides from them, so the decision is testable without the hardware.
use serde::{Deserialize, Serialize};

/// The user's choice in Appearance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandheldMode {
    /// On when this looks like a handheld or Steam's Game Mode.
    #[default]
    Auto,
    On,
    Off,
}

/// What `detect()` found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hints {
    /// `SteamDeck=1` (set by Steam on the Deck).
    pub steam_deck_env: bool,
    /// Inside a gamescope session (Steam's Game Mode or a nested gamescope).
    pub gamescope: bool,
    /// `/etc/os-release` `ID`, `ID_LIKE` and `VARIANT_ID`, lower case.
    pub os_ids: Vec<String>,
    /// Hardware product name (DMI on Linux, BIOS on Windows).
    pub product: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    /// Looks like a handheld (or Game Mode).
    pub handheld: bool,
    /// A short name for the Appearance page, e.g. "Steam Deck".
    pub device: Option<String>,
    /// Running in gamescope: full screen is forced and Steam's on-screen keyboard is available.
    pub game_mode: bool,
}

/// Known handheld models by (part of) their product name.
const DEVICES: [(&str, &str); 8] = [
    ("jupiter", "Steam Deck"),
    ("galileo", "Steam Deck OLED"),
    ("rog ally", "ROG Ally"),
    ("rc71l", "ROG Ally"),
    ("rc72l", "ROG Ally X"),
    ("legion go", "Legion Go"),
    ("claw", "MSI Claw"),
    ("onexplayer", "OneXPlayer"),
];

pub fn classify(h: &Hints) -> Detection {
    let product = h.product.to_lowercase();
    let device = DEVICES.iter().find(|(key, _)| product.contains(key)).map(|(_, name)| name.to_string());
    let handheld_os = h.os_ids.iter().any(|id| ["steamos", "chimeraos", "bazzite-deck"].contains(&id.as_str()) || id.ends_with("-deck"));
    let device = device.or_else(|| h.steam_deck_env.then(|| "Steam Deck".to_string()));
    Detection { handheld: device.is_some() || handheld_os || h.gamescope, device, game_mode: h.gamescope }
}

/// Whether handheld mode applies, given the user's choice.
pub fn active(mode: HandheldMode, d: &Detection) -> bool {
    match mode {
        HandheldMode::On => true,
        HandheldMode::Off => false,
        HandheldMode::Auto => d.handheld,
    }
}

pub fn detect() -> Detection {
    classify(&hints())
}

fn env_is(name: &str, value: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v.eq_ignore_ascii_case(value))
}

pub fn hints() -> Hints {
    let gamescope = std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|v| v.to_lowercase().contains("gamescope"))
        || env_is("SteamGamepadUI", "1");
    Hints { steam_deck_env: env_is("SteamDeck", "1"), gamescope, os_ids: os_ids(), product: product_name() }
}

#[cfg(not(windows))]
fn os_ids() -> Vec<String> {
    let text = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    parse_os_release(&text)
}

#[cfg(windows)]
fn os_ids() -> Vec<String> {
    Vec::new()
}

/// `ID`, `ID_LIKE` (space separated) and `VARIANT_ID` values, lower case.
pub fn parse_os_release(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        if matches!(k.trim(), "ID" | "ID_LIKE" | "VARIANT_ID") {
            out.extend(v.trim().trim_matches(['"', '\'']).split_whitespace().map(|s| s.to_lowercase()));
        }
    }
    out
}

#[cfg(not(windows))]
fn product_name() -> String {
    ["product_name", "board_name"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(format!("/sys/devices/virtual/dmi/id/{f}")).ok())
        .map(|s| s.trim().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `HKLM\HARDWARE\DESCRIPTION\System\BIOS\SystemProductName`.
#[cfg(windows)]
fn product_name() -> String {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (key, value) = (wide(r"HARDWARE\DESCRIPTION\System\BIOS"), wide("SystemProductName"));
    let mut buf = vec![0u16; 256];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its size in bytes are passed together; the result is NUL-terminated.
    let rc = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
    if rc != 0 {
        return String::new();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    String::from_utf16_lossy(&buf[..len])
}

/// Opens an on-screen keyboard: Steam's in Game Mode, the touch keyboard on Windows.
/// Returns false when there is none to open.
pub fn open_on_screen_keyboard(game_mode: bool) -> bool {
    if game_mode && cfg!(not(windows)) {
        return std::process::Command::new("xdg-open").arg("steam://open/keyboard").spawn().is_ok();
    }
    #[cfg(windows)]
    {
        let tabtip = std::path::Path::new(r"C:\Program Files\Common Files\microsoft shared\ink\TabTip.exe");
        if tabtip.is_file() {
            return std::process::Command::new(tabtip).spawn().is_ok();
        }
    }
    false
}

/// How to get the launcher into Steam's library on this system.
#[derive(Clone, Debug, PartialEq)]
pub enum AddToSteam {
    /// SteamOS / Bazzite ship `steamos-add-to-steam`, which adds a non-Steam game for us.
    Helper(std::path::PathBuf),
    /// Elsewhere: Steam → Games → "Add a Non-Steam Game to My Library…".
    Manual,
}

pub fn add_to_steam_method() -> AddToSteam {
    for dir in ["/usr/bin", "/usr/local/bin", "/bin"] {
        let p = std::path::Path::new(dir).join("steamos-add-to-steam");
        if p.is_file() {
            return AddToSteam::Helper(p);
        }
    }
    AddToSteam::Manual
}

/// Adds `exe` to Steam with the system helper. Steam asks no questions; the entry appears in the
/// library (in Game Mode after Steam restarts or switches back).
pub fn add_to_steam(exe: &std::path::Path) -> anyhow::Result<()> {
    match add_to_steam_method() {
        AddToSteam::Helper(helper) => {
            let st = std::process::Command::new(helper).arg(exe).status()?;
            if !st.success() {
                anyhow::bail!("steamos-add-to-steam failed ({st})");
            }
            Ok(())
        }
        AddToSteam::Manual => anyhow::bail!("this system has no steamos-add-to-steam; add it in Steam: Games > Add a Non-Steam Game"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hints(product: &str, os: &str) -> Hints {
        Hints { product: product.into(), os_ids: parse_os_release(os), ..Default::default() }
    }

    #[test]
    fn recognises_handhelds_by_model() {
        let d = classify(&hints("Jupiter", ""));
        assert!(d.handheld && !d.game_mode);
        assert_eq!(d.device.as_deref(), Some("Steam Deck"));
        assert_eq!(classify(&hints("Galileo Valve", "")).device.as_deref(), Some("Steam Deck OLED"));
        assert_eq!(classify(&hints("ROG Ally RC71L_RC71L", "")).device.as_deref(), Some("ROG Ally"));
        assert_eq!(classify(&hints("83E1 Legion Go", "")).device.as_deref(), Some("Legion Go"));
        assert!(!classify(&hints("MS-7D91 B650M", "ID=ubuntu\n")).handheld);
    }

    #[test]
    fn recognises_handheld_systems_and_game_mode() {
        assert!(classify(&hints("", "NAME=\"SteamOS\"\nID=steamos\nID_LIKE=arch\n")).handheld);
        assert!(classify(&hints("", "ID=bazzite\nVARIANT_ID=bazzite-deck\n")).handheld);
        assert!(!classify(&hints("", "ID=bazzite\nVARIANT_ID=bazzite-gnome\n")).handheld);
        let gm = classify(&Hints { gamescope: true, ..Default::default() });
        assert!(gm.handheld && gm.game_mode);
        let env = classify(&Hints { steam_deck_env: true, ..Default::default() });
        assert_eq!(env.device.as_deref(), Some("Steam Deck"));
    }

    #[test]
    fn the_users_choice_wins() {
        let desk = classify(&hints("Desktop", "ID=ubuntu"));
        let deck = classify(&hints("Jupiter", ""));
        assert!(!active(HandheldMode::Auto, &desk) && active(HandheldMode::Auto, &deck));
        assert!(active(HandheldMode::On, &desk) && !active(HandheldMode::Off, &deck));
    }
}
