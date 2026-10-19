# Third-party material

The MIT licence in `LICENSE` covers the source code of this project only.

## Gemina 2 font
`crates/roe-launcher/assets/fonts/gemina2.ttf` (used for the Play button) is *Gemina 2* by Daniel Zadorozny,
Iconian Fonts (http://www.iconian.com/). It is free for non-commercial use and may be freely distributed; see
`gemina2.txt` next to it. It is not covered by this project's licence. Commercial use needs a licence from
Iconian Fonts.

## Ring of Elysium artwork
`crates/roe-launcher/assets/` contains the Ring of Elysium logo and four promotional
backgrounds. They are original artwork by the game's developers/publisher, used here as a fan tribute
to a game whose official servers shut down at the end of 2023. They are **not** covered by this project's
licence and remain the property of their owners. If you are a rights holder and want them removed,
please open an issue and they will be taken out.

To build without them, replace the files in that folder (keep the file names) or edit `src/assets.rs`.

## Translation dictionary
`crates/roe-core/assets/translations.json` and `text_translations.json` contain English translations of
comments and messages from https://github.com/ROE-LIVE/ring-of-elysium-local, keyed by the original text.
That project has not chosen a licence yet; check its status before redistributing these files separately.

## Server
The server itself is not included: the launcher downloads it from ROE-LIVE/ring-of-elysium-local at run time.
