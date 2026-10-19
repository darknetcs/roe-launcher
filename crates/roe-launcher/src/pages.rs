//! All pages and widgets.
use crate::app::{App, Page, Pick, ServerEdit, Src};
use crate::assets;
use crate::theme::{self, Palette};
use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, Margin, RichText, Sense, Stroke, Ui};
use roe_core::config::{Background, Effect, ProtonChoice};
use roe_core::game::Build;
use roe_core::proton::{self, Kind};
use roe_core::options::{self, ServerOption};
use roe_core::servers::{self, ServerEntry, Source};
use roe_core::{lan, update};
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
pub enum SrvState {
    Stopped,
    Starting,
    Running,
    External,
    Crashed(i64),
}

enum Action {
    Play,
    InstallGame,
    ServerOnly,
    StopServer,
    StopGame,
}

enum BrowserAction {
    Join(ServerEntry),
    Favorite(String),
    Edit(usize),
    Remove(usize),
    Add,
    Refresh,
}

fn short(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        format!("…{}", s.chars().skip(n - max + 1).collect::<String>())
    }
}

/// Keeps the beginning of a text (for messages); `short` keeps the end (for paths).
fn head(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

/// The name of the page with the game folder (Proton exists only on Linux).
pub const GAME_PAGE: &str = if roe_core::NEEDS_PROTON { "Game & Proton" } else { "Game" };

fn open_path(p: &Path) {
    let opener = if cfg!(windows) { "explorer" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(p).spawn();
}

fn dot(ui: &mut Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 5.5, color);
}

fn card<R>(ui: &mut Ui, pal: &Palette, opacity: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(pal.card.gamma_multiply(opacity))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(16))
        .stroke(Stroke::new(1.0_f32, pal.accent.gamma_multiply(0.20)))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// One selectable background in the picker: preview, border (accent when selected) and a caption.
fn bg_tile(ui: &mut Ui, pal: &Palette, tex: Option<&egui::TextureHandle>, label: &str, selected: bool) -> egui::Response {
    let size = vec2(176.0, 99.0);
    ui.vertical(|ui| {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        match tex {
            Some(t) => egui::Image::new((t.id(), size)).corner_radius(CornerRadius::same(8)).paint_at(ui, rect),
            None => theme::paint_gradient(ui.painter(), rect, pal.bg_top, pal.bg_bottom),
        }
        let stroke = if selected { Stroke::new(3.0_f32, pal.accent) } else { Stroke::new(1.0_f32, pal.muted.gamma_multiply(0.4)) };
        ui.painter().rect_stroke(rect, 8.0, stroke, egui::StrokeKind::Outside);
        ui.label(RichText::new(short(label, 24)).size(12.5).color(if selected { pal.accent } else { pal.muted }));
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
}

/// A fixed-width column cell with a normal top-down layout (rows would otherwise inherit "horizontal").
fn cell(ui: &mut Ui, w: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(w, 0.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
        ui.set_width(w);
        add(ui)
    });
}

/// Yellow-and-black hazard tape across the page, with the caption repeated along it.
fn hazard_tape(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    let yellow = Color32::from_rgb(255, 204, 0);
    p.rect_filled(rect, 0.0, yellow);
    let (h, step) = (rect.height(), 30.0);
    let mut x = rect.left() - h;
    while x < rect.right() {
        let stripe = vec![pos2(x, rect.bottom()), pos2(x + h, rect.top()), pos2(x + h + step / 2.0, rect.top()), pos2(x + step / 2.0, rect.bottom())];
        p.add(egui::Shape::convex_polygon(stripe, Color32::BLACK, Stroke::NONE));
        x += step;
    }
    let galley = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(17.0), Color32::BLACK);
    let plate_size = galley.size() + vec2(26.0, 4.0);
    let count = ((rect.width() / 380.0).floor() as usize).max(1);
    for i in 0..count {
        let cx = rect.left() + rect.width() * (i as f32 + 0.5) / count as f32;
        let plate = egui::Rect::from_center_size(pos2(cx, rect.center().y), plate_size);
        p.rect_filled(plate, 2.0, yellow);
        p.galley(plate.center() - galley.size() / 2.0, galley.clone(), Color32::BLACK);
    }
}

/// A settings row: the label in a fixed column, the controls after it, everything centred on one line
/// as tall as a button (a plain `horizontal` starts shorter, so labels would sit higher than buttons).
fn form_row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    form_row_w(ui, label, 120.0, add)
}

/// `form_row` with a label column of the given width.
fn form_row_w(ui: &mut Ui, label: &str, label_w: f32, add: impl FnOnce(&mut Ui)) {
    let h = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
    let centred = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), h), centred, |ui| {
        ui.set_min_height(h);
        ui.allocate_ui_with_layout(vec2(label_w, h), centred, |ui| {
            ui.set_width(label_w);
            ui.label(label);
        });
        add(ui);
    });
}

/// Letter height of a tab title in points (what the old GIMP-made header images showed).
const HEADER_LETTERS: f32 = 20.8;
/// Gemina 2's letters are this share of the font size tall.
const GEMINA_LETTER: f32 = 0.663;
/// Extra space between letters, as a share of the font size (the old headers were set wider than the font).
const HEADER_SPACING: f32 = 0.04;

/// A tab title in the launcher's header style: the name in Gemina 2 between two accent-coloured triangles with a
/// soft shadow. Gemina's lower-case letters are its clean small capitals, so the text is drawn in lower case.
/// Drawn live (no images); it shrinks to fit a narrow window.
fn header(ui: &mut Ui, pal: &Palette, text: &str) {
    let family = egui::FontFamily::Name(assets::PLAY_FAMILY.into());
    let layout = |size: f32| {
        let mut job = egui::text::LayoutJob::default();
        let format = egui::TextFormat { font_id: egui::FontId::new(size, family.clone()), color: pal.text, extra_letter_spacing: size * HEADER_SPACING, ..Default::default() };
        job.append(&text.to_lowercase(), 0.0, format);
        ui.fonts(|f| f.layout_job(job))
    };
    // triangle and gap sizes follow the letters: triangles a fifth taller, reaching a little below the baseline
    let parts = |size: f32, text_w: f32| {
        let letters = size * GEMINA_LETTER;
        let (tri_h, gap) = (letters * 1.2, letters * 0.48);
        let tri_w = tri_h * 1.155; // equilateral
        (letters, tri_h, tri_w, gap, text_w + 2.0 * (tri_w + gap))
    };
    let mut size = HEADER_LETTERS / GEMINA_LETTER;
    let mut galley = layout(size);
    let text_w = |g: &std::sync::Arc<egui::Galley>| g.size().x;
    let total = parts(size, text_w(&galley)).4;
    if total > ui.available_width() {
        size *= ui.available_width() / total;
        galley = layout(size);
    }
    let (letters, tri_h, tri_w, gap, total) = parts(size, text_w(&galley));
    let shadow = 3.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total + shadow, tri_h + shadow), Sense::hover());
    let painter = ui.painter();
    let baseline = rect.top() + tri_h - letters * 0.1;
    // where the first letter's baseline sits inside the laid-out text
    let galley_baseline = galley.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(letters);
    let triangle = |x: f32| {
        let (top, bottom) = (rect.top(), rect.top() + tri_h);
        for (off, alpha) in [(2.5, 40u8), (1.5, 70)] {
            let pts = vec![pos2(x + off, bottom + off), pos2(x + tri_w + off, bottom + off), pos2(x + tri_w / 2.0 + off, top + off)];
            painter.add(egui::Shape::convex_polygon(pts, Color32::from_black_alpha(alpha), Stroke::NONE));
        }
        let pts = vec![pos2(x, bottom), pos2(x + tri_w, bottom), pos2(x + tri_w / 2.0, top)];
        painter.add(egui::Shape::convex_polygon(pts, pal.accent, Stroke::NONE));
    };
    triangle(rect.left());
    // drawn three times within a point: as heavy as the GIMP-made headers (egui's text is lighter)
    let at = pos2(rect.left() + tri_w + gap, baseline - galley_baseline);
    for dx in [0.0, 0.45, 0.9] {
        painter.galley(at + vec2(dx, 0.0), galley.clone(), pal.text);
    }
    triangle(rect.left() + total - tri_w);
}

fn muted(ui: &mut Ui, pal: &Palette, t: impl Into<String>) {
    ui.label(RichText::new(t.into()).color(pal.muted));
}

fn status_line(ui: &mut Ui, color: Color32, text: impl Into<String>) {
    ui.horizontal(|ui| {
        dot(ui, color);
        ui.label(text.into());
    });
}

impl App {
    pub fn ui(&mut self, ctx: &egui::Context) {
        let pal = theme::palette(&self.cfg.theme.preset, self.cfg.theme.accent, self.cfg.theme.text_color);
        theme::apply(ctx, &pal, self.cfg.theme.ui_scale);
        self.paint_background(ctx, &pal);
        self.sidebar(ctx, &pal);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::TRANSPARENT).inner_margin(Margin::symmetric(26, 18)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.page {
                    Page::Play => self.page_play(ui, &pal),
                    Page::Server => self.page_server(ui, &pal),
                    Page::Game => self.page_game(ui, &pal),
                    Page::Updates => self.page_updates(ui, &pal),
                    Page::Appearance => self.page_appearance(ui, &pal),
                    Page::Logs => self.page_logs(ui, &pal),
                    Page::ServerBrowser => self.page_server_browser(ui, &pal),
                });
            });
    }

    fn paint_background(&self, ctx: &egui::Context, pal: &Palette) {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let rect = ctx.screen_rect();
        match self.bg.as_ref().and_then(|b| b.1.as_ref()) {
            Some(tex) => theme::paint_cover(&painter, rect, tex, self.cfg.theme.bg_dim),
            None => theme::paint_gradient(&painter, rect, pal.bg_top, pal.bg_bottom),
        }
        let t = ctx.input(|i| i.time);
        match self.cfg.theme.effect {
            Effect::Off => {}
            Effect::Snow => theme::paint_snow(&painter, rect, t),
            Effect::Sandstorm => theme::paint_sandstorm(&painter, rect, t),
        }
    }

    /// The tab's title, drawn live in the header style (see `header`).
    fn page_title(&self, ui: &mut Ui, pal: &Palette, text: &str) {
        header(ui, pal, text);
    }

    // ---------- derived state ----------
    pub fn server_state(&self) -> SrvState {
        match &self.server {
            Some(h) if h.is_running() => {
                if self.ports.all_up() {
                    SrvState::Running
                } else {
                    SrvState::Starting
                }
            }
            Some(h) => match h.exit_code() {
                // -15 = ended by SIGTERM from outside (e.g. the session ending): not a crash
                Some(c) if c != 0 && c != -15 && !h.stop_requested() => SrvState::Crashed(c),
                _ if self.ports.proxy => SrvState::External,
                _ => SrvState::Stopped,
            },
            None if self.ports.proxy => SrvState::External,
            None => SrvState::Stopped,
        }
    }

    fn update_available(&self) -> bool {
        matches!(&self.remote, Some(Ok(r)) if update::update_available(self.info.as_ref(), r))
    }

    // ---------- sidebar ----------
    fn sidebar(&mut self, ctx: &egui::Context, pal: &Palette) {
        let op = self.cfg.theme.panel_opacity;
        egui::SidePanel::left("nav")
            .exact_width(214.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(pal.panel.gamma_multiply(op)).inner_margin(Margin::same(14)))
            .show(ctx, |ui| {
                ui.add_space(2.0);
                ui.vertical_centered(|ui| {
                    let w = 92.0;
                    ui.add(egui::Image::new((self.logo.id(), vec2(w, w * assets::LOGO_ASPECT))));
                });
                ui.add_space(14.0);
                let upd = self.update_available() || self.launcher_update_available();
                let items = [
                    (Page::Play, "Play Locally".to_string()),
                    (Page::ServerBrowser, "Server Browser".to_string()),
                    (Page::Server, "Server Settings".to_string()),
                    (Page::Game, GAME_PAGE.to_string()),
                    (Page::Updates, if upd { "Updates  •".to_string() } else { "Updates".to_string() }),
                    (Page::Appearance, "Appearance".to_string()),
                    (Page::Logs, "Logs".to_string()),
                ];
                for (page, label) in items {
                    let sel = self.page == page;
                    let btn = egui::Button::new(RichText::new(label).size(16.0).color(if sel { pal.accent } else { pal.text }))
                        .fill(if sel { pal.accent.gamma_multiply(0.20) } else { Color32::TRANSPARENT })
                        .stroke(if sel { Stroke::new(1.0_f32, pal.accent.gamma_multiply(0.6)) } else { Stroke::NONE });
                    if ui.add_sized([ui.available_width(), 40.0], btn).clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(4.0);
                    let (st, col) = self.server_badge(pal);
                    status_line(ui, col, st);
                    muted(ui, pal, "Status");
                    ui.add_space(10.0);
                    // community and source links
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                        if ui.add_sized([w, 30.0], egui::Button::new("Discord")).on_hover_text(crate::app::DISCORD_INVITE).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(crate::app::DISCORD_INVITE));
                        }
                        if ui.add_sized([w, 30.0], egui::Button::new("GitHub")).on_hover_text(crate::app::GITHUB_URL).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(crate::app::GITHUB_URL));
                        }
                    });
                });
            });
    }

    fn server_badge(&self, pal: &Palette) -> (String, Color32) {
        match self.server_state() {
            SrvState::Running => ("Server running".into(), pal.good),
            SrvState::External => ("Server running (external)".into(), pal.good),
            SrvState::Starting => ("Server starting…".into(), pal.warn),
            SrvState::Crashed(c) => (format!("Server stopped ({c})"), pal.bad),
            SrvState::Stopped => ("Server stopped".into(), pal.muted),
        }
    }

    fn port_pills(&self, ui: &mut Ui, pal: &Palette) {
        let p = self.ports;
        for (name, on) in [("Lobby 9001", p.lobby), ("Proxy 8001", p.proxy), ("UDP 18001", p.udp)] {
            ui.horizontal(|ui| {
                dot(ui, if on { pal.good } else { pal.muted.gamma_multiply(0.6) });
                ui.label(RichText::new(name).color(if on { pal.text } else { pal.muted }));
            });
        }
    }

    fn player_name_card(&mut self, ui: &mut Ui, pal: &Palette) {
        ui.label(RichText::new("Player name").strong());
        ui.add(
            egui::TextEdit::singleline(&mut self.cfg.player_name)
                .char_limit(roe_core::playername::MAX_LEN)
                .desired_width(260.0)
                .hint_text("BRING_BACK_ROE!!"),
        );
        match roe_core::playername::validate(&self.cfg.player_name) {
            Err(e) => {
                ui.colored_label(pal.bad, format!("{e} The original names are used until it is fixed."));
            }
            Ok(_) => muted(
                ui,
                pal,
                "Your name in the lobby, on the map selection, in the newspaper and on the stats screen. Everyone starts as \
                 BRING_BACK_ROE!!, the original server's name. Takes effect the next time the server starts.",
            ),
        }
    }

    /// Map selector (see `roe_core::maps`), ordered from working to most broken. Supported and experimental maps can
    /// be picked; broken ones are shown greyed out with what went wrong. Applied when the server starts, together with the player name.
    fn map_card(&mut self, ui: &mut Ui, pal: &Palette) {
        use roe_core::maps::{self, Support};
        ui.label(RichText::new("Map").strong());
        ui.add_space(4.0);
        let current = maps::playable_id(self.cfg.map_id);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
            for m in maps::MAPS {
                let pickable = m.selectable();
                let selected = m.id == Some(current);
                let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 104.0), if pickable { Sense::click() } else { Sense::hover() });
                let p = ui.painter();
                let fill = if selected { pal.accent.gamma_multiply(0.22) } else { pal.panel.gamma_multiply(0.85) };
                let stroke = if selected {
                    Stroke::new(2.0_f32, pal.accent)
                } else if pickable && resp.hovered() {
                    Stroke::new(1.0_f32, pal.accent.gamma_multiply(0.6))
                } else {
                    Stroke::new(1.0_f32, pal.muted.gamma_multiply(0.35))
                };
                p.rect(rect, CornerRadius::same(8), fill, stroke, egui::StrokeKind::Inside);
                let text = if pickable { pal.text } else { pal.muted };
                let at = rect.left_top() + vec2(12.0, 10.0);
                p.text(at, egui::Align2::LEFT_TOP, m.name, egui::FontId::proportional(17.0), text);
                let id_text = m.id.map(|i| format!("Map {i}")).unwrap_or_else(|| "Map ID unknown".into());
                p.text(at + vec2(0.0, 24.0), egui::Align2::LEFT_TOP, id_text, egui::FontId::proportional(11.0), pal.muted);
                p.text(at + vec2(0.0, 38.0), egui::Align2::LEFT_TOP, m.world, egui::FontId::proportional(11.0), pal.muted);
                let colour = match m.support {
                    Support::Supported => pal.good,
                    Support::Experimental => pal.warn,
                    Support::Broken => pal.bad,
                };
                let (badge, hint) = (m.badge, m.note);
                p.text(rect.left_bottom() + vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, badge, egui::FontId::proportional(12.5), colour);
                if selected {
                    p.text(rect.right_bottom() + vec2(-12.0, -12.0), egui::Align2::RIGHT_BOTTOM, "✔ Selected", egui::FontId::proportional(12.5), pal.accent);
                }
                let resp = resp.on_hover_text(hint);
                if pickable && resp.clicked() {
                    self.cfg.map_id = m.id.unwrap_or(maps::DIONE);
                }
            }
        });
        ui.add_space(4.0);
        match maps::by_id(current) {
            Some(m) if m.support == Support::Experimental => {
                ui.colored_label(
                    pal.warn,
                    format!(
                        "{} is experimental: the server still uses Dione's spawn points, loot, zones and evacuation. \
                         If the match misbehaves, switch back to Dione.",
                        m.name
                    ),
                );
            }
            _ => muted(ui, pal, "Takes effect the next time the server starts. More maps unlock as the community server supports them."),
        }
    }

    // ---------- Play ----------
    fn page_play(&mut self, ui: &mut Ui, pal: &Palette) {
        header(ui, pal, "Ring of Elysium");
        ui.add_space(2.0);
        ui.label(RichText::new("Local server launcher").size(16.0).color(pal.muted));
        ui.add_space(14.0);
        let op = self.cfg.theme.panel_opacity;
        let w = ((ui.available_width() - 14.0) / 2.0).floor();

        ui.horizontal_top(|ui| {
            cell(ui, w, |ui| card(ui, pal, op, |ui| self.card_game(ui, pal)));
            cell(ui, w, |ui| card(ui, pal, op, |ui| self.card_proton(ui, pal)));
        });
        ui.horizontal_top(|ui| {
            cell(ui, w, |ui| card(ui, pal, op, |ui| self.card_server(ui, pal)));
            cell(ui, w, |ui| card(ui, pal, op, |ui| self.card_updates(ui, pal)));
        });
        ui.add_space(8.0);

        let blockers = self.play_blockers();
        let busy = self.busy();
        let game_up = self.client_running();
        let mut action = None;
        ui.horizontal(|ui| {
            // no game found: the main button installs it through Steam instead
            let install = !self.game_ok();
            let label = if game_up {
                "GAME RUNNING"
            } else if busy {
                "WORKING…"
            } else if install && self.install_wait.is_some() {
                "INSTALLING…"
            } else if install {
                // this exact mix of cases: some Gemina capitals look wrong
                "INSTALL vIA StEAm"
            } else {
                "PLAY"
            };
            // the long install label in a smaller size, so the button keeps PLAY's proportions
            let size = if install { 20.0 } else { 34.0 };
            let text = RichText::new(label).size(size).family(egui::FontFamily::Name(assets::PLAY_FAMILY.into())).color(pal.text);
            let btn = egui::Button::new(text)
                .fill(pal.accent)
                .min_size(vec2(250.0, 60.0))
                .corner_radius(CornerRadius::same(14));
            if install {
                let r = ui.add_enabled(self.install_wait.is_none() && !busy, btn).on_hover_text("Opens Steam's install dialog for Ring of Elysium (it must be in your Steam library)");
                if r.clicked() {
                    action = Some(Action::InstallGame);
                }
            } else if ui.add_enabled(blockers.is_empty() && !busy && !game_up, btn).clicked() {
                action = Some(Action::Play);
            }
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy && !self.server_running() && self.game_ok(), egui::Button::new("Start server only")).clicked() {
                        action = Some(Action::ServerOnly);
                    }
                    if ui.add_enabled(self.server_running(), egui::Button::new("Stop server")).clicked() {
                        action = Some(Action::StopServer);
                    }
                    if ui.add_enabled(game_up, egui::Button::new("Stop game")).clicked() {
                        action = Some(Action::StopGame);
                    }
                });
                if let Some(s) = &self.stage {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(s);
                    });
                }
                // an experimental map is easy to forget once it's picked on Server Settings
                let map = roe_core::maps::playable_id(self.cfg.map_id);
                if let Some(m) = roe_core::maps::by_id(map).filter(|m| m.support == roe_core::maps::Support::Experimental) {
                    ui.colored_label(pal.warn, format!("⚠ Map: {} (experimental: {}). Change it on Server Settings.", m.name, m.badge.to_lowercase()))
                        .on_hover_text(m.note);
                }
            });
        });
        for b in &blockers {
            ui.colored_label(pal.warn, b);
        }
        if !self.game_ok() {
            self.steam_install_help(ui, pal);
        }
        if let Some(e) = &self.flow_error {
            ui.colored_label(pal.bad, e);
        }

        ui.add_space(10.0);
        card(ui, pal, op, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Recent activity").strong());
            let n = self.logs.len();
            for (src, t) in self.logs.iter().skip(n.saturating_sub(7)) {
                let tag = match src {
                    Src::Launcher => ("launcher", pal.accent),
                    Src::Server => ("server", pal.good),
                    Src::Client => ("game", pal.warn),
                };
                // one label per line: compact, and the source tag and message stay on one baseline
                let mono = egui::FontId::monospace(13.0);
                let mut job = egui::text::LayoutJob::default();
                job.append(&format!("{:>8}  ", tag.0), 0.0, egui::TextFormat::simple(mono.clone(), tag.1));
                job.append(&head(t, 110), 0.0, egui::TextFormat::simple(mono, pal.muted));
                ui.label(job);
            }
        });

        match action {
            Some(Action::Play) => self.start_flow(true),
            Some(Action::InstallGame) => self.request_game_install(),
            Some(Action::ServerOnly) => self.start_flow(false),
            Some(Action::StopServer) => self.stop_server(),
            Some(Action::StopGame) => self.stop_game(),
            None => {}
        }
    }

    fn card_game(&self, ui: &mut Ui, pal: &Palette) {
        ui.set_min_size(vec2(ui.available_width(), 118.0));
        ui.label(RichText::new("GAME").size(12.5).color(pal.muted));
        match &self.cfg.game_dir {
            Some(p) if self.game_ok() => {
                ui.label(RichText::new("Ring of Elysium").size(19.0).strong());
                muted(ui, pal, short(&p.display().to_string(), 46));
                match self.build.as_ref().map(|b| &b.1) {
                    Some(Ok(Build::Supported)) => status_line(ui, pal.good, "Supported 2022 build"),
                    Some(Ok(Build::Different(_))) => status_line(ui, pal.warn, "Different build: the server may not work"),
                    Some(Err(e)) => status_line(ui, pal.warn, format!("Could not check: {e}")),
                    None => status_line(ui, pal.muted, "Checking the executable…"),
                }
            }
            _ => {
                ui.label(RichText::new("Not found").size(19.0).strong().color(pal.bad));
                muted(ui, pal, format!("Install it with the button below, or choose the folder in {GAME_PAGE}"));
            }
        }
    }

    fn card_proton(&self, ui: &mut Ui, pal: &Palette) {
        ui.set_min_size(vec2(ui.available_width(), 118.0));
        if !roe_core::NEEDS_PROTON {
            ui.label(RichText::new("PLATFORM").size(12.5).color(pal.muted));
            ui.label(RichText::new("Windows").size(19.0).strong());
            muted(ui, pal, "The game starts directly");
            status_line(ui, pal.good, "No Proton needed");
            return;
        }
        ui.label(RichText::new("PROTON").size(12.5).color(pal.muted));
        match self.resolved_proton() {
            Some(rt) => {
                ui.label(RichText::new(&rt.display).size(19.0).strong());
                muted(ui, pal, format!("{}{}", rt.kind.label(), if rt.alias { " (auto-updating alias)" } else { "" }));
                if rt.name == "GE-Proton11-7" {
                    status_line(ui, pal.good, "Known to work");
                } else {
                    status_line(ui, pal.muted, "Not the tested version, but usually fine");
                }
            }
            None => {
                ui.label(RichText::new("None found").size(19.0).strong().color(pal.bad));
                muted(ui, pal, "Install GE-Proton (e.g. with ProtonUp-Qt)");
            }
        }
    }

    fn card_server(&self, ui: &mut Ui, pal: &Palette) {
        ui.set_min_size(vec2(ui.available_width(), 118.0));
        ui.label(RichText::new("SERVER").size(12.5).color(pal.muted));
        let (t, c) = self.server_badge(pal);
        ui.horizontal(|ui| {
            dot(ui, c);
            ui.label(RichText::new(t).size(19.0).strong());
        });
        ui.horizontal(|ui| {
            let p = self.ports;
            for (n, on) in [("lobby", p.lobby), ("proxy", p.proxy), ("udp", p.udp)] {
                dot(ui, if on { pal.good } else { pal.muted.gamma_multiply(0.6) });
                ui.label(RichText::new(n).color(pal.muted));
            }
        });
    }

    fn card_updates(&self, ui: &mut Ui, pal: &Palette) {
        ui.set_min_size(vec2(ui.available_width(), 118.0));
        ui.label(RichText::new("SERVER FILES").size(12.5).color(pal.muted));
        let installed = self.info.is_some();
        match &self.remote {
            _ if self.checking => status_line(ui, pal.muted, "Checking GitHub…"),
            Some(Ok(r)) if !installed => {
                ui.label(RichText::new("Not downloaded yet").size(19.0).strong().color(pal.warn));
                muted(ui, pal, format!("Latest: {}", head(&r.message, 46)));
            }
            Some(Ok(r)) if self.update_available() => {
                ui.label(RichText::new("Update available").size(19.0).strong().color(pal.warn));
                muted(ui, pal, head(&r.message, 56));
            }
            Some(Ok(_)) => {
                ui.label(RichText::new("Up to date").size(19.0).strong().color(pal.good));
                muted(ui, pal, self.info.as_ref().map(|i| format!("commit {}", &i.sha[..i.sha.len().min(8)])).unwrap_or_default());
            }
            Some(Err(_)) => {
                ui.label(RichText::new("Offline").size(19.0).strong().color(pal.warn));
                muted(ui, pal, if installed { "Using the installed copy" } else { "Needed for the first start" });
            }
            None => muted(ui, pal, if installed { "Installed" } else { "Will be downloaded on first start" }),
        }
    }

    // ---------- Server ----------
    fn page_server(&mut self, ui: &mut Ui, pal: &Palette) {
        self.page_title(ui, pal, "Server Settings");
        muted(ui, pal, "The lobby emulator, proxy and scene endpoint that the game connects to.");
        ui.add_space(8.0);
        let op = self.cfg.theme.panel_opacity;
        let mut start = false;
        let mut stop = false;
        card(ui, pal, op, |ui| {
            let (t, c) = self.server_badge(pal);
            ui.horizontal(|ui| {
                dot(ui, c);
                ui.label(RichText::new(t).size(20.0).strong());
                if let Some(h) = self.server.as_ref().filter(|h| h.is_running()) {
                    let s = h.started.elapsed().as_secs();
                    muted(ui, pal, format!("pid {}  •  up {}m {:02}s", h.pid, s / 60, s % 60));
                }
            });
            ui.add_space(4.0);
            self.port_pills(ui, pal);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy() && !self.server_running() && self.game_ok(), egui::Button::new("Start")).clicked() {
                    start = true;
                }
                if ui.add_enabled(self.server_running(), egui::Button::new("Stop")).clicked() {
                    stop = true;
                }
            });
            if !self.game_ok() {
                ui.colored_label(pal.warn, format!("Select the game folder first ({GAME_PAGE})."));
            }
            if let Some(e) = &self.flow_error {
                ui.colored_label(pal.bad, e);
            }
        });
        ui.add_space(10.0);
        card(ui, pal, op, |ui| self.player_name_card(ui, pal));
        ui.add_space(10.0);
        card(ui, pal, op, |ui| self.map_card(ui, pal));
        ui.add_space(10.0);
        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Files").strong());
            let dir = self.cfg.server_dir_or_default();
            muted(ui, pal, dir.display().to_string());
            match &self.info {
                Some(i) => muted(ui, pal, format!("Installed commit {}{}", &i.sha[..i.sha.len().min(10)], if i.translated { "  •  English" } else { "" })),
                None => muted(ui, pal, "Not installed yet — downloaded automatically on the first start."),
            }
            ui.horizontal(|ui| {
                if ui.button("Choose folder…").clicked() {
                    self.pick(Pick::ServerDir);
                }
                if ui.button("Open folder").clicked() {
                    open_path(&dir);
                }
                if ui.button("Open run logs").clicked() {
                    open_path(&dir.join("runs"));
                }
            });
            ui.label(RichText::new("This folder belongs to the launcher: an update replaces everything in it except your data (.tools, runs, runtime with your saved profiles, reports). Do not point it at a checkout you edit by hand (git checkouts are refused).").size(12.5).color(pal.muted));
        });
        ui.add_space(10.0);
        card(ui, pal, op, |ui| self.match_options(ui, pal));
        ui.add_space(10.0);
        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Session").strong());
            form_row_w(ui, "Session length (seconds)", 200.0, |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.server_duration, 120..=7200));
            });
            form_row_w(ui, "Keep session logs", 200.0, |ui| {
                ui.add(egui::DragValue::new(&mut self.cfg.keep_runs).range(0..=1000).suffix(" newest"));
                muted(ui, pal, if self.cfg.keep_runs == 0 { "keeping all".to_string() } else { "older ones are deleted when the launcher starts (0 = keep all)".to_string() });
            });
            ui.label("Extra arguments for windows_launch.py (anything not listed above)");
            ui.add(egui::TextEdit::singleline(&mut self.cfg.server_extra_args).desired_width(f32::INFINITY).hint_text("e.g. --practice-targets 10 --live-control"));
            ui.label(RichText::new("The run-roe-2022.cmd style works too: -PracticeTargets 10 -LiveControl").size(12.5).color(pal.muted));
        });
        ui.add_space(10.0);
        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Server output").strong());
            self.log_box(ui, pal, Some(Src::Server), 240.0);
        });
        if start {
            self.start_flow(false);
        }
        if stop {
            self.stop_server();
        }
    }

    /// Every option the installed windows_launch.py offers, grouped, with the server's own help text.
    fn match_options(&mut self, ui: &mut Ui, pal: &Palette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Match options").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Reset to launcher defaults").on_hover_text("Glider kit, cycle mode ×3, reliable UDP, idle handoff, winter doors and windows, armored target").clicked() {
                    self.cfg.server_options = options::launcher_defaults();
                }
            });
        });
        muted(ui, pal, "Read from the installed server, so new upstream options appear here after an update. Changes apply the next time the server starts.");
        let schema: Vec<ServerOption> = match &self.schema {
            Some(Ok(s)) => s.clone(),
            Some(Err(e)) => {
                ui.colored_label(pal.warn, format!("Not available: {e}. The launcher's standard match is used until then."));
                return;
            }
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    muted(ui, pal, "Reading the server's options…");
                });
                return;
            }
        };
        if self.server_running() {
            ui.colored_label(pal.warn, "The server is running: changes apply after you stop and start it again.");
        }
        for group in options::GROUPS {
            let list: Vec<&ServerOption> = schema.iter().filter(|o| options::group_of(&o.flag) == group).collect();
            if list.is_empty() {
                continue;
            }
            let open = matches!(group, "Match" | "Targets & bots");
            egui::CollapsingHeader::new(RichText::new(group).strong().color(pal.accent)).default_open(open).show(ui, |ui| {
                for o in list {
                    self.option_row(ui, pal, o);
                }
            });
        }
    }

    fn option_row(&mut self, ui: &mut Ui, pal: &Palette, o: &ServerOption) {
        let values = &mut self.cfg.server_options;
        let mut browse = false;
        form_row_w(ui, &options::label_of(&o.flag), 220.0, |ui| {
            if o.switch {
                let mut on = values.get(&o.flag).map(|v| v == "true").unwrap_or(false);
                if ui.checkbox(&mut on, "").changed() {
                    if on {
                        values.insert(o.flag.clone(), "true".into());
                    } else {
                        values.remove(&o.flag);
                    }
                }
            } else if let Some(choices) = &o.choices {
                let current = values.get(&o.flag).cloned();
                let default = o.default_text();
                let shown = current.clone().unwrap_or_else(|| format!("Server default ({default})"));
                egui::ComboBox::from_id_salt(&o.flag).width(200.0).selected_text(shown).show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_none(), format!("Server default ({default})")).clicked() {
                        values.remove(&o.flag);
                    }
                    for c in choices {
                        if ui.selectable_label(current.as_deref() == Some(c.as_str()), c).clicked() {
                            values.insert(o.flag.clone(), c.clone());
                        }
                    }
                });
            } else {
                let mut text = values.get(&o.flag).cloned().unwrap_or_default();
                let hint = match (o.flag.as_str(), o.default_text()) {
                    ("--loot-catalog", _) => "automatic: the newest bundled catalog".to_string(),
                    (_, d) if d.is_empty() => "server default".to_string(),
                    (_, d) => format!("server default: {d}"),
                };
                let wide = o.ty.as_deref() == Some("Path");
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(if wide { 320.0 } else { 160.0 }).hint_text(hint));
                if r.changed() {
                    if text.trim().is_empty() {
                        values.remove(&o.flag);
                    } else {
                        values.insert(o.flag.clone(), text.clone());
                    }
                }
                if o.is_int() && !text.trim().is_empty() && text.trim().parse::<i64>().is_err() {
                    ui.colored_label(pal.bad, "a whole number");
                }
                if o.flag == "--loot-catalog" && ui.button("Browse…").clicked() {
                    browse = true;
                }
            }
        });
        let help = o.help_en();
        if !help.is_empty() {
            ui.label(RichText::new(help).size(12.0).color(pal.muted));
        }
        if browse {
            self.pick(Pick::LootCatalog);
        }
    }

    // ---------- Game & Proton ----------
    fn page_game(&mut self, ui: &mut Ui, pal: &Palette) {
        self.page_title(ui, pal, GAME_PAGE);
        let op = self.cfg.theme.panel_opacity;
        ui.add_space(6.0);
        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Game folder").strong());
            muted(ui, pal, "The folder that contains Europa_Client.exe. Found automatically in your Steam libraries; set it by hand if yours is elsewhere.");
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 250.0).max(120.0);
                let r = ui.add(egui::TextEdit::singleline(&mut self.game_input).desired_width(w).hint_text("/path/to/Ring of Elysium"));
                if r.lost_focus() {
                    self.set_game_from_text();
                }
                if ui.button("Browse…").clicked() {
                    self.pick(Pick::Game);
                }
                if ui.button("Auto-detect").clicked() {
                    self.cfg.game_dir = None;
                    self.detect();
                }
            });
            match &self.cfg.game_dir {
                Some(p) if self.game_ok() => {
                    status_line(ui, pal.good, "Europa_Client.exe found");
                    if roe_core::game::has_world_data(p) {
                        status_line(ui, pal.good, "World data present");
                    } else {
                        status_line(ui, pal.warn, "SFC/World_EU_ICELAND_4096_RemainingFiles.sfc is missing: the server cannot start");
                    }
                    match self.build.as_ref().map(|b| &b.1) {
                        Some(Ok(Build::Supported)) => status_line(ui, pal.good, "Supported 2022 build"),
                        Some(Ok(Build::Different(h))) => status_line(ui, pal.warn, format!("Different build ({}…): the server only supports the 2022 build", &h[..12])),
                        Some(Err(e)) => status_line(ui, pal.warn, format!("Could not hash the executable: {e}")),
                        None => status_line(ui, pal.muted, "Checking the executable…"),
                    }
                }
                _ => {
                    status_line(ui, pal.bad, "No game folder selected");
                    self.install_with_steam(ui, pal);
                }
            }
            if !self.libs.is_empty() {
                ui.label(RichText::new(format!("Steam libraries scanned: {}", self.libs.iter().map(|l| short(&l.display().to_string(), 36)).collect::<Vec<_>>().join(", "))).size(12.5).color(pal.muted));
            }
        });
        if roe_core::NEEDS_PROTON {
            self.proton_and_prefix_cards(ui, pal);
        }
        ui.add_space(10.0);
        egui::CollapsingHeader::new(RichText::new("Advanced launch options").strong()).show(ui, |ui| {
            card(ui, pal, op, |ui| {
                ui.label("Extra environment variables (one KEY=VALUE per line)");
                let hint = if roe_core::NEEDS_PROTON { "DXVK_HUD=fps\nPROTON_ENABLE_WAYLAND=1" } else { "KEY=VALUE" };
                ui.add(egui::TextEdit::multiline(&mut self.cfg.extra_env).desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text(hint));
                ui.label("Extra game arguments");
                ui.add(egui::TextEdit::singleline(&mut self.cfg.client_extra_args).desired_width(f32::INFINITY).hint_text("-windowed"));
                if roe_core::NEEDS_PROTON {
                    ui.checkbox(&mut self.cfg.proton_log, "Write a Proton log (steam-<appid>.log in the home folder)");
                }
                ui.checkbox(&mut self.cfg.stop_server_on_game_exit, "Stop the server when the game closes");
            });
        });
    }

    /// Linux only: which Proton runs the game, and in which Wine prefix.
    fn proton_and_prefix_cards(&mut self, ui: &mut Ui, pal: &Palette) {
        let op = self.cfg.theme.panel_opacity;
        ui.add_space(10.0);
        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Proton").strong());
            muted(ui, pal, "Read from Steam's compatibility tools. GE-Proton is the default; pick any other, or let a kind track its newest version.");
            let label = self.proton_label();
            let width = (ui.available_width() - 20.0).min(520.0);
            egui::ComboBox::from_id_salt("proton").width(width).selected_text(label).show_ui(ui, |ui| {
                ui.label(RichText::new("Always the newest installed").size(12.5).color(pal.muted));
                for k in [Kind::GeProton, Kind::CachyOs, Kind::ProtonEm, Kind::UmuProton, Kind::Valve] {
                    if let Some(rt) = proton::latest_of(&self.runtimes, k) {
                        let sel = self.cfg.proton == ProtonChoice::Latest(k);
                        if ui.selectable_label(sel, format!("Latest {}  ({})", k.label(), rt.display)).clicked() {
                            self.cfg.proton = ProtonChoice::Latest(k);
                        }
                    }
                }
                ui.separator();
                ui.label(RichText::new("A specific version").size(12.5).color(pal.muted));
                for rt in &self.runtimes {
                    let sel = self.cfg.proton == ProtonChoice::Named(rt.name.clone());
                    let text = format!("{}  —  {}{}", rt.display, rt.kind.label(), if rt.name == "GE-Proton11-7" { "  ✔ tested" } else { "" });
                    if ui.selectable_label(sel, text).clicked() {
                        self.cfg.proton = ProtonChoice::Named(rt.name.clone());
                    }
                }
            });
            if let Some(rt) = self.resolved_proton() {
                muted(ui, pal, short(&rt.dir.display().to_string(), 70));
            } else {
                ui.colored_label(pal.bad, "The selected Proton is not installed.");
            }
            ui.horizontal(|ui| {
                if ui.button("Rescan").clicked() {
                    self.detect();
                }
                if ui.button("Use a custom folder…").clicked() {
                    self.pick(Pick::ProtonDir);
                }
                muted(ui, pal, format!("{} runtimes found", self.runtimes.len()));
            });
        });
        ui.add_space(10.0);
        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Wine prefix").strong());
            let game = self.cfg.game_dir.clone().unwrap_or_default();
            let eff = self.cfg.resolve_prefix(&game);
            muted(ui, pal, format!("{}{}", eff.display(), if self.cfg.compat_data.is_none() { "   (automatic)" } else { "" }));
            ui.horizontal(|ui| {
                if ui.button("Choose…").clicked() {
                    self.pick(Pick::Prefix);
                }
                if ui.add_enabled(self.cfg.compat_data.is_some(), egui::Button::new("Use automatic")).clicked() {
                    self.cfg.compat_data = None;
                }
            });
            ui.label(RichText::new("Automatic = the game's own Steam prefix when there is one, otherwise a private prefix the launcher creates.").size(12.5).color(pal.muted));
        });
    }

    /// Shown when no game is found: ask Steam to install it, then wait for the download.
    fn install_with_steam(&mut self, ui: &mut Ui, pal: &Palette) {
        if self.install_wait.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                muted(ui, pal, "Waiting for Steam to install Ring of Elysium…");
            });
        } else if ui.button("Install with Steam").on_hover_text("Opens Steam's install dialog for Ring of Elysium").clicked() {
            self.request_game_install();
        }
        ui.label(RichText::new("Needs Ring of Elysium in your Steam library.").size(12.5).color(pal.muted));
        self.steam_install_help(ui, pal);
    }

    /// When Steam could not be reached: the manual way and the link to copy.
    fn steam_install_help(&self, ui: &mut Ui, pal: &Palette) {
        if let Some(Err(e)) = &self.steam_install {
            ui.colored_label(pal.bad, format!("Could not open Steam ({e})."));
            ui.label(RichText::new("In Steam: Library > Ring of Elysium > Install, or open this link:").size(12.5).color(pal.muted));
            ui.horizontal(|ui| {
                ui.monospace(roe_core::steam::INSTALL_URL);
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(roe_core::steam::INSTALL_URL.to_string());
                }
            });
        }
    }

    fn proton_label(&self) -> String {
        match &self.cfg.proton {
            ProtonChoice::Latest(k) => match proton::latest_of(&self.runtimes, *k) {
                Some(r) => format!("Latest {}  ({})", k.label(), r.display),
                None => format!("Latest {}  (none installed)", k.label()),
            },
            ProtonChoice::Named(n) => n.clone(),
            ProtonChoice::Path(p) => format!("Custom: {}", short(&p.display().to_string(), 50)),
        }
    }

    // ---------- Updates ----------
    fn page_updates(&mut self, ui: &mut Ui, pal: &Palette) {
        self.page_title(ui, pal, "Updates");
        muted(ui, pal, "Updates for the launcher itself, and for its own copy of ROE-LIVE/ring-of-elysium-local.");
        ui.add_space(8.0);
        let op = self.cfg.theme.panel_opacity;
        card(ui, pal, op, |ui| self.launcher_update_card(ui, pal));
        ui.add_space(10.0);
        let mut check = false;
        let mut upd = false;
        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Server files").strong());
            egui::Grid::new("upd").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
                ui.label("Installed");
                match &self.info {
                    Some(i) => ui.label(format!("{}{}", &i.sha[..i.sha.len().min(10)], if i.translated { "  (translated to English)" } else { "" })),
                    None => ui.label("not installed"),
                };
                ui.end_row();
                ui.label("On GitHub");
                match &self.remote {
                    Some(Ok(r)) => ui.label(format!("{}  {}", &r.sha[..10], r.date)),
                    Some(Err(e)) => ui.colored_label(pal.warn, short(e, 80)),
                    None => ui.label(if self.checking { "checking…" } else { "—" }),
                };
                ui.end_row();
                if let Some(Ok(r)) = &self.remote {
                    if !r.message.is_empty() {
                        ui.label("Latest change");
                        ui.label(&r.message);
                        ui.end_row();
                    }
                }
            });
            ui.add_space(6.0);
            if self.update_available() {
                status_line(ui, pal.warn, "An update is available");
            } else if matches!(self.remote, Some(Ok(_))) {
                status_line(ui, pal.good, "You have the latest version");
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.checking, egui::Button::new("Check now")).clicked() {
                    check = true;
                }
                let can = !self.updating && !self.server_running() && !self.busy();
                let label = if self.info.is_some() { "Update now" } else { "Download server files" };
                if ui.add_enabled(can, egui::Button::new(label).fill(pal.accent.gamma_multiply(0.5))).clicked() {
                    upd = true;
                }
                if self.updating {
                    ui.spinner();
                }
                if self.server_running() {
                    muted(ui, pal, "Stop the server to update.");
                }
            });
        });
        ui.add_space(10.0);
        card(ui, pal, op, |ui| {
            ui.checkbox(&mut self.cfg.check_launcher_updates, "Check for a new launcher version when it starts");
            ui.checkbox(&mut self.cfg.check_updates_on_start, "Check for server file updates when the launcher starts");
            ui.checkbox(&mut self.cfg.translate_after_update, "Translate comments and messages to English after each update");
            let n = self.report.as_ref().map(|r| r.untranslated).or(self.info.as_ref().and_then(|i| i.untranslated));
            if let Some(n) = n {
                muted(ui, pal, format!("{n} lines of text added upstream since the last translation are still Russian (mostly documentation)."));
            }
            ui.add_space(4.0);
            ui.hyperlink_to("Open the repository on GitHub", "https://github.com/ROE-LIVE/ring-of-elysium-local");
        });
        if check {
            self.check_updates();
        }
        if upd {
            self.do_update();
        }
    }

    fn discord_card(&mut self, ui: &mut Ui, pal: &Palette) {
        ui.label(RichText::new("Discord").strong());
        let configured = crate::app::discord_app_id().is_some();
        ui.add_enabled(configured, egui::Checkbox::new(&mut self.cfg.discord_presence, "Show what I'm doing on Discord (Rich Presence)"));
        muted(ui, pal, "Friends see \"Playing Ring of Elysium\" with what you are doing (in the launcher, hosting a server, in a match) and a link to the community. Only while Discord runs on this computer.");
        ui.label(RichText::new("Not showing? In Discord: User Settings > Activity Privacy > \"Share your detected activities with others\" must be on.").size(12.5).color(pal.muted));
        if !configured {
            ui.colored_label(pal.warn, "Not set up in this build yet.");
        } else if self.cfg.discord_presence {
            let s = self.presence_status.lock().unwrap().clone();
            let (c, t) = match s.as_str() {
                "connected to Discord" => (pal.good, "Connected to Discord".to_string()),
                "" => (pal.muted, "Connecting…".to_string()),
                other => (pal.warn, other.to_string()),
            };
            status_line(ui, c, t);
        }
    }

    fn launcher_update_card(&mut self, ui: &mut Ui, pal: &Palette) {
        use roe_core::selfupdate;
        ui.label(RichText::new("Launcher").strong());
        let (mut check, mut install, mut restart) = (false, false, false);
        egui::Grid::new("launcher-upd").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
            ui.label("Installed");
            ui.label(format!("v{}", selfupdate::CURRENT));
            ui.end_row();
            ui.label("On GitHub");
            match &self.launcher_release {
                Some(Ok(r)) => ui.hyperlink_to(&r.tag, r.page()),
                Some(Err(e)) => ui.colored_label(pal.warn, short(e, 80)),
                None => ui.label(if self.launcher_checking { "checking…" } else { "—" }),
            };
            ui.end_row();
        });
        ui.add_space(6.0);
        let blocked = self.launcher_blocked.clone();
        if let Some(tag) = &self.launcher_installed {
            status_line(ui, pal.good, format!("Launcher {tag} is installed: restart to use it"));
        } else if self.launcher_update_available() {
            status_line(ui, pal.warn, "A new launcher version is available");
        } else if matches!(self.launcher_release, Some(Ok(_))) {
            status_line(ui, pal.good, "You have the latest launcher");
        }
        let in_use = self.client_running() || self.server_running();
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.launcher_checking, egui::Button::new("Check now")).clicked() {
                check = true;
            }
            if self.launcher_installed.is_some() {
                let r = ui.add_enabled(!in_use, egui::Button::new("Restart now").fill(pal.accent.gamma_multiply(0.5)));
                if r.on_disabled_hover_text("Close the game and stop the server first").clicked() {
                    restart = true;
                }
            } else if self.launcher_update_available() {
                let can = !self.launcher_updating && blocked.is_none();
                if ui.add_enabled(can, egui::Button::new("Update now").fill(pal.accent.gamma_multiply(0.5))).clicked() {
                    install = true;
                }
            }
            if self.launcher_updating {
                ui.spinner();
            }
            ui.hyperlink_to("Release notes", selfupdate::RELEASES_PAGE);
        });
        if let (Some(why), true) = (&blocked, self.launcher_update_available()) {
            ui.colored_label(pal.warn, format!("Cannot update here: {why}."));
        }
        if let Some(e) = &self.launcher_error {
            ui.colored_label(pal.bad, e);
        }
        if check {
            self.check_launcher();
        }
        if install {
            self.update_launcher();
        }
        if restart {
            self.restart_launcher();
        }
    }

    // ---------- Appearance ----------
    fn page_appearance(&mut self, ui: &mut Ui, pal: &Palette) {
        self.page_title(ui, pal, "Appearance");
        ui.spacing_mut().slider_width = 280.0;
        self.ensure_thumbs();
        ui.add_space(6.0);
        let op = self.cfg.theme.panel_opacity;
        card(ui, pal, op, |ui| {
            form_row(ui, "Theme", |ui| {
                egui::ComboBox::from_id_salt("preset").selected_text(self.cfg.theme.preset.clone()).show_ui(ui, |ui| {
                    for p in theme::PRESETS {
                        if ui.selectable_label(self.cfg.theme.preset == p, p).clicked() {
                            self.cfg.theme.preset = p.to_string();
                            self.cfg.theme.accent = theme::preset_accent(p);
                        }
                    }
                });
            });
            form_row(ui, "Accent colour", |ui| {
                ui.color_edit_button_srgb(&mut self.cfg.theme.accent);
                if ui.button("Reset").clicked() {
                    self.cfg.theme.accent = theme::preset_accent(&self.cfg.theme.preset);
                }
            });
            form_row(ui, "Text colour", |ui| {
                let mut rgb = self.cfg.theme.text_color.unwrap_or(theme::DEFAULT_TEXT);
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    self.cfg.theme.text_color = Some(rgb);
                }
                if ui.add_enabled(self.cfg.theme.text_color.is_some(), egui::Button::new("Reset")).clicked() {
                    self.cfg.theme.text_color = None;
                }
            });
            form_row(ui, "Weather", |ui| {
                let fx = &mut self.cfg.theme.effect;
                ui.selectable_value(fx, Effect::Off, "Off");
                ui.selectable_value(fx, Effect::Snow, "Falling snow");
                ui.selectable_value(fx, Effect::Sandstorm, "Sandstorm");
            });
        });
        ui.add_space(10.0);

        card(ui, pal, op, |ui| self.discord_card(ui, pal));
        ui.add_space(10.0);

        let mut choose: Option<Background> = None;
        let mut add = false;
        let mut delete: Option<String> = None;
        let current = self.cfg.theme.background.clone();
        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Background").strong());
            muted(ui, pal, "Pick one of the built-in pictures or add your own. Added images are copied into the launcher's own folder, so moving or deleting the original never loses your choice.");
            // explicit grid: wrapping is computed from the width, so tiles never overflow
            let mut tiles: Vec<(Background, String)> = vec![(Background::None, "Plain gradient".to_string())];
            tiles.extend((1..=4u8).map(|n| (Background::Bundled(n), assets::bundled_name(n).to_string())));
            tiles.extend(self.custom_bgs.iter().map(|n| (Background::Custom(n.clone()), n.rsplit_once('-').map(|(a, _)| a).unwrap_or(n).to_string())));
            let (tile_w, gap) = (176.0, 14.0);
            let cols = (((ui.available_width() + gap) / (tile_w + gap)).floor() as usize).max(1);
            for row in tiles.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (key, label) in row {
                        let tex = if *key == Background::None { None } else { self.thumbs.get(key) };
                        if bg_tile(ui, pal, tex, label, current == *key).clicked() {
                            choose = Some(key.clone());
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.button("Add image…").clicked() {
                    add = true;
                }
                if let Background::Custom(name) = &current {
                    if ui.button("Remove this image").clicked() {
                        delete = Some(name.clone());
                    }
                }
            });
            if let Some(w) = &self.bg_warning {
                ui.colored_label(pal.warn, w);
            }
            form_row(ui, "Darken image", |ui| {
                ui.add_enabled(current != Background::None, egui::Slider::new(&mut self.cfg.theme.bg_dim, 0.0..=0.9).fixed_decimals(2));
            });
        });
        ui.add_space(10.0);
        card(ui, pal, op, |ui| {
            form_row(ui, "Panel opacity", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.theme.panel_opacity, 0.3..=1.0).fixed_decimals(2));
            });
            form_row(ui, "Interface size", |ui| {
                ui.add(egui::Slider::new(&mut self.cfg.theme.ui_scale, 0.8..=1.6).fixed_decimals(2));
            });
            if ui.button("Reset appearance").clicked() {
                self.cfg.theme = Default::default();
            }
        });
        if let Some(bg) = choose {
            self.cfg.theme.background = bg;
        }
        if add {
            self.pick(Pick::Background);
        }
        if let Some(name) = delete {
            self.remove_custom_background(&name);
        }
    }

    // ---------- Server Browser ----------
    fn page_server_browser(&mut self, ui: &mut Ui, pal: &Palette) {
        self.page_title(ui, pal, "Server Browser");
        muted(ui, pal, "Your local server, servers you saved, launchers found on your network and an optional online list.");
        ui.add_space(8.0);
        hazard_tape(ui, "UNDER CONSTRUCTION");
        ui.add_space(10.0);
        let op = self.cfg.theme.panel_opacity;
        let mut action = None;

        card(ui, pal, op, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.browser.query.search).desired_width(220.0).hint_text("Search name, address or tag"));
                ui.checkbox(&mut self.browser.query.only_favorites, "Favourites");
                ui.checkbox(&mut self.browser.query.hide_full, "Hide full");
                ui.checkbox(&mut self.browser.query.hide_password, "Hide locked");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Add server…").clicked() {
                        action = Some(BrowserAction::Add);
                    }
                    if ui.add_enabled(!self.browser.refreshing, egui::Button::new("Refresh")).clicked() {
                        action = Some(BrowserAction::Refresh);
                    }
                    if self.browser.refreshing {
                        ui.spinner();
                    }
                });
            });
            for e in &self.browser.errors {
                ui.colored_label(pal.warn, e);
            }
        });
        ui.add_space(10.0);

        let all = self.server_list();
        let shown = servers::filter_and_sort(&all, &self.cfg.favorite_servers, &self.browser.query);
        card(ui, pal, op, |ui| {
            if shown.is_empty() {
                muted(ui, pal, if all.is_empty() { "No servers yet." } else { "No servers match the filters." });
                return;
            }
            egui::Grid::new("servers").num_columns(7).striped(true).spacing([14.0, 8.0]).show(ui, |ui| {
                for h in ["", "Name", "Address", "Players", "Mode", "Source", ""] {
                    ui.label(RichText::new(h).size(12.5).color(pal.muted));
                }
                ui.end_row();
                for e in &shown {
                    let fav = self.cfg.favorite_servers.iter().any(|f| f == &e.address || f == &e.id);
                    let star = egui::Button::new(RichText::new("★").size(16.0).color(if fav { pal.accent } else { pal.muted.gamma_multiply(0.5) })).frame(false);
                    if ui.add(star).on_hover_text(if fav { "Remove from favourites" } else { "Add to favourites" }).clicked() {
                        action = Some(BrowserAction::Favorite(e.address.clone()));
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(head(&e.name, 40)).strong());
                            if e.password {
                                ui.label(RichText::new("locked").size(11.5).color(pal.warn));
                            }
                            if e.official {
                                ui.label(RichText::new("official").size(11.5).color(pal.good));
                            }
                        });
                        let mut sub: Vec<String> = e.region.iter().cloned().collect();
                        sub.extend(e.tags.iter().cloned());
                        if !sub.is_empty() {
                            ui.label(RichText::new(head(&sub.join(" · "), 48)).size(11.5).color(pal.muted));
                        }
                    });
                    ui.label(RichText::new(&e.address).monospace().color(pal.muted));
                    ui.label(if e.max_players > 0 { format!("{}/{}", e.players, e.max_players) } else { "—".into() });
                    ui.label(e.mode.as_deref().unwrap_or("—"));
                    ui.label(RichText::new(e.source.label()).color(pal.muted));
                    ui.horizontal(|ui| {
                        let join = ui.add_enabled(servers::can_join(e).is_ok(), egui::Button::new("Join"));
                        let join = match servers::can_join(e) {
                            Ok(()) => join.on_hover_text("Start the local server (if needed) and the game"),
                            Err(why) => join.on_disabled_hover_text(why),
                        };
                        if join.clicked() {
                            action = Some(BrowserAction::Join(e.clone()));
                        }
                        if e.source == Source::Saved {
                            if let Some(i) = self.cfg.saved_servers.iter().position(|s| s.id == e.id) {
                                if ui.small_button("Edit").clicked() {
                                    action = Some(BrowserAction::Edit(i));
                                }
                                if ui.small_button("Remove").clicked() {
                                    action = Some(BrowserAction::Remove(i));
                                }
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
        ui.label(RichText::new("Only the server on this computer can be joined for now: the community server accepts players on its own computer only. Other entries are listed so they are ready once that changes.").size(12.5).color(pal.muted));
        ui.add_space(10.0);

        card(ui, pal, op, |ui| {
            ui.label(RichText::new("Sources").strong());
            ui.label("Online server list (optional)");
            ui.add(egui::TextEdit::singleline(&mut self.cfg.server_directory_url).desired_width(f32::INFINITY).hint_text("https://example.org/servers.json  —  returns {\"servers\": [...]}"));
            ui.add_space(4.0);
            ui.checkbox(&mut self.cfg.lan_search, "Search my local network for other launchers");
            ui.checkbox(&mut self.cfg.lan_visible, format!("Let other launchers on my network find this computer while its server runs (UDP port {})", lan::DISCOVERY_PORT));
            if self.cfg.lan_visible {
                ui.horizontal(|ui| {
                    ui.label("Name shown to others");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.lan_name).desired_width(260.0).hint_text("Ring of Elysium local server"));
                });
                match &self.browser.responder_error {
                    Some(e) => ui.colored_label(pal.warn, e),
                    None => ui.colored_label(pal.good, "Answering searches on the local network"),
                };
            }
        });

        ui.add_space(10.0);
        hazard_tape(ui, "UNDER CONSTRUCTION");

        self.server_edit_window(ui.ctx(), pal);

        match action {
            Some(BrowserAction::Join(e)) => self.join_server(&e),
            Some(BrowserAction::Favorite(a)) => self.toggle_favorite(&a),
            Some(BrowserAction::Edit(i)) => {
                let s = &self.cfg.saved_servers[i];
                self.browser.edit = Some(ServerEdit {
                    index: Some(i),
                    name: s.name.clone(),
                    address: s.address.clone(),
                    region: s.region.clone().unwrap_or_default(),
                    mode: s.mode.clone().unwrap_or_default(),
                    tags: s.tags.join(", "),
                    password: s.password,
                    error: None,
                });
            }
            Some(BrowserAction::Remove(i)) => {
                let s = self.cfg.saved_servers.remove(i);
                self.cfg.favorite_servers.retain(|f| f != &s.address && f != &s.id);
            }
            Some(BrowserAction::Add) => {
                self.browser.edit = Some(ServerEdit { index: None, name: String::new(), address: String::new(), region: String::new(), mode: String::new(), tags: String::new(), password: false, error: None });
            }
            Some(BrowserAction::Refresh) => self.refresh_servers(),
            None => {}
        }
    }

    /// The add / edit dialog for a saved server.
    fn server_edit_window(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(edit) = self.browser.edit.as_mut() else { return };
        let (mut save, mut cancel) = (false, false);
        egui::Window::new(if edit.index.is_some() { "Edit server" } else { "Add server" })
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("server_edit").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    for (label, value, hint) in [
                        ("Name", &mut edit.name, "Friday training"),
                        ("Address", &mut edit.address, "host:port, e.g. 192.168.1.20:8001"),
                        ("Region", &mut edit.region, "EU"),
                        ("Mode", &mut edit.mode, "Training"),
                        ("Tags", &mut edit.tags, "comma separated"),
                    ] {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(value).desired_width(300.0).hint_text(hint));
                        ui.end_row();
                    }
                    ui.label("");
                    ui.checkbox(&mut edit.password, "Needs a password");
                    ui.end_row();
                });
                if let Some(e) = &edit.error {
                    ui.colored_label(pal.bad, e);
                }
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if cancel {
            self.browser.edit = None;
            return;
        }
        if !save {
            return;
        }
        let address = edit.address.trim().to_string();
        if let Err(e) = servers::validate_address(&address) {
            edit.error = Some(format!("Address: {e}. Use host:port, e.g. 192.168.1.20:8001."));
            return;
        }
        let taken = self.cfg.saved_servers.iter().enumerate().any(|(i, s)| Some(i) != edit.index && s.address.eq_ignore_ascii_case(&address));
        if taken || address == roe_core::config::Connection::LOCAL_ADDRESS {
            edit.error = Some("This address is already in the list.".into());
            return;
        }
        let opt = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        let entry = ServerEntry {
            id: edit.index.map(|i| self.cfg.saved_servers[i].id.clone()).unwrap_or_else(|| format!("saved-{address}")),
            name: opt(&edit.name).unwrap_or_else(|| address.clone()),
            address,
            region: opt(&edit.region),
            mode: opt(&edit.mode),
            tags: edit.tags.split(',').filter_map(opt).collect(),
            password: edit.password,
            ..Default::default()
        };
        match edit.index {
            Some(i) => {
                // keep the star when the address changes
                let old = std::mem::replace(&mut self.cfg.saved_servers[i], entry.clone());
                for f in &mut self.cfg.favorite_servers {
                    if *f == old.address {
                        *f = entry.address.clone();
                    }
                }
            }
            None => self.cfg.saved_servers.push(entry),
        }
        self.browser.edit = None;
    }

    // ---------- Logs ----------
    fn page_logs(&mut self, ui: &mut Ui, pal: &Palette) {
        self.page_title(ui, pal, "Logs");
        let op = self.cfg.theme.panel_opacity;
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.log_show[0], "Launcher");
            ui.checkbox(&mut self.log_show[1], "Server");
            ui.checkbox(&mut self.log_show[2], "Game");
            if ui.button("Copy all").clicked() {
                let text = self.logs.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
                ui.ctx().copy_text(text);
            }
            if ui.button("Clear").clicked() {
                self.logs.clear();
            }
        });
        card(ui, pal, op, |ui| {
            let h = (ui.available_height() - 40.0).max(300.0);
            self.log_box(ui, pal, None, h);
        });
    }

    /// `only` = show just that source; None = honour the checkboxes.
    fn log_box(&self, ui: &mut Ui, pal: &Palette, only: Option<Src>, height: f32) {
        let rows: Vec<&(Src, String)> = self
            .logs
            .iter()
            .filter(|(s, _)| match only {
                Some(o) => *s == o,
                None => self.log_show[match s {
                    Src::Launcher => 0,
                    Src::Server => 1,
                    Src::Client => 2,
                }],
            })
            .collect();
        let rh = 16.0;
        egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, rh, rows.len(), |ui, range| {
            for i in range {
                let (src, t) = rows[i];
                let col = match src {
                    Src::Launcher => pal.accent,
                    Src::Server => pal.text,
                    Src::Client => pal.warn,
                };
                ui.add(egui::Label::new(RichText::new(t).monospace().color(col)).wrap_mode(egui::TextWrapMode::Truncate));
            }
        });
    }
}

