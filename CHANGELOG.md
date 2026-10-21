# Changelog

All notable changes to roe-launcher. Full release notes and downloads:
<https://github.com/darknetcs/roe-launcher/releases>.

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
