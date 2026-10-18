//! Visual styling: palettes, egui visuals, gradient background and the weather animations.
use eframe::egui::{self, vec2, Color32, CornerRadius, Pos2, Rect, Stroke};

pub const PRESETS: [&str; 4] = ["Obsidian", "Elysium Frost", "Aurora", "Ember"];

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg_top: Color32,
    pub bg_bottom: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
}

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn preset_accent(preset: &str) -> [u8; 3] {
    match preset {
        "Aurora" => [94, 234, 212],
        "Ember" => [255, 150, 90],
        "Obsidian" => [255, 158, 0],
        _ => [111, 211, 255],
    }
}

/// The default text colour (used when the user has not picked one).
pub const DEFAULT_TEXT: [u8; 3] = [232, 241, 255];

/// `a` moved towards `b` by `t` (0 = a, 1 = b).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

pub fn palette(preset: &str, accent: [u8; 3], text_color: Option<[u8; 3]>) -> Palette {
    let (bg_top, bg_bottom, panel, card) = match preset {
        "Aurora" => (c(5, 16, 22), c(12, 50, 52), c(9, 24, 30), c(15, 38, 44)),
        "Ember" => (c(22, 11, 10), c(60, 27, 20), c(30, 15, 13), c(46, 24, 19)),
        "Obsidian" => (c(10, 10, 12), c(26, 26, 31), c(16, 16, 19), c(27, 27, 32)),
        _ => (c(9, 19, 35), c(27, 53, 84), c(13, 24, 43), c(21, 36, 62)),
    };
    let [r, g, b] = text_color.unwrap_or(DEFAULT_TEXT);
    let text = c(r, g, b);
    // secondary text: the default keeps its tuned grey-blue; a custom colour is faded towards the panels
    let muted = if text_color.is_none() { c(146, 166, 194) } else { mix(text, card, 0.4) };
    Palette {
        bg_top,
        bg_bottom,
        panel,
        card,
        text,
        muted,
        accent: Color32::from_rgb(accent[0], accent[1], accent[2]),
        good: c(96, 214, 140),
        warn: c(255, 196, 84),
        bad: c(255, 108, 108),
    }
}

pub fn apply(ctx: &egui::Context, p: &Palette, scale: f32) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.panel.gamma_multiply(0.8);
    v.faint_bg_color = p.card;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.window_stroke = Stroke::new(1.0_f32, p.accent.gamma_multiply(0.35));
    let r = CornerRadius::same(9);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.accent.gamma_multiply(0.15));
    w.inactive.weak_bg_fill = p.card.gamma_multiply(1.5);
    w.inactive.bg_fill = p.card.gamma_multiply(1.5);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.hovered.weak_bg_fill = p.accent.gamma_multiply(0.30);
    w.hovered.bg_fill = p.accent.gamma_multiply(0.30);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.text);
    w.active.weak_bg_fill = p.accent.gamma_multiply(0.55);
    w.active.bg_fill = p.accent.gamma_multiply(0.55);
    w.active.fg_stroke = Stroke::new(1.5_f32, p.text);
    for x in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        x.corner_radius = r;
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(10.0, 9.0);
        s.spacing.button_padding = vec2(14.0, 7.0);
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
    });
    ctx.set_zoom_factor(scale);
}

/// Vertical two-colour gradient filling `rect`.
pub fn paint_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), top);
    m.colored_vertex(rect.right_top(), top);
    m.colored_vertex(rect.right_bottom(), bottom);
    m.colored_vertex(rect.left_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(m));
}

/// A user-supplied image scaled to cover the window (centre-cropped), darkened by `dim`.
pub fn paint_cover(painter: &egui::Painter, rect: Rect, tex: &egui::TextureHandle, dim: f32) {
    let ts = tex.size_vec2();
    let (rw, rh) = (rect.width() / ts.x, rect.height() / ts.y);
    let s = rw.max(rh);
    let (uw, uh) = (rect.width() / (ts.x * s), rect.height() / (ts.y * s));
    let uv = Rect::from_min_max(Pos2::new((1.0 - uw) / 2.0, (1.0 - uh) / 2.0), Pos2::new((1.0 + uw) / 2.0, (1.0 + uh) / 2.0));
    painter.image(tex.id(), rect, uv, Color32::WHITE);
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha((dim.clamp(0.0, 1.0) * 255.0) as u8));
}

/// Stable pseudo-random 0..1 per particle and property: no state, no allocation.
fn hash(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B1).wrapping_add(salt.wrapping_mul(0x85EB_CA6B));
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// Cheap deterministic snowfall.
pub fn paint_snow(painter: &egui::Painter, rect: Rect, t: f64) {
    for i in 0..110u32 {
        let size = 0.8 + hash(i, 1) * 2.2;
        let speed = 14.0 + size * 16.0;
        let sway = (t as f32 * (0.4 + hash(i, 2)) + hash(i, 3) * 6.28).sin() * 18.0;
        let x = rect.left() + (hash(i, 4) * rect.width() + sway).rem_euclid(rect.width());
        let y = rect.top() + (hash(i, 5) * rect.height() + t as f32 * speed).rem_euclid(rect.height());
        let a = (70.0 + hash(i, 6) * 150.0) as u8;
        painter.circle_filled(Pos2::new(x, y), size, Color32::from_white_alpha(a));
    }
}

/// Cheap deterministic sandstorm: drifting dust clouds, fast streaks and fine grains blown left to right,
/// with gusts that come and go. About 200 shapes a frame, the same budget as the snow.
pub fn paint_sandstorm(painter: &egui::Painter, rect: Rect, t: f64) {
    // wind strength 0.5..1.5; `travel` is its integral, so particles speed up and slow down without jumping
    let t = (t % 3600.0) as f32;
    let (w, k) = (0.23_f32, 0.5_f32);
    let gust = 1.0 + k * (w * t).sin();
    let travel = t - (k / w) * (w * t).cos();
    let sand = |a: f32| Color32::from_rgba_unmultiplied(214, 178, 120, (a.clamp(0.0, 1.0) * 255.0) as u8);
    let (width, height) = (rect.width(), rect.height());

    // faint overall haze, thicker during gusts
    painter.rect_filled(rect, 0.0, sand(0.03 + 0.04 * gust));

    // dust clouds: soft horizontal bands whose thickness varies along x and rolls with the wind
    const COLS: usize = 12;
    for b in 0..5u32 {
        let band_h = height * (0.12 + hash(b, 10) * 0.18);
        let cy = rect.top() + (hash(b, 11) * height + t * (3.0 + hash(b, 12) * 4.0)).rem_euclid(height + band_h) - band_h / 2.0;
        let peak = (0.05 + hash(b, 13) * 0.07) * gust;
        let (freq, phase) = (1.5 + hash(b, 14) * 2.5, hash(b, 15) * 6.28);
        let mut m = egui::Mesh::default();
        for c in 0..=COLS {
            let f = c as f32 / COLS as f32;
            let x = rect.left() + f * width;
            let roll = 0.5 + 0.5 * (f * freq * 6.28 - travel * 0.35 + phase).sin();
            let clear = sand(0.0);
            m.colored_vertex(Pos2::new(x, cy - band_h / 2.0), clear);
            m.colored_vertex(Pos2::new(x, cy), sand(peak * roll));
            m.colored_vertex(Pos2::new(x, cy + band_h / 2.0), clear);
            if c > 0 {
                let v = (c as u32) * 3;
                let p = v - 3;
                m.add_triangle(p, v, p + 1);
                m.add_triangle(v, v + 1, p + 1);
                m.add_triangle(p + 1, v + 1, p + 2);
                m.add_triangle(v + 1, v + 2, p + 2);
            }
        }
        painter.add(egui::Shape::mesh(m));
    }

    // streaks: fast, short lines, longer when the wind is stronger
    for i in 0..70u32 {
        let speed = 260.0 + hash(i, 20) * 340.0;
        let x = rect.left() + (hash(i, 21) * width + travel * speed).rem_euclid(width + 60.0) - 30.0;
        let y = rect.top() + (hash(i, 22) * height + (t * (0.8 + hash(i, 23)) + hash(i, 24) * 6.28).sin() * 10.0 + t * 12.0).rem_euclid(height);
        let len = (8.0 + hash(i, 25) * 22.0) * gust;
        let a = (0.12 + hash(i, 26) * 0.25) * (0.6 + 0.4 * gust);
        painter.line_segment([Pos2::new(x - len, y - len * 0.08), Pos2::new(x, y)], Stroke::new(0.8 + hash(i, 27), sand(a)));
    }

    // fine grains: slower, tumbling
    for i in 0..110u32 {
        let speed = 90.0 + hash(i, 30) * 160.0;
        let x = rect.left() + (hash(i, 31) * width + travel * speed).rem_euclid(width);
        let y = rect.top() + (hash(i, 32) * height + (t * (1.5 + hash(i, 33) * 2.0) + hash(i, 34) * 6.28).sin() * 14.0 + t * 20.0).rem_euclid(height);
        let a = 0.25 + hash(i, 35) * 0.45;
        painter.circle_filled(Pos2::new(x, y), 0.6 + hash(i, 36) * 1.3, sand(a));
    }
}
