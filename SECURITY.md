# Security policy

## Supported versions

Only the **latest release** gets fixes. The launcher updates itself (*Updates → Launcher → Update now*), so please
update before reporting.

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue:

* GitHub: **Security → Report a vulnerability** on <https://github.com/darknetcs/roe-launcher/security>
  (private vulnerability reporting).

Include what you found, how to reproduce it, and which version and platform you use. You'll get an answer as soon as
possible; this is a volunteer project, so please allow a few days. A confirmed problem is fixed in a new release, and
you're credited in the release notes unless you prefer otherwise.

## Antivirus warnings

Some antivirus engines flag the Windows launcher as suspicious. These are false positives on a new, unsigned
program; see [Windows: antivirus warnings](README.md#windows-antivirus-warnings) for how to check your copy is
genuine. If your copy does **not** match the release's checksum or its build attestation, please report it as above.

## Verifying a download

Every release package is built by GitHub Actions from this repository's public source and comes with:

* a `.sha256` checksum file next to it,
* (from v0.1.11) a VirusTotal scan right after the build, with the results and report links in the release notes,
  and
* a GitHub **build provenance attestation**, which proves the file was built by this repository's release workflow
  from a specific commit. Check it with the [GitHub CLI](https://cli.github.com/):

  ```sh
  gh attestation verify roe-launcher-windows-x86_64.zip -R darknetcs/roe-launcher
  ```

## Scope

In scope: the launcher (`roe-launcher`) and its release packages. The community server (ROE-LIVE) and the game have
their own maintainers; problems there are best reported to them.
