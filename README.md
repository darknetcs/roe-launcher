# Ring of Elysium — Local Launcher

A small native Linux GUI (Rust + egui) that starts **Ring of Elysium** together with the
community **local server** ([ROE-LIVE/ring-of-elysium-local](https://github.com/ROE-LIVE/ring-of-elysium-local)),
keeps that server up to date from GitHub, and runs the game through the Proton of your choice.

Unofficial fan tooling. It contains no game files: you bring your own copy of the game
(the supported **2022 build**). See [Artwork](#artwork) about the bundled images.

## What it does

| Area | Details |
|---|---|
| **Play** | One button: prepares the server, starts it, waits until its ports are up, then starts the game. When the game closes, the server it started is stopped again. |
| **Server status** | Live indicators for the lobby (TCP 9001), proxy (TCP 8001) and endpoint (UDP 18001), uptime, pid, and the server's own output. Detects a server you started yourself and leaves it alone. |
| **Game location** | Found automatically through Steam's `libraryfolders.vdf` and the app manifest (native and Flatpak Steam, every library). You can always browse or paste a path, or drop the exe. The `Europa_Client.exe` hash is checked against the supported 2022 build. |
| **Proton** | Reads Steam's compatibility tools: GE-Proton, Proton-CachyOS, Proton-EM, UMU-Proton, Valve's Proton, anything with a `proton` script. Choose a fixed version, *"always the newest GE-Proton / CachyOS / …"*, or any custom folder. |
| **Updates** | Keeps its own copy of the server files and updates it from GitHub (exact-commit download, keeps the Python/.NET environment and run logs). Optionally re-applies the bundled **English translation** after every update. |
| **Look** | Ring of Elysium styling by default (Obsidian theme, orange accent, logo and wordmark). Four bundled backgrounds or your own images, darkening, panel opacity, UI scale, optional weather (falling snow or a sandstorm), more presets. |

## Quick start

```bash
tar xf roe-launcher-linux-x86_64.tar.gz
./roe-launcher/roe-launcher
```

On the first **Play** it downloads the server files, creates a Python environment, installs the pinned
dependencies and, if no .NET 8 SDK is installed, a private copy under `.tools/`. After that, startup is instant.

### Requirements
* A Linux desktop with OpenGL (Mesa or a vendor driver), X11 or Wayland.
* `python3` ≥ 3.11 **with venv** (Debian/Ubuntu: `sudo apt install python3 python3-venv`).
* Steam + a Proton runtime (GE-Proton recommended; `GE-Proton11-7` is the tested one).
* `xdg-desktop-portal` for the file dialogs (present on all mainstream desktop environments).
* Internet access on the first start (GitHub, PyPI, Microsoft's .NET installer).

## Where things live
| What | Where |
|---|---|
| Settings | `~/.config/roe-local/launcher.toml` (also imports the answers saved by the old `play-roe.sh` scripts) |
| Server files | `~/.local/share/roe-local/server` (changeable; run logs are in its `runs/` folder) |
| Wine prefix | the game's own Steam prefix when it exists, otherwise `~/.local/share/roe-local/prefix` |
| Your backgrounds | `~/.local/share/roe-local/backgrounds` — added images are **copied** here, so moving, renaming or deleting the original changes nothing. It is deliberately *outside* the server folder, because updates replace that folder. |

## How updating works
1. The latest commit is read from GitHub's public Atom feed, falling back to the REST API and finally
   to git's smart-HTTP refs, so a rate limit never blocks you.
2. The exact commit is downloaded as a tarball (no `git` needed) and swapped in; `.tools/` and `runs/` survive.
3. If enabled, the translator (embedded in the binary) rewrites Russian comments/messages to English.
   New upstream text that isn't in its dictionary stays Russian and is counted on the Updates page.

HTTPS uses the **system certificate store**, so corporate proxies and custom CAs work.

## Build from source
```bash
cargo build --release -p roe-launcher     # binary: target/release/roe-launcher
cargo test  -p roe-core                   # 33 unit tests, no GUI needed
```
For maximum portability build on an old glibc (the CI workflow uses Ubuntu 22.04). The GUI libraries
(X11/Wayland/GL/xkbcommon) are loaded at run time, so nothing GUI-specific has to be installed to *build*.

## Layout
* `crates/roe-core` — everything that is not drawing: Steam/Proton detection, config, updates, translation,
  runtime preparation, server/client command construction, process supervision. Fully unit-tested.
* `crates/roe-launcher` — the egui application.
* `crates/roe-core/examples/headless.rs` — end-to-end run of the core against live GitHub, no GUI.

Development switches: `ROE_START_PAGE=server|game|updates|appearance|logs|browser`, `ROE_AUTORUN=update|server|play`.

## Server Browser
The tab lists servers from four sources: this computer's local server, servers you add yourself, other launchers
found on your local network, and an optional online list. You can search, filter, star favourites and edit your
saved servers.

**Only the local server can be joined for now.** The community server accepts players on its own computer only,
so other entries are listed but their Join button is disabled until that changes.

* `roe-core/src/servers.rs`: the entry model and its `Source`, a `ServerSource` trait, merging, favourites-first
  filtering/sorting, `host:port` validation, `can_join`, and a parser for a proposed JSON list format
  `{"servers":[...]}` (no such online list exists yet; set its URL under *Sources*).
* `roe-core/src/lan.rs`: finding launchers on the local network. Searching sends one UDP broadcast to port 18110.
  Being findable is off by default; when switched on, the launcher answers on UDP 18110, only to computers on
  private networks and only while its server is running.
* `config::Connection` (`Local` or `Remote("host:port")`) decides what the game connects to; with a remote
  connection the launcher does not start a local server. Saved servers, favourites and the source settings are
  stored in `launcher.toml`.

## Artwork
`crates/roe-launcher/assets/` holds the Ring of Elysium logo, wordmark and four backgrounds (the game
publisher's artwork), plus tab headers in the wordmark's style (`assets/headers/`). None of them is covered by any
licence of this project. To build a launcher without them, replace
those files (keep the names) or adjust `src/assets.rs`.

## Ideas / roadmap
* **Match settings page** generated from `windows_launch.py --help`, so every new server option appears automatically.
* **Proton downloader** (fetch the newest GE-Proton release, verify its checksum, install into `compatibilitytools.d`).
* **Doctor page**: one-click diagnostics (Python/venv, .NET, GL, portal, prefix, ports, hash) with a copyable report.
* Tray icon, restart-on-crash, desktop notifications.
* LAN/SSH remote-peer mode (the server already supports it).
* Launcher self-update, AppImage/Flatpak packaging, Steam Deck friendly layout.
