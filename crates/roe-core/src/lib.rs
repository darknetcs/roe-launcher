//! Core logic of the Ring of Elysium local-server launcher.
//! Everything here is GUI-free so it can be unit-tested and reused by a CLI.

pub mod backgrounds;
pub mod client;
pub mod config;
pub mod game;
pub mod lan;
pub mod options;
pub mod process;
pub mod proton;
pub mod runtime;
pub mod server;
pub mod servers;
pub mod steam;
pub mod translate;
pub mod update;
pub mod util;

/// Callback used to stream log lines to whoever is interested (the GUI log view).
pub type Sink = std::sync::Arc<dyn Fn(String) + Send + Sync>;

pub const APP_NAME: &str = "Ring of Elysium Local Launcher";

/// Linux runs the Windows game through Proton; on Windows it starts directly.
pub const NEEDS_PROTON: bool = cfg!(not(windows));

/// True when this Windows build runs under Wine / Proton (Wine's ntdll exports `wine_get_version`).
pub fn under_wine() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        // SAFETY: ntdll is always loaded; both names are NUL-terminated.
        unsafe {
            let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast());
            !ntdll.is_null() && GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_some()
        }
    }
    #[cfg(not(windows))]
    false
}
