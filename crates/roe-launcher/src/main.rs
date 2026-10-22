// a release build on Windows opens no console window next to the launcher
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod assets;
mod pages;
mod theme;

use eframe::egui;

/// A crash is written to the end of `launcher.log`: on Windows there is no console to show it.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let path = roe_core::util::data_dir().join("launcher.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "[crash] {info}\n{}", std::backtrace::Backtrace::force_capture());
        }
        default(info);
    }));
}

fn main() -> eframe::Result {
    log_panics();
    // `ROE_WINDOW_SIZE=1600x900` (development): start at another size, e.g. to check layouts with ROE_SCREENSHOT
    let size = std::env::var("ROE_WINDOW_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some([w.trim().parse().ok()?, h.trim().parse().ok()?])))
        .unwrap_or([1280.0, 720.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size) // 16:9 by default, like the bundled backgrounds, so they are not cropped
            .with_min_inner_size([880.0, 580.0])
            .with_title("Ring of Elysium — Local Launcher")
            .with_app_id("roe-launcher")
            .with_icon(std::sync::Arc::new(assets::icon())),
        ..Default::default()
    };
    eframe::run_native("roe-launcher", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
