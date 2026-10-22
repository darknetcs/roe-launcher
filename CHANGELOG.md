# Changelog

All notable changes to roe-launcher. Full release notes and downloads:
<https://github.com/darknetcs/roe-launcher/releases>.

## v0.1.12 — 2026-10-22

### Fixes (reported by players)

* **Game folder** (reported by a player on Windows):
  * A game Steam marks as "update required" or "update paused" no longer leaves the launcher stuck on "Waiting for
    Steam to install Ring of Elysium…". Only Steam actually writing the files counts as installing.
  * A folder you choose yourself (Browse or typed) is remembered as yours: Steam's install state isn't checked for
    it any more, also after a restart, until you press Auto-detect. A folder outside your Steam libraries is never
    held up by Steam either.
  * When a folder isn't accepted, or Auto-detect finds nothing, the reason now shows under the field instead of
    only on the Logs page.
  * Choosing a parent folder or a renamed copy works: the launcher looks up to three folders deeper for
    Europa_Client.exe (never the DX12 copy in XVersion; choosing XVersion picks the game folder above it).
  * "Install with Steam" now sits under "Don't have the game installed?", so it no longer reads as an instruction.
* **Linux: no more "No module named pip".** If the launcher first ran before `python3-venv` was installed (Debian,
  Ubuntu, Zorin, Mint …), it was left with a half-made Python environment and failed on every start, even after
  installing the package. It now removes that environment and makes it again, and if `python3-venv` is missing
  it sets up pip itself with PyPA's get-pip.py, so the setup goes on without it. The README's Linux quick start
  now lists `python3-venv`.
* **"Stop the server when the game closes" works while the launcher is minimised.** The launcher only did its
  checks while its window updated, and a minimised window doesn't (on Wayland at least), so the server kept running
  until you next touched the launcher. A background watcher now notices the end of the game, logs it and stops the
  server at once. Log lines also reach `launcher.log` at once, instead of when the window next updates.
* **Server updates explain themselves:** the log says why the server files are downloaded, and leftover
  `server.old` / `server.new` folders from an interrupted update are cleaned up at the next start.

### New

* **Game files check:** the Game page compares the game folder with the 299 files Steam ships for the supported
  build (names and sizes, in milliseconds): "All game files present", or how many are missing or damaged, with
  the full list in the log and how to repair them in Steam. A folder with Europa_Client.exe but most of the game
  missing (an exe copied on its own) is not accepted as the game folder, and the search looks further.
* **Profiles** (Server Settings → Player): several people on one computer can each keep their own character,
  appearance and player name. Add a profile, pick it, and the server starts with it (the community server's own
  `--profile`). Deleting one also deletes its saved appearance. When hosting for your network, joining players use
  the host's profile automatically (the server requires the same one on both sides).
* **Block Tencent's crash reporter** (Game page, off by default): the game starts TQMCenter, Tencent's crash and
  telemetry collector, which can send crash reports and system details to Tencent. The launcher can now stop it
  from starting, without changing any game file (Windows: a "deny run" permission; Linux: no read permission), and
  blocks it again before each game start if Steam restored it. Idea from MLVP's server project.
* **Report an Issue** button (red, above Discord and GitHub in the sidebar): opens the launcher's GitHub issues.
* **Server Browser:** new **Ping** column (green / yellow / red), and the columns now follow the window width:
  long names and addresses are cut to fit (hover shows them in full), and in a narrow window the filter switches
  move to a second line instead of overlapping the buttons.

### Also

* **Credits** in the README (ROE-LIVE, awpstar / MLVP and their team, the community); NOTICE records MLVP's
  permission to use their server work.
* The sidebar's menu entries shrink a little in short windows, so the buttons at the bottom never cover them.
* Test builds for a single bug are published as pre-releases (tags with a dash, e.g. v0.1.11-test.1): the launcher
  never offers them as updates.

## v0.1.11 — 2026-10-21

* **LAN play over Hamachi and Radmin VPN:** hosts accept players with Hamachi (25.x.x.x) and Radmin VPN (26.x.x.x)
  addresses, and **Server Browser → Hosts by address** lets players add a host's address so it's found without the
  network search. Still Training and one player per host.
* **Updates bring the documents too:** updating the launcher now also replaces the licences, privacy policy,
  notices and README next to it (and the icon and metadata on Linux).
* **VirusTotal:** every release is scanned automatically by VirusTotal; the results and report links are on its
  release page.
* Kill Zone's tooltip explains that you can't pick a landing point and get Dione's starting packs.
* **Tidier pages:**
  * Server Settings now reads server status, Network, Player name, Map, Session, Match options. The match
    options start collapsed.
  * The server status shows the players on the server (e.g. "players 1/1").
  * "Stop the server when the game closes" sits under Start / Stop.
  * The hosting switches moved from the Server Browser to Server Settings → Network.
  * The server folder moved to Game & Proton, next to the game folder.
  * "Start server only" opens Server Settings.
  * The duplicate "Server output" box is gone (it's all on the Logs page).
* **Logs → Export…** saves a copy of the launcher log wherever you like, e.g. for a bug report.

## v0.1.10 — 2026-10-21

* **Windows:** the exe now carries version information (publisher, description, version), an icon and a manifest,
  and is built with Microsoft's toolchain (MSVC) instead of MinGW. Together they make antivirus false positives far
  less likely. No Visual C++ runtime needs to be installed.
* **Third-party licences:** both packages include `THIRD-PARTY-LICENSES.html`, the licence notices of all open-source
  libraries in the launcher.
* **Verifiable builds:** every release package has a GitHub build provenance attestation
  (`gh attestation verify <file> -R darknetcs/roe-launcher`).
* **Documents:** privacy policy (`PRIVACY.md`, shipped in the packages), security policy, this changelog,
  uninstall instructions in the README, issue templates.
* **Linux:** AppStream metadata and an icon for software centres.

## v0.1.9 — 2026-10-20

* **LAN play:** host a Training session on one computer and **Join** it from another on the same network, from the
  Server Browser. A launcher-to-launcher relay carries the traffic; no SSH, accounts or extra software.
* **Network search** also asks every address of the local network directly, so firewalls that drop broadcast
  answers (such as ufw) no longer hide hosts.

## v0.1.8 — 2026-10-19

* **Map selector (experimental)** on Server Settings: Dione (works), Kill Zone (selectable for testing, no
  extraction), Desert and Dione at night (shown, not selectable). A warning next to PLAY while an experimental map
  is selected.

## v0.1.7 — 2026-10-19

* **Player name** on Server Settings: your name in the lobby, on the map selection, in the newspaper and on the
  stats screen. Everyone starts as BRING_BACK_ROE!!, the original server's name.

## v0.1.6 — 2026-10-19

* Tab titles drawn live in the Gemina 2 font instead of images: one look for every tab, smaller launcher.

## v0.1.5 — 2026-10-19

* **Discord** and **GitHub** buttons in the sidebar; optional Discord **Rich Presence** (off by default).
* An unreadable `launcher.toml` is kept as `launcher.toml.broken` instead of being replaced by the defaults.

## v0.1.4 — 2026-10-19

* **Install via Steam:** without the game, PLAY becomes INSTALL VIA STEAM and turns back into PLAY when Steam has
  finished downloading.

## v0.1.3 — 2026-10-19

* Server updates keep your saved profile (`runtime/`: character and cosmetics) instead of deleting it.
* Old session logs are cleaned up automatically (keeps the newest 20 by default).

## v0.1.2 — 2026-10-19

* More compact Recent activity; readable arrows in server help texts; README covers Windows as well as Linux.

## v0.1.1 — 2026-10-19

* **The launcher updates itself** (Updates → Launcher), with SHA-256 checks, on Linux and Windows.
* Server Settings no longer offers the Debug console option the server refuses in this mode.

## v0.1.0 — 2026-10-18

* First release for Linux and Windows: one-button play (server + game), automatic server setup and updates,
  Steam library detection with the supported-build check, Proton selection on Linux, Server Settings with every
  match option, practice targets, themes and backgrounds.
