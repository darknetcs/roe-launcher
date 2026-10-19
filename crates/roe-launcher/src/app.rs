//! Application state, background workflows and the event loop glue.
use eframe::egui;
use roe_core::backgrounds;
use roe_core::config::{Background, Config, Connection, Effect, ProtonChoice};
use roe_core::game::{self, Build};
use roe_core::process::Handle;
use roe_core::proton::{self, Runtime};
use roe_core::server::{self, Ports};
use roe_core::servers::{self, ServerEntry, ServerSource};
use roe_core::update::{self, InstallInfo, Remote};
use roe_core::options::{self, ServerOption};
use roe_core::selfupdate::{self, Release};
use roe_core::{client, lan, runtime, steam, translate, Sink};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::assets;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Play,
    Server,
    Game,
    Updates,
    Appearance,
    Logs,
    ServerBrowser,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Src {
    Launcher,
    Server,
    Client,
}

#[derive(Clone, Copy, Debug)]
pub enum Pick {
    Game,
    Prefix,
    ServerDir,
    Background,
    ProtonDir,
    LootCatalog,
}

pub enum Event {
    Log(Src, String),
    Stage(Option<String>),
    Remote(Result<Remote, String>),
    Updated(Result<Option<translate::Report>, String>),
    ServerUp(Handle),
    ClientUp(Handle),
    FlowDone(Result<(), String>),
    Build(PathBuf, Result<Build, String>),
    Picked(Pick, Option<PathBuf>),
    /// Servers found by the network sources (LAN, directory), and one message per failed source.
    Servers(Vec<ServerEntry>, Vec<String>),
    /// Background previews decoded off the UI thread (None = the image could not be read).
    Thumbs(Vec<(Background, Option<egui::ColorImage>)>),
    /// The match options of the installed windows_launch.py.
    Schema(Result<Vec<ServerOption>, String>),
    /// The newest launcher release on GitHub.
    LauncherRelease(Result<Release, String>),
    /// A launcher update was installed (its tag) or failed.
    LauncherInstalled(Result<String, String>),
}

/// The add / edit dialog of the Server Browser.
pub struct ServerEdit {
    /// Index in `cfg.saved_servers`; None = a new entry.
    pub index: Option<usize>,
    pub name: String,
    pub address: String,
    pub region: String,
    pub mode: String,
    pub tags: String,
    pub password: bool,
    pub error: Option<String>,
}

/// Server Browser state that is not saved.
#[derive(Default)]
pub struct Browser {
    /// From the network sources; local and saved entries are added when shown.
    pub found: Vec<ServerEntry>,
    pub errors: Vec<String>,
    pub refreshing: bool,
    pub refreshed: bool,
    pub query: servers::Query,
    pub edit: Option<ServerEdit>,
    responder: Option<lan::Responder>,
    pub responder_error: Option<String>,
    /// Shared with the responder thread so a rename applies at once.
    lan_name: Arc<Mutex<String>>,
    /// Random per run: lets a search skip this launcher's own answer.
    instance: String,
}

pub struct App {
    pub cfg: Config,
    saved: Config,
    last_seen: Config,
    changed_at: Instant,
    pub page: Page,
    pub tx: Sender<Event>,
    rx: Receiver<Event>,
    pub ctx: egui::Context,

    // detection
    pub roots: Vec<PathBuf>,
    pub libs: Vec<PathBuf>,
    pub runtimes: Vec<Runtime>,
    pub detected_game: Option<PathBuf>,
    pub game_input: String,
    pub build: Option<(PathBuf, Result<Build, String>)>,
    build_for: Option<PathBuf>,

    // processes
    pub server: Option<Handle>,
    pub client: Option<Handle>,
    pub server_ours: bool,
    pub ports: Ports,
    last_poll: Instant,
    client_was_running: bool,
    pub stage: Option<String>,
    pub flow_error: Option<String>,

    // updates
    pub remote: Option<Result<Remote, String>>,
    pub checking: bool,
    pub updating: bool,
    pub info: Option<InstallInfo>,
    pub report: Option<translate::Report>,
    /// What the installed server's windows_launch.py accepts (None = not read yet).
    pub schema: Option<Result<Vec<ServerOption>, String>>,
    schema_loading: bool,

    // the launcher's own updates
    pub launcher_release: Option<Result<Release, String>>,
    pub launcher_checking: bool,
    pub launcher_updating: bool,
    /// Tag of a newer launcher that is installed and waits for a restart.
    pub launcher_installed: Option<String>,
    pub launcher_error: Option<String>,
    /// Why this launcher cannot replace itself (checked once at start).
    pub launcher_blocked: Option<String>,
    /// This launcher's file, taken at start (an update replaces the file under the running program).
    exe: Option<PathBuf>,

    // logs
    pub logs: VecDeque<(Src, String)>,
    pub log_show: [bool; 3],
    /// The same lines, written to `<data dir>/launcher.log` (replaced on every start).
    log_file: Option<std::io::LineWriter<std::fs::File>>,

    // look & feel
    pub logo: egui::TextureHandle,
    pub name_img: egui::TextureHandle,
    /// Picture headers of the tabs that have one; the others show a text title.
    pub headers: Vec<(Page, egui::TextureHandle)>,
    /// The loaded background: which one, and its texture (None = plain gradient).
    pub bg: Option<(Background, Option<egui::TextureHandle>)>,
    pub bg_warning: Option<String>,
    pub thumbs: HashMap<Background, egui::TextureHandle>,
    /// Previews already asked for (made, failed or on their way), so each is decoded once.
    thumbs_requested: HashSet<Background>,
    pub custom_bgs: Vec<String>,
    /// "Install with Steam": how Steam was asked (or why it could not be), and since when we wait.
    pub steam_install: Option<Result<String, String>>,
    pub install_wait: Option<Instant>,
    last_install_check: Instant,
    /// Steam reports the game as not fully installed yet (downloading or updating).
    steam_installing: bool,
    /// `ROE_SCREENSHOT=<file.png>` (development): save the window after start-up, then close.
    screenshot: Option<PathBuf>,
    frames: u64,
    pub browser: Browser,
    /// Running under Wine / Proton: work around what its Windows emulation lacks (see `wine_fixes`).
    wine: bool,
    #[cfg(windows)]
    uia_blocked: bool,
}

const MAX_LOG: usize = 5000;

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        assets::install_fonts(&cc.egui_ctx);
        let cfg = Config::load();
        let mut app = App {
            saved: cfg.clone(),
            last_seen: cfg.clone(),
            cfg,
            changed_at: Instant::now(),
            page: Page::Play,
            tx,
            rx,
            ctx: cc.egui_ctx.clone(),
            roots: vec![],
            libs: vec![],
            runtimes: vec![],
            detected_game: None,
            game_input: String::new(),
            build: None,
            build_for: None,
            server: None,
            client: None,
            server_ours: false,
            ports: Ports::default(),
            last_poll: Instant::now() - Duration::from_secs(5),
            client_was_running: false,
            stage: None,
            flow_error: None,
            remote: None,
            checking: false,
            updating: false,
            info: None,
            report: None,
            schema: None,
            schema_loading: false,
            launcher_release: None,
            launcher_checking: false,
            launcher_updating: false,
            launcher_installed: None,
            launcher_error: None,
            launcher_blocked: selfupdate::current_exe().and_then(|e| selfupdate::unsupported_reason(&e)),
            exe: selfupdate::current_exe(),
            logs: VecDeque::new(),
            log_show: [true; 3],
            log_file: open_log_file(),
            logo: tex(&cc.egui_ctx, "logo", assets::LOGO),
            name_img: tex(&cc.egui_ctx, "name", assets::NAME),
            headers: page_headers(&cc.egui_ctx),
            bg: None,
            bg_warning: None,
            thumbs: HashMap::new(),
            thumbs_requested: HashSet::new(),
            custom_bgs: backgrounds::list(&backgrounds::dir()),
            steam_install: None,
            install_wait: None,
            last_install_check: Instant::now(),
            steam_installing: false,
            screenshot: std::env::var_os("ROE_SCREENSHOT").map(PathBuf::from),
            frames: 0,
            browser: Browser { instance: instance_id(), ..Default::default() },
            wine: roe_core::under_wine(),
            #[cfg(windows)]
            uia_blocked: false,
        };
        app.migrate_legacy_background();
        // handy for screenshots / development: ROE_START_PAGE=server|game|updates|appearance|logs|browser
        app.page = match std::env::var("ROE_START_PAGE").as_deref() {
            Ok("server") => Page::Server,
            Ok("game") => Page::Game,
            Ok("updates") => Page::Updates,
            Ok("appearance") => Page::Appearance,
            Ok("logs") => Page::Logs,
            Ok("browser") => Page::ServerBrowser,
            _ => Page::Play,
        };
        app.detect();
        app.refresh_info();
        app.load_schema();
        app.prune_session_logs();
        if app.cfg.check_updates_on_start {
            app.check_updates();
        }
        if let Some(exe) = &app.exe {
            selfupdate::cleanup(exe);
        }
        if app.cfg.check_launcher_updates {
            app.check_launcher();
        }
        app.log(Src::Launcher, "Launcher started.".into());
        if app.wine {
            app.log(Src::Launcher, "Running under Wine/Proton: input methods (IME) and screen-reader support are off.".into());
        }
        // development hook: ROE_AUTORUN=update|server|play
        match std::env::var("ROE_AUTORUN").as_deref() {
            Ok("update") => app.do_update(),
            Ok("server") => app.start_flow(false),
            Ok("play") => app.start_flow(true),
            _ => {}
        }
        app
    }

    // ---------- logging / events ----------
    pub fn log(&mut self, src: Src, text: String) {
        if let Some(f) = self.log_file.as_mut() {
            use std::io::Write;
            let tag = match src {
                Src::Launcher => "launcher",
                Src::Server => "server",
                Src::Client => "game",
            };
            let _ = writeln!(f, "[{tag}] {text}");
        }
        self.logs.push_back((src, text));
        while self.logs.len() > MAX_LOG {
            self.logs.pop_front();
        }
    }

    pub fn sink(&self, src: Src) -> Sink {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        Arc::new(move |l| {
            let _ = tx.send(Event::Log(src, l));
            ctx.request_repaint();
        })
    }

    fn emit(tx: &Sender<Event>, ctx: &egui::Context, ev: Event) {
        let _ = tx.send(ev);
        ctx.request_repaint();
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Log(s, t) => self.log(s, t),
                Event::Stage(s) => self.stage = s,
                Event::Remote(r) => {
                    self.checking = false;
                    if let Err(e) = &r {
                        self.log(Src::Launcher, format!("Update check failed: {e}"));
                    }
                    self.remote = Some(r);
                }
                Event::Updated(r) => {
                    self.updating = false;
                    self.refresh_info();
                    match r {
                        Ok(rep) => {
                            if rep.is_some() {
                                self.report = rep;
                            }
                            self.schema = None;
                            self.load_schema();
                            self.log(Src::Launcher, "Server files are up to date.".into());
                            self.check_updates();
                        }
                        Err(e) => self.log(Src::Launcher, format!("Update failed: {e}")),
                    }
                }
                Event::ServerUp(h) => {
                    self.server = Some(h);
                    self.server_ours = true;
                }
                Event::ClientUp(h) => self.client = Some(h),
                Event::FlowDone(r) => {
                    self.stage = None;
                    self.refresh_info();
                    // the first start installs the server's Python, which reads the option list
                    if !matches!(self.schema, Some(Ok(_))) {
                        self.load_schema();
                    }
                    if let Err(e) = r {
                        let e = self.explain_server_error(e);
                        self.log(Src::Launcher, format!("Error: {e}"));
                        self.flow_error = Some(e);
                    }
                }
                Event::LauncherRelease(r) => {
                    self.launcher_checking = false;
                    match &r {
                        Ok(rel) if rel.is_newer_than(selfupdate::CURRENT) => self.log(Src::Launcher, format!("Launcher {} is available (this is v{}).", rel.tag, selfupdate::CURRENT)),
                        Ok(_) => {}
                        Err(e) => self.log(Src::Launcher, format!("Launcher update check failed: {e}")),
                    }
                    self.launcher_release = Some(r);
                }
                Event::LauncherInstalled(r) => {
                    self.launcher_updating = false;
                    match r {
                        Ok(tag) => self.launcher_installed = Some(tag),
                        Err(e) => {
                            self.log(Src::Launcher, format!("Launcher update failed: {e}"));
                            self.launcher_error = Some(e);
                        }
                    }
                }
                Event::Schema(r) => {
                    self.schema_loading = false;
                    if let Err(e) = &r {
                        self.log(Src::Launcher, format!("Server options: {e}"));
                    }
                    self.schema = Some(r);
                }
                Event::Build(p, r) => {
                    if self.cfg.game_dir.as_ref() == Some(&p) {
                        self.build = Some((p, r));
                    }
                }
                Event::Picked(kind, path) => {
                    if let Some(p) = path {
                        self.apply_pick(kind, p);
                    }
                }
                Event::Thumbs(done) => {
                    for (key, img) in done {
                        if let Some(img) = img {
                            self.thumbs.insert(key, self.ctx.load_texture("thumb", img, egui::TextureOptions::LINEAR));
                        }
                    }
                }
                Event::Servers(found, errors) => {
                    self.browser.refreshing = false;
                    self.browser.found = found;
                    for e in &errors {
                        self.log(Src::Launcher, format!("Server Browser: {e}"));
                    }
                    self.browser.errors = errors;
                }
            }
        }
    }

    // ---------- detection ----------
    pub fn detect(&mut self) {
        self.roots = steam::steam_roots();
        self.libs = steam::libraries(&self.roots);
        self.runtimes = proton::detect(&self.roots, &self.libs);
        self.detected_game = if pretend_no_game() { None } else { steam::find_game(&self.libs) };
        // Steam is still downloading or updating it (its manifest says so, even before the exe exists):
        // the files may be half written, so wait
        self.steam_installing = !pretend_no_game() && !steam::install_finished(&self.libs);
        if self.steam_installing {
            self.detected_game = None;
            self.install_wait.get_or_insert_with(Instant::now);
            return;
        }
        if self.cfg.game_dir.as_ref().map(|p| !game::is_game_dir(p)).unwrap_or(true) {
            if let Some(g) = self.detected_game.clone() {
                self.cfg.game_dir = Some(g);
            }
        }
        self.game_input = self.cfg.game_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
    }

    /// Asks Steam to install the game; `tick` then looks for it every few seconds.
    pub fn request_game_install(&mut self) {
        let r = steam::request_install();
        match &r {
            Ok(how) => {
                self.log(Src::Launcher, format!("Asked Steam to install Ring of Elysium ({how}). The launcher will find it when the download finishes."));
                self.install_wait = Some(Instant::now());
            }
            Err(e) => self.log(Src::Launcher, format!("Could not open Steam: {e}")),
        }
        self.steam_install = Some(r);
    }

    pub fn set_game_from_text(&mut self) {
        match game::resolve_input(&self.game_input) {
            Some(p) => {
                self.cfg.game_dir = Some(p.clone());
                self.game_input = p.display().to_string();
            }
            None => self.log(Src::Launcher, format!("Europa_Client.exe not found in: {}", self.game_input)),
        }
    }

    pub fn resolved_proton(&self) -> Option<Runtime> {
        self.cfg.resolve_proton(&self.runtimes)
    }

    pub fn game_ok(&self) -> bool {
        !pretend_no_game() && !self.steam_installing && self.cfg.game_dir.as_deref().map(game::is_game_dir).unwrap_or(false)
    }

    pub fn refresh_info(&mut self) {
        self.info = update::read_info(&self.cfg.server_dir_or_default());
    }

    pub fn server_running(&self) -> bool {
        self.server.as_ref().map(|h| h.is_running()).unwrap_or(false)
    }

    pub fn client_running(&self) -> bool {
        self.client.as_ref().map(|h| h.is_running()).unwrap_or(false)
    }

    pub fn busy(&self) -> bool {
        self.stage.is_some() || self.updating
    }

    pub fn play_blockers(&self) -> Vec<String> {
        let mut v = vec![];
        let page = crate::pages::GAME_PAGE;
        if !self.game_ok() {
            v.push(format!("Select the game folder ({page})."));
        } else if let Some(Err(_)) = self.build.as_ref().map(|b| &b.1) {
            // hash could not be computed; not fatal
        }
        if roe_core::NEEDS_PROTON && self.resolved_proton().is_none() {
            v.push(format!("No Proton runtime found ({page})."));
        }
        v
    }

    // ---------- backgrounds ----------
    /// Old versions stored a path to the image; copy it into the launcher's own folder once.
    fn migrate_legacy_background(&mut self) {
        let Some(p) = self.cfg.theme.bg_image.take() else { return };
        match backgrounds::import(&p, &backgrounds::dir()) {
            Ok(name) => {
                self.cfg.theme.background = Background::Custom(name);
                self.custom_bgs = backgrounds::list(&backgrounds::dir());
                self.log(Src::Launcher, "Your previous background image was copied into the launcher's own folder.".into());
            }
            Err(e) => self.log(Src::Launcher, format!("Could not migrate the old background ({}): {e:#}", p.display())),
        }
    }

    pub fn refresh_custom_bgs(&mut self) {
        self.custom_bgs = backgrounds::list(&backgrounds::dir());
    }

    /// Loads the texture for the selected background when the selection changed.
    /// A missing/unreadable custom file falls back to the default picture but is *kept* in the
    /// settings, so nothing is forgotten if the file comes back.
    fn ensure_background(&mut self, ctx: &egui::Context) {
        let want = self.cfg.theme.background.clone();
        if self.bg.as_ref().map(|b| &b.0) == Some(&want) {
            return;
        }
        self.bg_warning = None;
        let load = |bytes: &[u8]| assets::decode(bytes, 2560).ok().map(|img| ctx.load_texture("background", img, egui::TextureOptions::LINEAR));
        let tex = match &want {
            Background::None => None,
            Background::Bundled(n) => assets::bundled(*n).and_then(load),
            Background::Custom(name) => {
                let found = backgrounds::path_of(&backgrounds::dir(), name).and_then(|p| std::fs::read(p).ok()).and_then(|b| load(&b));
                if found.is_none() {
                    self.bg_warning = Some(format!("The saved background \"{name}\" is missing or unreadable, so the default is shown. Choose another or add the image again."));
                    assets::bundled(1).and_then(load)
                } else {
                    found
                }
            }
        };
        self.bg = Some((want, tex));
    }

    /// Previews for the background picker, made once each in a background thread when the
    /// Appearance page first needs them, so the page opens at once (tiles fill in as they are ready).
    pub fn ensure_thumbs(&mut self) {
        let missing: Vec<Background> = (1..=4u8)
            .map(Background::Bundled)
            .chain(self.custom_bgs.iter().map(|n| Background::Custom(n.clone())))
            .filter(|k| !self.thumbs_requested.contains(k))
            .collect();
        if missing.is_empty() {
            return;
        }
        self.thumbs_requested.extend(missing.iter().cloned());
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let done = missing
                .into_iter()
                .map(|key| {
                    let bytes = match &key {
                        Background::Bundled(n) => assets::bundled(*n).map(<[u8]>::to_vec),
                        Background::Custom(name) => backgrounds::path_of(&backgrounds::dir(), name).and_then(|p| std::fs::read(p).ok()),
                        Background::None => None,
                    };
                    let img = bytes.and_then(|b| assets::thumbnail(&b).ok());
                    (key, img)
                })
                .collect();
            App::emit(&tx, &ctx, Event::Thumbs(done));
        });
    }

    pub fn remove_custom_background(&mut self, name: &str) {
        if let Err(e) = backgrounds::remove(&backgrounds::dir(), name) {
            self.log(Src::Launcher, format!("Could not remove the image: {e:#}"));
        }
        self.thumbs.remove(&Background::Custom(name.to_string()));
        self.thumbs_requested.remove(&Background::Custom(name.to_string()));
        if self.cfg.theme.background == Background::Custom(name.to_string()) {
            self.cfg.theme.background = Background::Bundled(1);
        }
        self.refresh_custom_bgs();
    }

    // ---------- file pickers ----------
    pub fn pick(&self, kind: Pick) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut d = rfd::FileDialog::new();
            d = match kind {
                Pick::Background => d.add_filter("Images", &["png", "jpg", "jpeg", "webp"]).set_title("Choose a background image"),
                Pick::Game => d.set_title("Select the Ring of Elysium folder (contains Europa_Client.exe)"),
                Pick::Prefix => d.set_title("Select the Wine prefix folder"),
                Pick::ServerDir => d.set_title("Select where the server files should live"),
                Pick::ProtonDir => d.set_title("Select a Proton folder (contains the 'proton' script)"),
                Pick::LootCatalog => d.add_filter("Loot catalog", &["json"]).set_title("Choose a loot catalog (JSON)"),
            };
            let res = if matches!(kind, Pick::Background | Pick::LootCatalog) { d.pick_file() } else { d.pick_folder() };
            App::emit(&tx, &ctx, Event::Picked(kind, res));
        });
    }

    fn apply_pick(&mut self, kind: Pick, p: PathBuf) {
        match kind {
            Pick::Game => match game::resolve_input(&p.to_string_lossy()) {
                Some(g) => {
                    self.game_input = g.display().to_string();
                    self.cfg.game_dir = Some(g);
                }
                None => self.log(Src::Launcher, format!("Europa_Client.exe not found in {}", p.display())),
            },
            Pick::Prefix => self.cfg.compat_data = Some(p),
            Pick::ServerDir => {
                self.cfg.server_dir = Some(p);
                self.refresh_info();
            }
            Pick::Background => match backgrounds::import(&p, &backgrounds::dir()) {
                Ok(name) => {
                    self.cfg.theme.background = Background::Custom(name);
                    self.bg = None; // reload (also clears a "file is missing" fallback)
                    self.custom_bgs = backgrounds::list(&backgrounds::dir());
                }
                Err(e) => self.log(Src::Launcher, format!("Could not add the background: {e:#}")),
            },
            Pick::LootCatalog => {
                self.cfg.server_options.insert("--loot-catalog".into(), p.display().to_string());
            }
            Pick::ProtonDir => {
                if proton::runtime_from_dir(&p, false).is_some() {
                    self.cfg.proton = ProtonChoice::Path(p);
                } else {
                    self.log(Src::Launcher, format!("No executable 'proton' script in {}", p.display()));
                }
            }
        }
    }

    // ---------- workflows ----------
    pub fn check_updates(&mut self) {
        if self.checking {
            return;
        }
        self.checking = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = update::remote_head().map_err(|e| format!("{e:#}"));
            App::emit(&tx, &ctx, Event::Remote(r));
        });
    }

    pub fn do_update(&mut self) {
        if self.updating || self.server_running() {
            return;
        }
        self.updating = true;
        let dir = self.cfg.server_dir_or_default();
        let do_tr = self.cfg.translate_after_update;
        let (sink, tx, ctx) = (self.sink(Src::Launcher), self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<Option<translate::Report>> {
                let remote = update::remote_head()?;
                update::install(&dir, &remote.sha, &sink)?;
                translate_step(&dir, do_tr, &sink)
            })();
            App::emit(&tx, &ctx, Event::Updated(res.map_err(|e| format!("{e:#}"))));
        });
    }

    /// Starts the server (if needed) and optionally the game.
    pub fn start_flow(&mut self, with_client: bool) {
        if self.busy() {
            return;
        }
        self.flow_error = None;
        let (Some(game_dir), proton) = (self.cfg.game_dir.clone(), self.resolved_proton()) else { return };
        if with_client && roe_core::NEEDS_PROTON && proton.is_none() {
            return;
        }
        let cfg = self.cfg.clone();
        let known: Option<Vec<ServerOption>> = self.schema.as_ref().and_then(|s| s.as_ref().ok()).cloned();
        // read the ports fresh: the cached status may be up to a second old (or not polled yet)
        self.ports = server::ports();
        let remote = !self.cfg.connection.is_local();
        let already = self.server_running() || self.ports.proxy;
        if !with_client && (remote || already) {
            let msg = if remote { "The connection is set to a remote server: there is no local server to start.".to_string() } else { format!("A server is already running on port {}.", server::PROXY_PORT) };
            self.log(Src::Launcher, msg);
            return;
        }
        self.stage = Some("Starting…".into());
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let (s_sink, c_sink, l_sink) = (self.sink(Src::Server), self.sink(Src::Client), self.sink(Src::Launcher));
        std::thread::spawn(move || {
            let stage = |s: &str| App::emit(&tx, &ctx, Event::Stage(Some(s.to_string())));
            let res = (|| -> anyhow::Result<()> {
                let dir = cfg.server_dir_or_default();
                if !already && !remote {
                    if !update::is_installed(&dir) {
                        stage("Downloading the server files from GitHub…");
                        let remote = update::remote_head()?;
                        update::install(&dir, &remote.sha, &l_sink)?;
                    }
                    stage("Preparing Python and .NET…");
                    let prepared = runtime::ensure(&dir, &l_sink)?;
                    let info = update::read_info(&dir).unwrap_or_default();
                    if cfg.translate_after_update && !info.translated {
                        stage("Translating to English…");
                        translate_step(&dir, true, &l_sink)?;
                    }
                    stage("Starting the server…");
                    let settings = server::Settings {
                        dir: dir.clone(),
                        game_dir: game_dir.clone(),
                        duration: cfg.server_duration,
                        extra_args: cfg.server_extra_args.clone(),
                        lan_tunnel: false,
                        options: options::to_args(&cfg.server_options, known.as_deref()),
                    };
                    let h = server::start(&prepared, &settings, s_sink)?;
                    App::emit(&tx, &ctx, Event::ServerUp(h.clone()));
                    let t = Instant::now();
                    loop {
                        if let Some(code) = h.exit_code() {
                            anyhow::bail!("the server stopped with exit code {code} — see the Logs page");
                        }
                        if server::ports().all_up() {
                            break;
                        }
                        if t.elapsed() > Duration::from_secs(120) {
                            anyhow::bail!("the server did not open its ports within 2 minutes — see the Logs page");
                        }
                        std::thread::sleep(Duration::from_millis(300));
                    }
                }
                if with_client {
                    stage("Starting the game…");
                    let script = match &proton {
                        Some(rt) => rt.script.clone(),
                        None if !roe_core::NEEDS_PROTON => PathBuf::new(),
                        None => anyhow::bail!("no Proton runtime"),
                    };
                    let prefix = if roe_core::NEEDS_PROTON { cfg.resolve_prefix(&game_dir) } else { PathBuf::new() };
                    let mut cs = client::Settings::new(game_dir.clone(), script, prefix);
                    cs.server_addr = cfg.connection.address();
                    cs.extra_args = cfg.client_extra_args.clone();
                    cs.extra_env = cfg.env_pairs();
                    cs.proton_log = cfg.proton_log;
                    let h = client::start(&cs, c_sink)?;
                    App::emit(&tx, &ctx, Event::ClientUp(h));
                }
                Ok(())
            })();
            App::emit(&tx, &ctx, Event::Stage(None));
            App::emit(&tx, &ctx, Event::FlowDone(res.map_err(|e| format!("{e:#}"))));
        });
    }

    // ---------- the launcher's own updates ----------
    pub fn check_launcher(&mut self) {
        if self.launcher_checking {
            return;
        }
        self.launcher_checking = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = selfupdate::latest().map_err(|e| format!("{e:#}"));
            App::emit(&tx, &ctx, Event::LauncherRelease(r));
        });
    }

    /// A newer launcher is published and not installed yet.
    pub fn launcher_update_available(&self) -> bool {
        self.launcher_installed.is_none() && matches!(&self.launcher_release, Some(Ok(r)) if r.is_newer_than(selfupdate::CURRENT))
    }

    pub fn update_launcher(&mut self) {
        let (Some(Ok(rel)), Some(exe)) = (self.launcher_release.clone(), self.exe.clone()) else { return };
        if self.launcher_updating {
            return;
        }
        self.launcher_updating = true;
        self.launcher_error = None;
        let (sink, tx, ctx) = (self.sink(Src::Launcher), self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = selfupdate::install(&rel, &exe, &sink).map(|()| rel.tag.clone()).map_err(|e| format!("{e:#}"));
            App::emit(&tx, &ctx, Event::LauncherInstalled(r));
        });
    }

    /// Starts the (new) launcher and closes this one. Not while a game or our server runs.
    pub fn restart_launcher(&mut self) {
        let Some(exe) = self.exe.clone() else { return };
        match std::process::Command::new(&exe).spawn() {
            Ok(_) => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => self.log(Src::Launcher, format!("Could not start the new launcher ({}): {e}", exe.display())),
        }
    }

    /// Deletes the oldest session logs (Server Settings > Session), only while no server runs.
    pub fn prune_session_logs(&mut self) {
        let keep = self.cfg.keep_runs as usize;
        if keep == 0 || self.server_running() || server::ports().proxy {
            return;
        }
        let dir = self.cfg.server_dir_or_default();
        let sink = self.sink(Src::Launcher);
        std::thread::spawn(move || match server::prune_runs(&dir, keep) {
            Ok((0, _)) => {}
            Ok((n, bytes)) => sink(format!("Deleted {n} old session log{} ({:.1} MB); keeping the newest {keep}.", if n == 1 { "" } else { "s" }, bytes as f64 / 1_048_576.0)),
            Err(e) => sink(format!("Could not clean up old session logs: {e:#}")),
        });
    }

    // ---------- server options ----------
    /// Reads what the installed windows_launch.py accepts, in the background (cached by the core).
    pub fn load_schema(&mut self) {
        if self.schema_loading {
            return;
        }
        let dir = self.cfg.server_dir_or_default();
        let python = runtime::server_python(&dir);
        if !dir.join("tools/windows_launch.py").is_file() || !python.is_file() {
            self.schema = Some(Err("the full list appears after the server's first start (it needs the server's Python)".into()));
            return;
        }
        self.schema_loading = true;
        let (sink, tx, ctx) = (self.sink(Src::Launcher), self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = options::read_schema(&dir, &python, &sink).map_err(|e| format!("{e:#}"));
            App::emit(&tx, &ctx, Event::Schema(r));
        });
    }

    /// "exit code 2" means windows_launch.py refused its arguments: show its own reason instead.
    fn explain_server_error(&self, e: String) -> String {
        if !e.contains("exit code 2") {
            return e;
        }
        let reason = self.logs.iter().rev().filter(|(s, _)| *s == Src::Server).find_map(|(_, t)| t.split_once(": error: ").map(|(_, r)| r.to_string()));
        match reason {
            Some(r) => format!("The server did not accept the match settings: {r}"),
            None => e,
        }
    }

    // ---------- Server Browser ----------
    /// Everything the browser shows: this computer first, then saved, then found entries.
    pub fn server_list(&self) -> Vec<ServerEntry> {
        servers::merge(&[&[servers::local_entry()], &self.cfg.saved_servers, &self.browser.found])
    }

    /// Asks the network sources again (LAN search, directory URL) in the background.
    pub fn refresh_servers(&mut self) {
        if self.browser.refreshing {
            return;
        }
        self.browser.refreshing = true;
        self.browser.refreshed = true;
        let url = self.cfg.server_directory_url.trim().to_string();
        let lan_search = self.cfg.lan_search;
        let instance = self.browser.instance.clone();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let (mut found, mut errors) = (Vec::new(), Vec::new());
            if lan_search {
                match lan::discover(Duration::from_millis(1200), Some(&instance)) {
                    Ok(v) => found.extend(v),
                    Err(e) => errors.push(format!("network search failed: {e:#}")),
                }
            }
            if !url.is_empty() {
                match (servers::JsonDirectory { url: url.clone() }).fetch() {
                    Ok(v) => found.extend(v),
                    Err(e) => errors.push(format!("{url}: {e:#}")),
                }
            }
            App::emit(&tx, &ctx, Event::Servers(found, errors));
        });
    }

    /// Joining = the normal Play flow with the connection set to that server.
    pub fn join_server(&mut self, e: &ServerEntry) {
        if servers::can_join(e).is_err() {
            return;
        }
        self.cfg.connection = Connection::Local;
        self.page = Page::Play;
        if self.play_blockers().is_empty() && !self.busy() && !self.client_running() {
            self.start_flow(true);
        }
    }

    pub fn toggle_favorite(&mut self, address: &str) {
        let favs = &mut self.cfg.favorite_servers;
        match favs.iter().position(|f| f == address) {
            Some(i) => {
                favs.remove(i);
            }
            None => favs.push(address.to_string()),
        }
    }

    /// Starts or stops answering LAN searches to match the settings.
    fn sync_lan_responder(&mut self) {
        let b = &mut self.browser;
        *b.lan_name.lock().unwrap() = self.cfg.lan_name.trim().to_string();
        if !self.cfg.lan_visible {
            b.responder = None;
            b.responder_error = None;
            return;
        }
        if b.responder.is_some() || b.responder_error.is_some() {
            return;
        }
        let (name, instance) = (b.lan_name.clone(), b.instance.clone());
        let info = Arc::new(move || {
            if !server::ports().all_up() {
                return None;
            }
            let name = name.lock().map(|n| n.clone()).unwrap_or_default();
            Some(lan::Announcement {
                instance: instance.clone(),
                name: if name.is_empty() { "Ring of Elysium local server".into() } else { name },
                port: server::PROXY_PORT,
                players: 0,
                max_players: 1,
                version: Some(format!("roe-launcher {}", env!("CARGO_PKG_VERSION"))),
                mode: Some("Training".into()),
                ..Default::default()
            })
        });
        let bind = std::net::SocketAddr::from(([0, 0, 0, 0], lan::DISCOVERY_PORT));
        match lan::Responder::start(bind, info) {
            Ok(r) => b.responder = Some(r),
            Err(e) => {
                let msg = format!("{e:#}");
                b.responder_error = Some(msg.clone());
                self.log(Src::Launcher, format!("Server Browser: {msg}"));
            }
        }
    }

    pub fn stop_server(&mut self) {
        if let Some(h) = &self.server {
            h.request_stop(Duration::from_secs(10));
        }
    }

    pub fn stop_game(&mut self) {
        if let Some(h) = &self.client {
            h.request_stop(Duration::from_secs(5));
        }
    }

    // ---------- per-frame housekeeping ----------
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.drain_events();

        // port status once a second
        if self.last_poll.elapsed() > Duration::from_secs(1) {
            self.ports = server::ports();
            self.last_poll = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(if self.cfg.theme.effect == Effect::Off { 500 } else { 33 }));

        // game exited -> optionally stop the server we started
        let running = self.client_running();
        if self.client_was_running && !running {
            let code = self.client.as_ref().and_then(|h| h.exit_code());
            self.log(Src::Launcher, format!("Game closed (exit code {}).", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())));
            if self.cfg.stop_server_on_game_exit && self.server_ours && self.server_running() {
                self.log(Src::Launcher, "Stopping the server (the game was closed).".into());
                self.stop_server();
            }
        }
        self.client_was_running = running;

        // No game yet (or Steam installing it): look every 5 s, so an install started here, in Steam or
        // before the launcher opened is noticed, and PLAY appears once Steam reports it complete
        // (the exe appears long before the download ends). Only two small files are read.
        let watch = (!self.game_ok() || self.install_wait.is_some()) && !pretend_no_game();
        if watch && self.last_install_check.elapsed() > Duration::from_secs(5) {
            self.last_install_check = Instant::now();
            let libs = steam::libraries(&steam::steam_roots());
            if !steam::install_finished(&libs) {
                if !self.steam_installing {
                    self.log(Src::Launcher, "Steam is installing Ring of Elysium; PLAY appears when it finishes.".into());
                }
                self.steam_installing = true;
                self.install_wait.get_or_insert_with(Instant::now);
            } else if steam::find_game(&libs).is_some() {
                let was_waiting = self.install_wait.is_some() || self.steam_installing;
                self.install_wait = None;
                self.steam_installing = false;
                self.detect();
                if was_waiting {
                    self.log(Src::Launcher, "Steam finished installing Ring of Elysium: ready to play.".into());
                }
            }
        }

        // hash check when the game folder changes
        if self.cfg.game_dir != self.build_for {
            self.build_for = self.cfg.game_dir.clone();
            self.build = None;
            if let Some(p) = self.cfg.game_dir.clone().filter(|p| game::is_game_dir(p)) {
                let (tx, c2) = (self.tx.clone(), self.ctx.clone());
                std::thread::spawn(move || {
                    let r = game::check_build(&p).map_err(|e| e.to_string());
                    App::emit(&tx, &c2, Event::Build(p, r));
                });
            }
        }

        self.ensure_background(ctx);
        self.sync_lan_responder();
        if self.page == Page::ServerBrowser && !self.browser.refreshed {
            self.refresh_servers();
        }

        // debounced autosave
        if self.cfg != self.last_seen {
            self.last_seen = self.cfg.clone();
            self.changed_at = Instant::now();
        }
        if self.last_seen != self.saved && self.changed_at.elapsed() > Duration::from_millis(400) {
            if let Err(e) = self.last_seen.save() {
                self.log(Src::Launcher, format!("Could not save settings: {e:#}"));
            }
            self.saved = self.last_seen.clone();
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.cfg.save();
        if self.server_ours {
            if let Some(h) = &self.server {
                h.request_stop(Duration::from_secs(5));
                let t = Instant::now();
                while h.is_running() && t.elapsed() < Duration::from_secs(6) {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

/// Applies the translation and records it in the install info.
fn translate_step(dir: &Path, enabled: bool, sink: &Sink) -> anyhow::Result<Option<translate::Report>> {
    if !enabled {
        return Ok(None);
    }
    let own = runtime::server_python(dir);
    let py = if own.is_file() { Some(own) } else { runtime::find_python() };
    let Some(py) = py else {
        sink("Skipping the translation: Python 3.11+ was not found.".into());
        return Ok(None);
    };
    sink("Translating comments and messages to English…".into());
    let rep = translate::apply(dir, &py)?;
    let mut info = update::read_info(dir).unwrap_or_default();
    info.translated = true;
    info.untranslated = Some(rep.untranslated);
    update::write_info(dir, &info)?;
    sink(format!("Translated {} files; {} lines of new upstream text are still Russian.", rep.files_changed, rep.untranslated));
    Ok(Some(rep))
}

fn open_log_file() -> Option<std::io::LineWriter<std::fs::File>> {
    let dir = roe_core::util::data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::File::create(dir.join("launcher.log")).ok().map(std::io::LineWriter::new)
}

/// `ROE_PRETEND_NO_GAME=1` (development): act as if the game is not installed, without touching the
/// settings, to test "Install via Steam" with the real Steam.
fn pretend_no_game() -> bool {
    std::env::var("ROE_PRETEND_NO_GAME").as_deref() == Ok("1")
}

/// Different for every launcher run (no randomness crate needed for this).
fn instance_id() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}-{:x}", std::process::id(), nanos)
}

fn page_headers(ctx: &egui::Context) -> Vec<(Page, egui::TextureHandle)> {
    let v = [
        (Page::ServerBrowser, assets::HEADER_BROWSER),
        (Page::Server, assets::HEADER_SERVER),
        (Page::Game, assets::HEADER_GAME),
        (Page::Updates, assets::HEADER_UPDATES),
        (Page::Appearance, assets::HEADER_APPEARANCE),
        (Page::Logs, assets::HEADER_LOGS),
    ];
    v.into_iter().map(|(page, (name, bytes))| (page, tex(ctx, name, bytes))).collect()
}

/// Texture from an embedded PNG (always valid: it ships inside the binary).
fn tex(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    ctx.load_texture(name, assets::decode(bytes, 4096).expect("embedded image"), egui::TextureOptions::LINEAR)
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let Some(path) = self.screenshot.clone() else { return };
        for ev in &raw_input.events {
            if let egui::Event::Screenshot { image, .. } = ev {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(img) = image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, bytes) {
                    let r = img.save(&path).map_err(|e| e.to_string());
                    self.log(Src::Launcher, format!("Screenshot {}: {r:?}", path.display()));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.screenshot.is_some() {
            self.frames += 1;
            if self.frames == 90 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint();
        }
        #[cfg(windows)]
        if self.wine && !self.uia_blocked {
            self.uia_blocked = true;
            if let Err(e) = wine_fixes::block_ui_automation(_frame) {
                self.log(Src::Launcher, format!("Could not apply the Wine fix: {e}"));
            }
        }
        self.tick(ctx);
        self.ui(ctx);
        if self.wine {
            // without an IME request eframe never calls set_ime_allowed / set_ime_cursor_area
            ctx.output_mut(|o| o.ime = None);
        }
        // debug builds only: reproduce cursor changes without a mouse (Wine testing)
        #[cfg(debug_assertions)]
        if std::env::var("ROE_DEBUG_CURSOR").as_deref() == Ok("text") {
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}

/// Wine's UI Automation (screen readers) crashes when AccessKit answers its `WM_GETOBJECT`
/// queries, and Wine then unwinds past winit, which panics. Under Wine only, the window answers
/// those queries itself with "no accessibility tree"; real Windows keeps screen-reader support.
#[cfg(windows)]
mod wine_fixes {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallWindowProcW, SetWindowLongPtrW, GWLP_WNDPROC, WM_GETOBJECT, WNDPROC};

    static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_GETOBJECT {
            return 0;
        }
        // SAFETY: PREVIOUS holds the procedure that was installed before ours.
        let previous: WNDPROC = std::mem::transmute(PREVIOUS.load(Ordering::SeqCst));
        CallWindowProcW(previous, hwnd, msg, wparam, lparam)
    }

    pub fn block_ui_automation(frame: &eframe::Frame) -> Result<(), String> {
        let handle = frame.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return Err("not a Win32 window".into()) };
        let hwnd = h.hwnd.get() as HWND;
        // SAFETY: standard window subclassing; our procedure forwards everything else unchanged.
        let old = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, window_proc as *const () as isize) };
        if old == 0 {
            return Err("SetWindowLongPtrW failed".into());
        }
        PREVIOUS.store(old, Ordering::SeqCst);
        Ok(())
    }
}
