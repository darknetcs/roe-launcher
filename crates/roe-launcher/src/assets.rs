//! Artwork embedded in the binary: the logo, the wordmark, the tab headers and the four bundled backgrounds.
//! Everything lives in `crates/roe-launcher/assets/`; delete the files there (and the matching
//! `include_bytes!` lines) to build a launcher without any game artwork.
use eframe::egui;

pub const LOGO: &[u8] = include_bytes!("../assets/logo.png");
pub const NAME: &[u8] = include_bytes!("../assets/name.png");
/// Aspect ratio (height / width) of the wordmark image.
pub const NAME_ASPECT: f32 = 79.0 / 1200.0;
/// Width of the wordmark image: every tab header was made at the same scale (80 px tall).
pub const NAME_WIDTH: f32 = 1200.0;

/// Tab headers in the wordmark's style: (texture name, png bytes).
pub const HEADER_BROWSER: (&str, &[u8]) = ("header-browser", include_bytes!("../assets/headers/browser.png"));
pub const HEADER_APPEARANCE: (&str, &[u8]) = ("header-appearance", include_bytes!("../assets/headers/appearance.png"));
/// "Game & Proton" on Linux; just "Game" on Windows, where there is no Proton.
#[cfg(not(windows))]
pub const HEADER_GAME: (&str, &[u8]) = ("header-game", include_bytes!("../assets/headers/game.png"));
#[cfg(windows)]
pub const HEADER_GAME: (&str, &[u8]) = ("header-game", include_bytes!("../assets/headers/game-windows.png"));
pub const HEADER_LOGS: (&str, &[u8]) = ("header-logs", include_bytes!("../assets/headers/logs.png"));
pub const HEADER_SERVER: (&str, &[u8]) = ("header-server", include_bytes!("../assets/headers/server.png"));
pub const HEADER_UPDATES: (&str, &[u8]) = ("header-updates", include_bytes!("../assets/headers/updates.png"));

/// Gemina 2 by Iconian Fonts (free for non-commercial use, see `assets/fonts/gemina2.txt`),
/// used for the Play button.
pub const PLAY_FONT: &[u8] = include_bytes!("../assets/fonts/gemina2.ttf");
/// The font family the Play button is drawn with.
pub const PLAY_FAMILY: &str = "play";

/// Adds the Play button's font as its own family, falling back to egui's fonts for missing glyphs.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("gemina2".into(), std::sync::Arc::new(egui::FontData::from_static(PLAY_FONT)));
    let mut family = vec!["gemina2".to_string()];
    family.extend(fonts.families[&egui::FontFamily::Proportional].iter().cloned());
    fonts.families.insert(egui::FontFamily::Name(PLAY_FAMILY.into()), family);
    ctx.set_fonts(fonts);
}

/// Aspect ratio (height / width) of the logo image.
pub const LOGO_ASPECT: f32 = 223.0 / 253.0;

/// (display name, jpeg bytes). `background1` is the default.
pub const BUNDLED: [(&str, &[u8]); 4] = [
    ("Lakeside", include_bytes!("../assets/backgrounds/background1.jpg")),
    ("Zipline", include_bytes!("../assets/backgrounds/background2.jpg")),
    ("Operator", include_bytes!("../assets/backgrounds/background3.jpg")),
    ("Squad", include_bytes!("../assets/backgrounds/background4.jpg")),
];

/// Bundled background by its 1-based number.
pub fn bundled(n: u8) -> Option<&'static [u8]> {
    BUNDLED.get((n as usize).checked_sub(1)?).map(|b| b.1)
}

pub fn bundled_name(n: u8) -> &'static str {
    BUNDLED.get((n as usize).wrapping_sub(1)).map(|b| b.0).unwrap_or("Background")
}

/// Decodes an image, shrinking it to at most `max_w` pixels wide (keeps GPU memory sane).
pub fn decode(bytes: &[u8], max_w: u32) -> anyhow::Result<egui::ColorImage> {
    let mut img = image::load_from_memory(bytes)?.to_rgba8();
    if img.width() > max_w {
        let h = (img.height() as f32 * max_w as f32 / img.width() as f32).max(1.0) as u32;
        img = image::imageops::resize(&img, max_w, h, image::imageops::FilterType::Triangle);
    }
    let size = [img.width() as usize, img.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// Small preview for the background picker.
pub fn thumbnail(bytes: &[u8]) -> anyhow::Result<egui::ColorImage> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let t = image::imageops::thumbnail(&img, 352, 198);
    let size = [t.width() as usize, t.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, t.as_raw()))
}

/// Window / taskbar icon: the logo centred on a transparent square.
pub fn icon() -> egui::IconData {
    let logo = image::load_from_memory(LOGO).expect("embedded logo").to_rgba8();
    let side = logo.width().max(logo.height());
    let mut canvas = image::RgbaImage::new(side, side);
    image::imageops::overlay(&mut canvas, &logo, ((side - logo.width()) / 2) as i64, ((side - logo.height()) / 2) as i64);
    egui::IconData { rgba: canvas.into_raw(), width: side, height: side }
}
