//! The window: the game library, the side panel for one game, and status toasts.
//!
//! `App` holds all state; `view.rs` draws it. Work that finishes on other threads (artwork,
//! games exiting) lands in `Inbox` and is picked up by `poll` on the UI thread.

#[cfg(test)]
mod tests;
mod theme;
mod view;

use crate::art::{ArtStore, Endpoints, has_art};
use crate::background::Background;
use crate::config::{Config, GameSettings};
use crate::launch::{LaunchSpec, OnExit, build_spec, first_non_empty};
use crate::paths::Paths;
use crate::scan::{Game, game_from_exe, scan_libraries};
use crate::steam::{self, Proton, Steam};
use rune::{AppSpec, FileDialogOptions, Image, ImageCache, Key, KeyEvent, TextEdit, Tx, Waker};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a status toast stays up, in seconds.
pub const TOAST_SECS: f64 = 4.0;
pub const WINDOW: (f32, f32) = (1080.0, 720.0);
/// Under 600 so Hyprland can tile the window narrow without Rune drawing it cropped.
pub const MIN_WINDOW: (f32, f32) = (540.0, 420.0);

/// Starts a game, returning its log file. Swapped out in tests.
pub type Launcher = Box<dyn Fn(&LaunchSpec, OnExit) -> Result<PathBuf, String>>;

pub struct App {
    pub paths: Paths,
    pub steam: Option<Steam>,
    pub protons: Vec<Proton>,
    pub default_proton: String,
    pub cfg: Config,
    pub art: ArtStore,
    /// Rune images of each app's cover and hero, made as artwork arrives.
    pub images: HashMap<u32, Images>,
    image_cache: Option<ImageCache>,
    /// Set by `setup`; shared so threads started before it can still wake the window.
    waker: Arc<Mutex<Option<Waker>>>,
    inbox: Arc<Mutex<Inbox>>,
    /// The latest artwork fetch, joined by headless runs so snapshots show the art.
    art_job: Option<std::thread::JoinHandle<()>>,

    /// Sorted by display name.
    pub games: Vec<Game>,
    pub search: TextEdit,
    /// The floating search bar shows; it opens when you start typing.
    pub search_open: bool,
    /// The game whose side panel is open.
    pub selected: Option<PathBuf>,
    pub running: HashSet<PathBuf>,
    pub logs: HashMap<PathBuf, PathBuf>,
    pub form: Form,
    /// The open dropdown in the panel.
    pub open_select: Option<&'static str>,
    pub toast: Option<Toast>,
    /// Seconds since start, as of the last frame.
    pub now: f64,
    pub background: Background,

    pub launcher: Option<Launcher>,
    pub steam_running: fn() -> bool,
}

#[derive(Default)]
pub struct Images {
    pub cover: Option<Image>,
    pub hero: Option<Image>,
}

/// The side panel's text fields.
#[derive(Default)]
pub struct Form {
    pub fake_id: TextEdit,
    pub args: TextEdit,
    pub overrides: TextEdit,
    pub env: TextEdit,
    pub prefix: TextEdit,
}

pub struct Toast {
    pub text: String,
    /// Bumped for every message, so each one gets its own entrance.
    pub seq: u64,
    pub until: f64,
}

/// What other threads hand the UI thread.
#[derive(Default)]
struct Inbox {
    art: Vec<u32>,
    exits: Vec<(PathBuf, String, Result<(), String>)>,
}

impl App {
    pub fn new(paths: Paths, endpoints: Endpoints) -> App {
        let steam = steam::detect(&paths.home);
        let protons = steam.as_ref().map(Steam::protons).unwrap_or_default();
        let default_proton = steam.as_ref().map(|s| s.default_proton(&protons)).unwrap_or_default();
        let launcher: Option<Launcher> = steam.clone().map(|steam| {
            let paths = paths.clone();
            Box::new(move |spec: &LaunchSpec, on_exit: OnExit| steam.launch(&paths, spec, on_exit)) as Launcher
        });
        let (cfg, cfg_warning) = Config::open(&paths);
        let mut app = App {
            cfg,
            art: ArtStore::new(&paths, steam.as_ref().map(|s| s.root.as_path()), endpoints),
            images: HashMap::new(),
            image_cache: None,
            waker: Arc::default(),
            inbox: Arc::default(),
            art_job: None,
            games: Vec::new(),
            search: TextEdit::default(),
            search_open: false,
            selected: None,
            running: HashSet::new(),
            logs: HashMap::new(),
            form: Form::default(),
            open_select: None,
            toast: None,
            now: 0.0,
            background: Background::new(),
            launcher,
            steam_running: steam::steam_running,
            paths,
            steam,
            protons,
            default_proton,
        };
        if app.steam.is_none() {
            app.set_status("Steam installation not found. Install Steam and log in once.");
        }
        if let Some(warning) = cfg_warning {
            app.set_status(warning);
        }
        app.rescan();
        app
    }

    /// Runs once the window exists, with the waker other threads use to reach it.
    pub fn setup(&mut self, waker: Waker) {
        self.image_cache = Some(ImageCache::new(waker.clone()));
        *lock(&self.waker) = Some(waker);
        if rune::tooling::headless_run().is_some()
            && let Some(job) = self.art_job.take()
        {
            let _ = job.join();
        }
        let ids: Vec<u32> = self.games.iter().map(|g| g.real_id).filter(|&id| has_art(id)).collect();
        for id in ids {
            self.show_art(id);
        }
        self.sort_games();
    }

    /// Picks up what other threads finished; true when anything visible changed.
    pub fn poll(&mut self, now: f64, _tx: &mut Tx) -> bool {
        self.now = now;
        let Inbox { art, exits } = std::mem::take(&mut *lock(&self.inbox));
        let mut changed = !art.is_empty() || !exits.is_empty();
        for id in art {
            self.show_art(id);
        }
        if changed {
            // A Steam name may change the order.
            self.sort_games();
        }
        for (root, name, result) in exits {
            self.running.remove(&root);
            match result {
                Ok(()) => self.set_status(format!("{name} exited")),
                Err(e) => self.set_status(format!("{name} exited: {e}")),
            }
        }
        // Clicking away from an empty search puts it away.
        if self.search_open && self.search.text.is_empty() && !self.search.focused {
            self.close_search();
            changed = true;
        }
        if self.toast.as_ref().is_some_and(|t| now >= t.until) {
            self.toast = None;
            changed = true;
        }
        changed
    }

    pub fn on_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let plain = !(ev.mods.cmd || ev.mods.ctrl || ev.mods.alt || ev.mods.logo);
        match &ev.key {
            Key::Escape if self.search_open => self.close_search(),
            Key::Escape if self.open_select.is_some() => self.open_select = None,
            Key::Escape if self.selected.is_some() => self.close_panel(),
            Key::Command('f') => {
                self.search_open = true;
                self.search.focused = true;
                tx.select_all_text("search");
            }
            // Typing anywhere that isn't a text field starts a search with what was typed.
            Key::Text(t) if plain && t.chars().any(|c| !c.is_whitespace() && !c.is_control()) => {
                if !self.search_open {
                    self.search = TextEdit::new("");
                    self.search_open = true;
                }
                self.search.text.push_str(t);
                let end = self.search.text.len();
                (self.search.anchor, self.search.focus) = (end, end);
                self.search.focused = true;
            }
            _ => return false,
        }
        true
    }

    /// Hides the search bar and shows every game again.
    pub fn close_search(&mut self) {
        self.search_open = false;
        self.search = TextEdit::default();
    }

    fn wake(&self) -> impl Fn() + Send + Sync + 'static {
        let waker = self.waker.clone();
        move || {
            if let Some(w) = lock(&waker).clone() {
                w()
            }
        }
    }

    pub fn display_name(&self, g: &Game) -> String {
        let s = self.cfg.settings(&g.root);
        let art = self.art.get(g.real_id);
        first_non_empty(&s.name, first_non_empty(&art.name, &g.name)).to_owned()
    }

    pub fn rescan(&mut self) {
        let mut games = scan_libraries(&self.cfg.libraries);
        for exe in &self.cfg.manual_exes {
            if exe.is_file() {
                let g = game_from_exe(exe);
                if !games.iter().any(|x| x.root == g.root) {
                    games.push(g);
                }
            }
        }
        self.games = games;
        self.sort_games();
        if self.selected.as_ref().is_some_and(|root| self.game(root).is_none()) {
            self.selected = None;
        }
        self.load_form();

        let ids: Vec<u32> = self.games.iter().map(|g| g.real_id).collect();
        let inbox = self.inbox.clone();
        let wake = self.wake();
        self.art_job = self.art.load(&ids, move |id| {
            lock(&inbox).art.push(id);
            wake();
        });
        // Art fetched by an earlier scan is already in the store.
        for id in ids {
            if has_art(id) && !self.images.contains_key(&id) {
                self.show_art(id);
            }
        }
    }

    /// Turns a game's downloaded artwork into images the window can draw.
    fn show_art(&mut self, id: u32) {
        let (art, Some(cache)) = (self.art.get(id), &self.image_cache) else { return };
        let image = |img: &Option<Arc<image::RgbaImage>>| {
            img.as_ref().map(|i| cache.rgba(i.width(), i.height(), Arc::<[u8]>::from(i.as_raw().as_slice())))
        };
        let entry = self.images.entry(id).or_default();
        if entry.cover.is_none() {
            entry.cover = image(&art.cover);
        }
        if entry.hero.is_none() {
            entry.hero = image(&art.hero);
        }
        if rune::tooling::headless_run().is_some() {
            // Snapshots render on the next frame; mips are made on a worker.
            for img in [&entry.cover, &entry.hero].into_iter().flatten() {
                let _ = img.wait();
            }
        }
    }

    fn sort_games(&mut self) {
        let mut keyed: Vec<(String, Game)> =
            std::mem::take(&mut self.games).into_iter().map(|g| (self.display_name(&g).to_lowercase(), g)).collect();
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        self.games = keyed.into_iter().map(|(_, g)| g).collect();
    }

    /// The games matching the search, in library order.
    pub fn visible(&self) -> Vec<&Game> {
        let q = self.search.text.trim().to_lowercase();
        self.games.iter().filter(|g| q.is_empty() || self.display_name(g).to_lowercase().contains(&q)).collect()
    }

    pub fn game(&self, root: &Path) -> Option<&Game> {
        self.games.iter().find(|g| g.root == root)
    }

    pub fn current(&self) -> Option<&Game> {
        self.selected.as_deref().and_then(|r| self.game(r))
    }

    pub fn spec_for(&self, g: &Game) -> Result<LaunchSpec, String> {
        build_spec(&self.paths, g, &self.cfg.settings(&g.root), &self.protons, &self.default_proton)
    }

    /// The defaults detected for the open game, shown as the fields' placeholders.
    pub fn detected(&self) -> Option<LaunchSpec> {
        let g = self.current()?;
        let s = GameSettings { exe: self.cfg.settings(&g.root).exe, ..GameSettings::default() };
        build_spec(&self.paths, g, &s, &self.protons, &self.default_proton).ok()
    }

    /// A click on a card: opens its panel, or closes it if it's the one open.
    pub fn toggle_panel(&mut self, root: &Path) {
        if self.selected.as_deref() == Some(root) { self.close_panel() } else { self.open_panel(root) }
    }

    pub fn open_panel(&mut self, root: &Path) {
        if self.selected.as_deref() != Some(root) {
            self.selected = Some(root.to_path_buf());
            self.open_select = None;
            self.load_form();
        }
    }

    pub fn close_panel(&mut self) {
        self.selected = None;
        self.open_select = None;
    }

    /// Fills the panel's fields from the open game's saved settings.
    fn load_form(&mut self) {
        let s = self.current().map(|g| self.cfg.settings(&g.root)).unwrap_or_default();
        let fake = if s.fake_app_id == 0 { String::new() } else { s.fake_app_id.to_string() };
        self.form = Form {
            fake_id: TextEdit::new(fake),
            args: TextEdit::new(s.args),
            overrides: TextEdit::new(s.overrides),
            env: TextEdit::new(s.env),
            prefix: TextEdit::new(s.prefix),
        };
    }

    /// Changes the open game's settings and saves them.
    pub fn edit(&mut self, f: impl FnOnce(&mut GameSettings, &Game)) {
        let Some(g) = self.current().cloned() else { return };
        let mut s = self.cfg.settings(&g.root);
        f(&mut s, &g);
        if s.is_empty() {
            self.cfg.games.remove(&g.root);
        } else {
            self.cfg.games.insert(g.root.clone(), s);
        }
        self.save();
    }

    /// Saves what's typed into the panel's fields.
    pub fn save_form(&mut self) {
        let form = [&self.form.fake_id, &self.form.args, &self.form.overrides, &self.form.env, &self.form.prefix]
            .map(|e| e.text.trim().to_owned());
        let [fake, args, overrides, env, prefix] = form;
        self.edit(|s, _| {
            s.fake_app_id = fake.parse().unwrap_or(0);
            (s.args, s.overrides, s.env, s.prefix) = (args, overrides, env, prefix);
        });
    }

    pub fn choose_proton(&mut self, name: &str) {
        let default = self.default_proton.clone();
        self.edit(|s, _| s.proton = if name == default { String::new() } else { name.to_owned() });
        self.open_select = None;
    }

    /// `exe` as listed in the dropdown: relative to the game's folder when inside it.
    pub fn choose_exe(&mut self, exe: &str) {
        if let Some(root) = self.current().map(|g| g.root.clone()) {
            self.use_exe(&root.join(exe));
        }
        self.open_select = None;
    }

    /// Makes the open game start `exe`; the detected one is saved as no override.
    fn use_exe(&mut self, exe: &Path) {
        self.edit(|s, g| {
            s.exe = if g.exes.first().map(PathBuf::as_path) == Some(exe) {
                String::new()
            } else {
                exe.to_string_lossy().into_owned()
            };
        });
    }

    fn save(&mut self) {
        if let Err(e) = self.cfg.save(&self.paths) {
            self.set_status(format!("Could not save settings: {e}"));
        }
    }

    /// Launches the game at `root`, unless it's running already.
    pub fn play(&mut self, root: &Path) {
        let Some(g) = self.game(root).cloned() else { return };
        if self.running.contains(root) {
            return self.set_status(format!("{} is already running", self.display_name(&g)));
        }
        let Some(launcher) = &self.launcher else {
            return self.set_status("Steam installation not found. Install Steam and log in once.");
        };
        let spec = match self.spec_for(&g) {
            Ok(spec) => spec,
            Err(e) => return self.set_status(e),
        };
        if !(self.steam_running)() {
            let msg = match steam::start_steam() {
                Ok(()) => "Steam isn't running, starting it. Press Play again once you're logged in.".to_owned(),
                Err(e) => format!("Could not start Steam: {e}"),
            };
            return self.set_status(msg);
        }
        let (inbox, wake, name, game_root) = (self.inbox.clone(), self.wake(), spec.name.clone(), g.root.clone());
        let on_exit: OnExit = Box::new(move |result| {
            lock(&inbox).exits.push((game_root, name, result));
            wake();
        });
        match launcher(&spec, on_exit) {
            Ok(log) => {
                self.logs.insert(g.root.clone(), log);
                self.running.insert(g.root.clone());
                self.set_status(format!("Launched {} with {}", spec.name, spec.proton.name));
            }
            Err(e) => self.set_status(format!("Launch failed: {e}")),
        }
    }

    pub fn copy_command(&mut self, tx: &mut Tx) {
        let (Some(steam), Some(g)) = (&self.steam, self.current()) else { return };
        match self.spec_for(g).and_then(|spec| steam.shell_command(&spec)) {
            Ok(line) => {
                tx.copy(line);
                self.set_status("Launch command copied");
            }
            Err(e) => self.set_status(e),
        }
    }

    pub fn add_folder(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions { title: "Add a folder of games".into(), ..Default::default() };
        tx.open_folder(options, |s: &mut App, result, _| match result {
            Ok(Some(dir)) => s.add_library(dir),
            Ok(None) => {}
            Err(e) => s.set_status(format!("Could not open a folder picker: {e}")),
        });
    }

    pub fn add_library(&mut self, dir: PathBuf) {
        if !self.cfg.libraries.contains(&dir) {
            self.cfg.libraries.push(dir);
            self.save();
        }
        self.rescan();
    }

    pub fn add_game(&mut self, tx: &mut Tx) {
        pick_exe(tx, "Add a game", None, |s, exe| s.add_manual_exe(exe));
    }

    pub fn add_manual_exe(&mut self, exe: PathBuf) {
        // An exe inside a game already in the library is that game's: it becomes the one it
        // starts, rather than a second card with its own settings and prefix.
        let owner = self.games.iter().filter(|g| exe.starts_with(&g.root)).max_by_key(|g| g.root.components().count());
        if let Some(root) = owner.map(|g| g.root.clone()) {
            self.close_search();
            self.open_panel(&root);
            self.use_exe(&exe);
            return;
        }
        if !self.cfg.manual_exes.contains(&exe) {
            self.cfg.manual_exes.push(exe.clone());
            self.save();
        }
        self.close_search();
        self.rescan();
        if let Some(root) = self.games.iter().find(|g| g.exes.first() == Some(&exe)).map(|g| g.root.clone()) {
            self.open_panel(&root);
        }
    }

    pub fn browse_exe(&mut self, tx: &mut Tx) {
        let dir = self.current().map(|g| g.root.clone());
        pick_exe(tx, "Choose the game's executable", dir, |s, exe| s.use_exe(&exe));
    }

    pub fn open_path(&mut self, path: &Path) {
        match std::process::Command::new("xdg-open").arg(path).spawn() {
            Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
            Err(e) => self.set_status(format!("xdg-open: {e}")),
        }
    }

    pub fn open_log(&mut self) {
        match self.current().and_then(|g| self.log_for(g)) {
            Some(log) => self.open_path(&log),
            None => self.set_status("No log yet. Launch the game first."),
        }
    }

    /// The game's latest log: from this run, or left by an earlier one.
    pub fn log_for(&self, g: &Game) -> Option<PathBuf> {
        if let Some(log) = self.logs.get(&g.root) {
            return Some(log.clone());
        }
        let log = crate::launch::log_path(&self.paths, &self.spec_for(g).ok()?.name);
        log.is_file().then_some(log)
    }

    /// Shows a message in a toast that hides itself after a while.
    pub fn set_status(&mut self, text: impl Into<String>) {
        let seq = self.toast.as_ref().map_or(0, |t| t.seq + 1);
        self.toast = Some(Toast { text: text.into(), seq, until: self.now + TOAST_SECS });
    }
}

fn pick_exe(tx: &mut Tx, title: &str, dir: Option<PathBuf>, done: impl FnOnce(&mut App, PathBuf) + 'static) {
    let options = FileDialogOptions { title: title.into(), directory: dir, ..Default::default() }
        .filter("Windows programs", &["exe", "EXE"]);
    tx.open_file(options, move |s: &mut App, result, _| match result {
        Ok(Some(exe)) => done(s, exe),
        Ok(None) => {}
        Err(e) => s.set_status(format!("Could not open a file picker: {e}")),
    });
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Toasts schedule their own end; `poll` runs then.
fn poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    if let Some(t) = &s.toast
        && t.until > now
    {
        tx.wake_after(Duration::from_secs_f64(t.until - now));
    }
    s.poll(now, tx)
}

pub fn run() {
    rune::run(AppSpec::<App> {
        min_size: Some(MIN_WINDOW),
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        setup: Some(|s, waker| {
            s.setup(waker);
            s.background.animate();
        }),
        poll: Some(poll),
        on_key: Some(App::on_key),
        scenes: view::SCENES,
        ..AppSpec::new("asterisk-ofix", WINDOW, || App::new(Paths::from_env(), Endpoints::default()), view::view)
    });
}
