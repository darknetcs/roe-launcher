# Privacy policy

**Short version:** roe-launcher has no accounts, no telemetry, no analytics and no advertising. It never sends
information about you or your computer to the developers or anyone else. It only connects to the services listed
below, and only to download what it needs or when you switch a feature on.

This policy covers the launcher itself (Linux and Windows). It is open source, so everything here can be checked
in the code: <https://github.com/darknetcs/roe-launcher>.

## What the launcher stores, and where

Everything stays on your computer:

| | Linux | Windows |
|---|---|---|
| Settings (`launcher.toml`) | `~/.config/roe-local/` | `%APPDATA%\roe-local\` |
| Server files, Python, .NET, session logs, `launcher.log`, your added backgrounds | `~/.local/share/roe-local/` | `%LOCALAPPDATA%\roe-local\` |

The settings include your game folder, Proton choice, match options, saved servers, theme and your **Player name**.
The session logs (`server/runs/`) record what the local server did during your matches. Nothing of this is uploaded.
To remove all of it, see [Uninstalling](README.md#uninstalling).

The launcher only *reads* the Windows registry (to find Steam) and never changes your system configuration. It
installs nothing outside the folders above.

## Network connections

| When | Connects to | What for | What is sent |
|---|---|---|---|
| At start (can be switched off on the Updates page) and on **Check** | `github.com`, `api.github.com` (repository `darknetcs/roe-launcher`) | Is a newer launcher available? | A normal web request; no personal data |
| **Update now** | `github.com` / `objects.githubusercontent.com` | Download the new launcher and its checksum | as above |
| First start and server updates | `github.com`, `codeload.github.com` (repository `ROE-LIVE/ring-of-elysium-local`) | Check for and download the community server files | as above |
| First start (preparing the server) | `www.python.org` (Windows), `bootstrap.pypa.io`, `pypi.org` / `files.pythonhosted.org` | Python and the server's Python packages | as above |
| First start (preparing the server) | `dot.net` / `dotnet.microsoft.com` / `aka.ms` and Microsoft's download servers | The .NET 8 SDK the server needs | as above |
| Only if you set an online server list URL (Server Browser) | the address you entered | Fetch that list | as above |
| **Discord** / **GitHub** buttons | your web browser opens the page | – | – |

These services see your IP address and a standard request like any website does. Their own privacy policies apply:
[GitHub](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement),
[Python Software Foundation](https://www.python.org/privacy/),
[Microsoft](https://privacy.microsoft.com/privacystatement).

**Only on your computer or your local network, never on the internet:**

* **The local server** runs on your computer and only accepts connections from it (`127.0.0.1`).
* **Steam:** the launcher reads Steam's library files to find the game and can ask the Steam client to install it.
  It never asks for, reads or stores Steam passwords or login data.
* **Discord Rich Presence** (off by default, *Appearance → Discord*): talks only to the Discord app running on your
  computer, which shows your activity to your Discord friends. Discord's
  [privacy policy](https://discord.com/privacy) applies to what Discord does with it.
* **LAN play** (off by default, *Server Browser*):
  * **Search** sends a small query to the computers of your own private network (UDP port 18110).
  * **Being visible** (when you switch it on) answers such queries from private-network addresses only, while
    your server runs. The answer contains the name you chose, the game port, the player count, the launcher
    version and the game mode.
  * **Hosting** (when you switch it on) lets computers from private networks, Hamachi (25.x.x.x) or Radmin VPN
    (26.x.x.x) join your server (TCP ports 28001 and 29001).
  * **Hosts by address** (addresses you add in the Server Browser) are asked directly with the same small query
    on every refresh.

## Children

The launcher collects no personal data from anyone, including children.

## Changes

Changes to this policy are part of the project's history on GitHub and are mentioned in the
[changelog](CHANGELOG.md).

Questions: open an issue at <https://github.com/darknetcs/roe-launcher/issues>.
