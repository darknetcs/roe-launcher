# Ring of Elysium — Local Launcher

<img width="1285" height="752" alt="Screenshot From 2026-10-18 22-54-10" src="https://github.com/user-attachments/assets/83264c2f-54b3-457d-81b8-dd05ab80d7e3" />

A native desktop launcher (Rust + egui) for **Linux and Windows** that starts **Ring of Elysium** together with the
community **local server** ([ROE-LIVE/ring-of-elysium-local](https://github.com/ROE-LIVE/ring-of-elysium-local)),
keeps that server up to date from GitHub, and gets out of your way: one **PLAY** button does the rest.

* **Linux:** runs the game through the Proton of your choice (GE-Proton recommended).
* **Windows:** starts the game directly, no Proton or Wine needed.

Unofficial fan tooling. It contains no game files: you bring your own copy of the game
(the supported **2022 build**). See [Artwork, font and licence](#artwork-font-and-licence) about the bundled images.

## Download

Get the latest release from the **[Releases page](https://github.com/darknetcs/roe-launcher/releases/latest)**:

| System | File |
|---|---|
| Linux x86_64 (any distro with glibc 2.35+: Ubuntu 22.04+, Debian 12, Fedora, Arch, SteamOS …) | `roe-launcher-linux-x86_64.tar.gz` |
| Windows 10 / 11, 64-bit | `roe-launcher-windows-x86_64.zip` |

Each file has a `.sha256` next to it. From v0.1.1 on, the launcher **updates itself** (Updates page), so you only
download it once.

## Quick start

### Linux
```bash
tar xf roe-launcher-linux-x86_64.tar.gz
./roe-launcher/roe-launcher
```
A `roe-launcher.desktop` file is included if you want a menu entry.

### Windows
1. Unzip `roe-launcher-windows-x86_64.zip` anywhere you can write to (e.g. `Documents\roe-launcher`).
2. Run `roe-launcher.exe`. The download isn't code-signed, so Windows SmartScreen may warn:
   choose **More info → Run anyway**.

### Then, on both
Check that **Play Locally** shows your game (it is found through Steam automatically) and press **PLAY**.
No game installed? The button reads **INSTALL vIA StEAm**: it opens Steam's install dialog for Ring of Elysium
(it must be in your Steam library), and turns into **PLAY** once Steam has finished the download.

The first start downloads what the server needs (a few hundred MB), then startup is quick:
* the server files from GitHub;
* **Linux:** a Python environment with the pinned dependencies, and a private .NET 8 SDK if you have none;
* **Windows:** a portable Python 3.12 (checksum-verified) and the .NET 8 SDK, kept inside the server folder.
  Nothing is installed system-wide, and no PowerShell is used.

## What it does

| Area | Details |
|---|---|
| **Play** | One button: prepares the server, starts it, waits until its ports are up, then starts the game. When the game closes, the server it started is stopped again. You can also run the server on its own. |
| **Server Settings** | Every match option the installed server offers, read from the server itself and grouped (Match, Targets & bots, Map & world, Vehicles, Network). New upstream options show up after an update without a launcher release. Includes **practice targets** (up to 10 targets on the map) and live control. Clear error messages when the server refuses a combination. |
| **Server status** | Live indicators for the lobby (TCP 9001), proxy (TCP 8001) and endpoint (UDP 18001), uptime, pid, and the server's own output. A server you started yourself is detected and left alone. |
| **Install via Steam** | When no game is found, the main button asks Steam to install it (Steam is called directly, so it works even without a `steam://` link handler; manual steps and a copyable link if Steam can't be reached). While Steam downloads it shows **INSTALLING…**, and it only uses the game once Steam reports the install complete, also when the download was started in Steam itself. |
| **Game location** | Found automatically in your Steam libraries, including libraries on other drives (Linux: `libraryfolders.vdf` of native or Flatpak Steam; Windows: Steam's registry entry). You can always browse or paste a path. The `Europa_Client.exe` hash is checked against the supported 2022 build. |
| **Proton** *(Linux)* | Reads Steam's compatibility tools: GE-Proton, Proton-CachyOS, Proton-EM, UMU-Proton, Valve's Proton, anything with a `proton` script. Choose a fixed version, *"always the newest GE-Proton / CachyOS / …"*, or any custom folder. |
| **Server updates** | Keeps its own copy of the server files and updates it from GitHub (exact-commit download; keeps your saved profiles, the Python/.NET tools, run logs and reports). Optionally re-applies the bundled **English translation** after every update. |
| **Launcher updates** | Checks this project's releases at start. *Update now* downloads the package for your system, installs it only if it matches the release's SHA-256 file and replaces the launcher in place; *Restart now* opens the new version (not while the game or server runs). |
| **Server Browser** *(preview)* | Your local server, servers you save, launchers found on your network and an optional online list, with search, filters and favourites. Only the local server can be joined for now (see [Known limitations](#known-limitations)). |
| **Discord** | *Discord* and *GitHub* buttons in the sidebar. Optional **Rich Presence** (Appearance → Discord, off by default): friends see "Playing Ring of Elysium" and whether you're in the launcher, hosting a server or in a match, with links to the community and the launcher. Works with the regular, Flatpak and Snap Discord apps, and only while Discord runs on your computer. |
| **Look** | Ring of Elysium styling (logo, tab headers, PLAY button font). 4 colour themes, your own accent and text colours, 4 bundled backgrounds or your own images, darkening, panel opacity, interface size, and falling snow or a sandstorm. |

## Requirements

**Linux**
* A desktop with OpenGL (Mesa or a vendor driver), X11 or Wayland.
* `python3` ≥ 3.11 **with venv** (Debian/Ubuntu: `sudo apt install python3 python3-venv`).
* Steam + a Proton runtime (GE-Proton recommended; `GE-Proton11-7` is the tested one).
* `xdg-desktop-portal` for the file dialogs (present on all mainstream desktops).

**Windows**
* Windows 10 or 11, 64-bit, with an OpenGL-capable graphics driver (any current GPU driver).
* Steam with the game installed. Nothing else: Python and .NET are downloaded by the launcher.

**Both:** internet access on the first start (GitHub, PyPI, python.org on Windows, Microsoft's .NET download).

## Where things live

| What | Linux | Windows |
|---|---|---|
| Settings | `~/.config/roe-local/launcher.toml` | `%APPDATA%\roe-local\launcher.toml` |
| Server files (changeable) | `~/.local/share/roe-local/server` | `%LOCALAPPDATA%\roe-local\server` |
| Python / .NET for the server | `server/.tools/` | `server\.tools\` (same layout as upstream's `play.cmd`, so both can share it) |
| Saved profiles (character, cosmetics) | `server/runtime/local-account/` (named: `local-accounts/NAME/`) | `server\runtime\local-account\` |
| Run logs of every session | `server/runs/` | `server\runs\` |
| Launcher log (and crash reports) | `~/.local/share/roe-local/launcher.log` | `%LOCALAPPDATA%\roe-local\launcher.log` |
| Your backgrounds | `~/.local/share/roe-local/backgrounds` | `%LOCALAPPDATA%\roe-local\backgrounds` |
| Wine prefix | the game's own Steam prefix, otherwise `~/.local/share/roe-local/prefix` | — (not needed) |

Each server start creates a new session folder in `runs/` with that session's settings and logs; that's how the
server works, not a reset of your player. Only the newest 20 are kept (Server Settings > Session > *Keep session
logs*, 0 = keep all); older ones are deleted when the launcher starts. Your **profile** (the character and
cosmetics you choose in the lobby) is the only thing the server saves between sessions, and updates keep it.

Added background images are **copied** into the launcher's folder, so moving or deleting the original changes
nothing. That folder is deliberately outside the server folder, because server updates replace the server folder.
On Linux the launcher also imports the answers saved by the old `play-roe.sh` scripts.

## Updating

**Server files** (Updates page):
1. The latest commit is read from GitHub's public Atom feed, falling back to the REST API and finally to git's
   smart-HTTP refs, so a rate limit never blocks you.
2. The exact commit is downloaded as an archive (no `git` needed) and swapped in. Your data is kept: `runtime/`
   (saved profiles: the character and cosmetics you pick in the lobby), `runs/` (session logs), `reports/` and
   `.tools/` (Python/.NET).
3. If enabled, the built-in translator rewrites Russian comments and messages to English. New upstream text that
   isn't in its dictionary stays Russian and is counted on the Updates page.

**The launcher itself** (Updates page → Launcher): see the table above. It needs a writable launcher folder;
otherwise download the new release by hand. On Windows the replaced file is kept as `roe-launcher.old.exe` until
the next start.

HTTPS uses the **system certificate store**, so corporate proxies and custom CAs work.

## Known limitations

* **Only Training is implemented** by the community server; other game modes don't exist yet.
* **Joining other computers' servers is not possible yet:** the community server accepts players on its own
  computer only. The Server Browser is ready for when that changes.
* **Practice targets vs. Bots:** the server's scripted bot (*Bots*) needs *Target* set to `none`, so it can't be
  combined with practice targets. The launcher shows the server's reason when a combination is refused.
* **Windows:** stopping the server ends it at once, so the run logs miss their final "stopped" lines (gameplay is
  not affected).
* **No controller support in the game client**, so there is no handheld mode for now (the groundwork is kept on
  the `handheld` branch).

## Build from source

```bash
cargo build --release -p roe-launcher     # Linux binary: target/release/roe-launcher
cargo test  -p roe-core                   # 58 unit tests, no GUI needed
```
For maximum portability build on an old glibc (the CI uses Ubuntu 22.04). The GUI libraries
(X11/Wayland/GL/xkbcommon) are loaded at run time, so nothing GUI-specific is needed to *build*.

**Windows** builds are cross-compiled from Linux with MinGW:
```bash
rustup target add x86_64-pc-windows-gnu          # plus the mingw-w64 gcc package of your distro
cargo build --release -p roe-launcher --target x86_64-pc-windows-gnu
```
The release workflow (`.github/workflows/release.yml`) does all of this on every `v*` tag: Linux build and tests
on Ubuntu 22.04, the core tests on a real Windows runner, then the Windows build, and attaches both packages and
their checksums to the release.

## Project layout

* `crates/roe-core` — everything that is not drawing: Steam/Proton detection, config, server and launcher updates,
  translation, Python/.NET preparation, server options, server/game command construction, process supervision
  (process groups on Linux, job objects on Windows), Server Browser model and LAN discovery. Unit-tested.
* `crates/roe-launcher` — the egui application.
* `crates/roe-core/examples/headless.rs` — end-to-end run of the core against live GitHub, no GUI.

Development switches: `ROE_START_PAGE=server|game|updates|appearance|logs|browser`, `ROE_AUTORUN=update|server|play`,
`ROE_SCREENSHOT=<file.png>` (save the window after start-up, then close), `ROE_PRETEND_NO_GAME=1` (act as if the
game is not installed, to try *Install via Steam*; settings are not changed), `ROE_DISCORD_APP_ID=<id>` (Rich
Presence with another Discord application).

**Server Browser internals:** `roe-core/src/servers.rs` holds the entry model, merging, favourites-first
filtering, `host:port` validation and a proposed online-list format `{"servers":[...]}` (no such list exists yet;
set its URL under *Sources*). `roe-core/src/lan.rs` finds launchers on your network with one UDP broadcast to
port 18110. Being findable is off by default; when switched on, the launcher answers only computers on private
networks, and only while its server runs.

## Artwork, font and licence

The code is MIT-licensed (`LICENSE`); contributions are welcome (`CONTRIBUTING.md`).

`crates/roe-launcher/assets/` holds the Ring of Elysium logo, wordmark and four backgrounds (the game publisher's
artwork) and tab headers in the wordmark's style (`assets/headers/`). None of them is covered by this project's
licence; see `NOTICE.md`. The PLAY button uses the *Gemina 2* font by Iconian Fonts, free for non-commercial use
(`assets/fonts/gemina2.txt`, shipped with every release). To build without them, replace those files (keep the
names) or adjust `src/assets.rs`.

## Ideas / roadmap
* **Live control panel**: heal, ammo and target damage during a match, with one click.
* **Profiles**: pick or create named profiles (separate characters and cosmetics).
* **Doctor page**: one-click diagnostics (Python, .NET, GL, prefix, ports, hash) with a copyable report.
* **Proton downloader** (Linux): fetch the newest GE-Proton, verify it, install it into `compatibilitytools.d`.
* Playing across two computers (LAN/SSH remote peer), tray icon, restart-on-crash, AppImage/Flatpak.
