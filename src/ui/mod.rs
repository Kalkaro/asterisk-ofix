//! The window: the game library, a page for one game, and status toasts.
//!
//! `App` holds all state; `view.rs` draws it. Work that finishes on other threads (artwork,
//! games and installers exiting) lands in `Inbox` and is picked up by `poll` on the UI thread.

pub mod i18n;
#[cfg(test)]
mod tests;
mod theme;
mod view;

use crate::art::{ArtStore, Endpoints, has_art};
use crate::background::Background;
use crate::config::{Config, GameSettings};
use crate::install::Plan;
use crate::launch::{LaunchSpec, OnExit, build_spec, first_non_empty, stop_game};
use crate::paths::Paths;
use crate::scan::{Game, game_from_exe, scan_libraries};
use crate::steam::{self, Proton, Steam};
use i18n::{Lang, Strings, fill};
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
/// Rune's double-click interval off macOS and Windows, in seconds.
pub const DOUBLE_CLICK: f64 = 0.5;

/// Starts a game, returning its log file. Swapped out in tests.
pub type Launcher = Box<dyn Fn(&LaunchSpec, OnExit) -> Result<PathBuf, String>>;

/// Stops the game in a folder, returning how many processes it found. Swapped out in tests.
pub type Stopper = Box<dyn Fn(&Path) -> usize>;

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
    /// The game whose page is open, in place of the grid.
    pub selected: Option<PathBuf>,
    /// A page opened by a click on a card leaves the grid under the pointer until then, so the
    /// second click of a double click still reaches the card.
    pub opening_until: f64,
    /// The page's launch options are expanded.
    pub options_open: bool,
    pub running: HashSet<PathBuf>,
    /// Running games the user pressed Stop on.
    pub stopping: HashSet<PathBuf>,
    pub logs: HashMap<PathBuf, PathBuf>,
    /// The game being installed, and which of its installers is running.
    pub installing: Option<(Plan, usize)>,
    pub form: Form,
    /// The open dropdown on the page.
    pub open_select: Option<&'static str>,
    pub toast: Option<Toast>,
    /// The UI's language: the one picked in the header's menu, or the system's.
    pub lang: Lang,
    /// Each language's flag, for the language menu; made once the window exists.
    pub flags: HashMap<Lang, Image>,
    /// Seconds since start, as of the last frame.
    pub now: f64,
    pub background: Background,

    pub launcher: Option<Launcher>,
    pub stopper: Stopper,
    pub steam_running: fn() -> bool,
}

#[derive(Default)]
pub struct Images {
    pub cover: Option<Image>,
    pub hero: Option<Image>,
}

/// The page's text fields.
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
    /// How each installer ended.
    installs: Vec<Result<(), String>>,
}

impl App {
    /// `system` is the language the UI starts in until one is picked (`Lang::detect`).
    pub fn new(paths: Paths, endpoints: Endpoints, system: Lang) -> App {
        let steam = steam::detect(&paths.home);
        let protons = steam.as_ref().map(Steam::protons).unwrap_or_default();
        let default_proton = steam.as_ref().map(|s| s.default_proton(&protons)).unwrap_or_default();
        let launcher: Option<Launcher> = steam.clone().map(|steam| {
            let paths = paths.clone();
            Box::new(move |spec: &LaunchSpec, on_exit: OnExit| steam.launch(&paths, spec, on_exit)) as Launcher
        });
        let (cfg, cfg_warning) = Config::open(&paths);
        let lang = Lang::from_code(&cfg.language).unwrap_or(system);
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
            opening_until: 0.0,
            options_open: false,
            running: HashSet::new(),
            stopping: HashSet::new(),
            logs: HashMap::new(),
            installing: None,
            form: Form::default(),
            open_select: None,
            toast: None,
            lang,
            flags: HashMap::new(),
            now: 0.0,
            background: Background::new(),
            launcher,
            stopper: Box::new(stop_game),
            steam_running: steam::steam_running,
            paths,
            steam,
            protons,
            default_proton,
        };
        if app.steam.is_none() {
            app.set_status(app.t().no_steam);
        }
        if let Some(warning) = cfg_warning {
            app.set_status(warning);
        }
        app.rescan();
        app
    }

    /// Runs once the window exists, with the waker other threads use to reach it.
    pub fn setup(&mut self, waker: Waker) {
        let cache = ImageCache::new(waker.clone());
        // Drawn at 20 points; three times that stays sharp on HiDPI screens.
        self.flags =
            Lang::ALL.into_iter().map(|l| (l, cache.svg(theme::flag_svg(l).into_bytes(), 20.0, 3.0))).collect();
        if rune::tooling::headless_run().is_some() {
            for flag in self.flags.values() {
                let _ = flag.wait();
            }
        }
        self.image_cache = Some(cache);
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
        let Inbox { art, exits, installs } = std::mem::take(&mut *lock(&self.inbox));
        let mut changed = !art.is_empty() || !exits.is_empty() || !installs.is_empty();
        for result in installs {
            self.install_step_done(result);
        }
        for id in art {
            self.show_art(id);
        }
        if changed {
            // A Steam name may change the order.
            self.sort_games();
        }
        for (root, name, result) in exits {
            self.running.remove(&root);
            let t = self.t();
            match result {
                // Killed, so it "failed", but that's what Stop asked for.
                _ if self.stopping.remove(&root) => self.set_status(fill(t.stopped, &[&name])),
                Ok(()) => self.set_status(fill(t.exited, &[&name])),
                Err(e) => self.set_status(fill(t.exited_with, &[&name, &e])),
            }
        }
        // Clicking away from an empty search puts it away.
        if self.search_open && self.search.text.is_empty() && !self.search.focused {
            self.close_search();
            changed = true;
        }
        if self.opening_until > 0.0 && now >= self.opening_until {
            self.opening_until = 0.0;
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
            Key::Escape if self.selected.is_some() => self.close_page(),
            Key::Command('f') => {
                // Search goes through the library, so it leaves the page.
                self.close_page();
                self.search_open = true;
                self.search.focused = true;
                tx.select_all_text("search");
            }
            // Typing anywhere that isn't a text field starts a search with what was typed.
            Key::Text(t) if plain && t.chars().any(|c| !c.is_whitespace() && !c.is_control()) => {
                self.close_page();
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

    /// The UI's strings, in its language.
    pub fn t(&self) -> &'static Strings {
        self.lang.strings()
    }

    /// Switches the UI to the language named `name` (in itself, as the menu lists it) and
    /// remembers it.
    pub fn choose_language(&mut self, name: &str) {
        self.open_select = None;
        let Some(lang) = Lang::from_name(name) else { return };
        self.lang = lang;
        self.cfg.language = lang.code().to_owned();
        self.save();
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

    /// A click on a card opens its page. The grid keeps taking presses for a moment, so a
    /// second click makes a double click on the card and plays it.
    pub fn open_from_card(&mut self, root: &Path) {
        if self.selected.is_none() {
            self.opening_until = self.now + DOUBLE_CLICK;
        }
        self.open_page(root);
    }

    /// A page just opened by a click on a card, while a second click could still make it a
    /// double click.
    pub fn page_opening(&self) -> bool {
        self.selected.is_some() && self.now < self.opening_until
    }

    pub fn open_page(&mut self, root: &Path) {
        if self.selected.as_deref() != Some(root) {
            self.selected = Some(root.to_path_buf());
            self.open_select = None;
            self.load_form();
        }
    }

    pub fn close_page(&mut self) {
        self.selected = None;
        self.open_select = None;
        self.opening_until = 0.0;
    }

    pub fn toggle_options(&mut self) {
        self.options_open = !self.options_open;
        self.open_select = None;
    }

    /// Fills the page's fields from the open game's saved settings.
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

    /// Saves what's typed into the page's fields.
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
            self.set_status(fill(self.t().cant_save, &[&e]));
        }
    }

    /// Launches the game at `root`, unless it's running already.
    pub fn play(&mut self, root: &Path) {
        let Some(g) = self.game(root).cloned() else { return };
        let t = self.t();
        if self.running.contains(root) {
            return self.set_status(fill(t.already_running, &[&self.display_name(&g)]));
        }
        let Some(launcher) = &self.launcher else {
            return self.set_status(t.no_steam);
        };
        let spec = match self.spec_for(&g) {
            Ok(spec) => spec,
            Err(e) => return self.set_status(e),
        };
        if !(self.steam_running)() {
            let msg = match steam::start_steam() {
                Ok(()) => t.starting_steam.to_owned(),
                Err(e) => fill(t.cant_start_steam, &[&e]),
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
                self.set_status(fill(t.launched, &[&spec.name, &spec.proton.name]));
            }
            Err(e) => self.set_status(fill(t.launch_failed, &[&e])),
        }
    }

    /// Stops the running game at `root`. The badge stays until its processes are gone.
    pub fn stop(&mut self, root: &Path) {
        let Some(g) = self.game(root).cloned() else { return };
        if !self.running.contains(root) {
            return;
        }
        let name = self.display_name(&g);
        if (self.stopper)(root) == 0 {
            return self.set_status(fill(self.t().already_closing, &[&name]));
        }
        self.stopping.insert(root.to_owned());
        self.set_status(fill(self.t().stopping, &[&name]));
    }

    pub fn copy_command(&mut self, tx: &mut Tx) {
        let (Some(steam), Some(g)) = (&self.steam, self.current()) else { return };
        match self.spec_for(g).and_then(|spec| steam.shell_command(&spec)) {
            Ok(line) => {
                tx.copy(line);
                self.set_status(self.t().command_copied);
            }
            Err(e) => self.set_status(e),
        }
    }

    pub fn add_folder(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions { title: self.t().add_folder_title.into(), ..Default::default() };
        tx.open_folder(options, |s: &mut App, result, _| match result {
            Ok(Some(dir)) => s.add_library(dir),
            Ok(None) => {}
            Err(e) => s.set_status(fill(s.t().folder_picker_failed, &[&e])),
        });
    }

    pub fn add_library(&mut self, dir: PathBuf) {
        if self.cfg.add_library(dir) {
            self.save();
        }
        self.rescan();
    }

    /// Asks for a game's installer, then for its online fix, and installs them silently,
    /// the game first.
    pub fn install(&mut self, tx: &mut Tx) {
        if let Some((plan, _)) = &self.installing {
            return self.set_status(fill(self.t().already_installing, &[&plan.name]));
        }
        let t = self.t();
        let options = FileDialogOptions { title: t.install_game_title.into(), ..Default::default() }
            .filter(t.windows_programs, &["exe", "EXE"]);
        tx.open_file(options, |s: &mut App, result, tx| match result {
            Ok(Some(setup)) => {
                // Fixes are usually downloaded beside the repack's folder.
                let dir = setup.parent().and_then(Path::parent).map(Path::to_path_buf);
                let t = s.t();
                let options =
                    FileDialogOptions { title: t.install_fix_title.into(), directory: dir, ..Default::default() }
                        .filter(t.windows_programs, &["exe", "EXE"]);
                tx.open_file(options, move |s: &mut App, result, _| match result {
                    Ok(fix) => s.start_install(std::iter::once(setup).chain(fix).collect()),
                    Err(e) => s.set_status(fill(s.t().file_picker_failed, &[&e])),
                });
            }
            Ok(None) => {}
            Err(e) => s.set_status(fill(s.t().file_picker_failed, &[&e])),
        });
    }

    /// Installs the game from `files`, its installer and online fixes in any order, into
    /// ~/Games/asterisk.
    pub fn start_install(&mut self, files: Vec<PathBuf>) {
        let t = self.t();
        if self.installing.is_some() {
            return;
        }
        if self.launcher.is_none() {
            return self.set_status(t.no_steam);
        }
        match crate::install::plan(&self.paths, &files) {
            Ok(plan) => {
                self.installing = Some((plan, 0));
                self.run_install_step();
            }
            Err(e) => self.set_status(fill(t.install_failed, &[&e])),
        }
    }

    /// Starts the current installer of the game being installed.
    fn run_install_step(&mut self) {
        let t = self.t();
        let (Some((plan, step)), Some(launcher)) = (&self.installing, &self.launcher) else { return };
        let Some(proton) = self.protons.iter().find(|p| p.name == self.default_proton) else {
            self.installing = None;
            return self.set_status(fill(t.install_failed, &[&t.no_proton]));
        };
        let spec = plan.spec(&self.paths, *step, proton);
        let (inbox, wake) = (self.inbox.clone(), self.wake());
        let on_exit: OnExit = Box::new(move |result| {
            lock(&inbox).installs.push(result);
            wake();
        });
        let message = if plan.steps[*step].is_fix { t.installing_fix } else { t.installing };
        let message = fill(message, &[&plan.name]);
        match launcher(&spec, on_exit) {
            Ok(_) => self.set_progress(message),
            Err(e) => {
                self.installing = None;
                self.set_status(fill(t.install_failed, &[&e]));
            }
        }
    }

    /// An installer ended: the next one starts, or the game is in the library.
    fn install_step_done(&mut self, result: Result<(), String>) {
        let t = self.t();
        let Some((plan, step)) = self.installing.take() else { return };
        let exe = crate::scan::file_name(&plan.steps[step].exe);
        if let Err(e) = result.map_err(|e| format!("{exe}: {e}")).and_then(|()| plan.check(step)) {
            return self.set_status(fill(t.install_failed, &[&e]));
        }
        if step + 1 < plan.steps.len() {
            self.installing = Some((plan, step + 1));
            return self.run_install_step();
        }
        if let Some(library) = plan.dest.parent() {
            self.add_library(library.to_path_buf());
        }
        self.close_search();
        self.open_page(&plan.dest);
        self.set_status(fill(t.installed, &[&plan.name]));
    }

    pub fn add_game(&mut self, tx: &mut Tx) {
        pick_exe(tx, self.t(), self.t().add_game_title, None, |s, exe| s.add_manual_exe(exe));
    }

    pub fn add_manual_exe(&mut self, exe: PathBuf) {
        // An exe inside a game already in the library is that game's: it becomes the one it
        // starts, rather than a second card with its own settings and prefix.
        let owner = self.games.iter().filter(|g| exe.starts_with(&g.root)).max_by_key(|g| g.root.components().count());
        if let Some(root) = owner.map(|g| g.root.clone()) {
            self.close_search();
            self.open_page(&root);
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
            self.open_page(&root);
        }
    }

    pub fn browse_exe(&mut self, tx: &mut Tx) {
        let dir = self.current().map(|g| g.root.clone());
        pick_exe(tx, self.t(), self.t().choose_exe_title, dir, |s, exe| s.use_exe(&exe));
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
            None => self.set_status(self.t().no_log),
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
        self.show_toast(text.into(), self.now + TOAST_SECS);
    }

    /// Shows a message that stays up until the next one replaces it.
    pub fn set_progress(&mut self, text: impl Into<String>) {
        self.show_toast(text.into(), f64::INFINITY);
    }

    fn show_toast(&mut self, text: String, until: f64) {
        let seq = self.toast.as_ref().map_or(0, |t| t.seq + 1);
        self.toast = Some(Toast { text, seq, until });
    }
}

fn pick_exe(
    tx: &mut Tx,
    t: &Strings,
    title: &str,
    dir: Option<PathBuf>,
    done: impl FnOnce(&mut App, PathBuf) + 'static,
) {
    let options = FileDialogOptions { title: title.into(), directory: dir, ..Default::default() }
        .filter(t.windows_programs, &["exe", "EXE"]);
    tx.open_file(options, move |s: &mut App, result, _| match result {
        Ok(Some(exe)) => done(s, exe),
        Ok(None) => {}
        Err(e) => s.set_status(fill(s.t().file_picker_failed, &[&e])),
    });
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Toasts and a page's opening moment schedule their own end; `poll` runs then.
fn poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let ends = [s.toast.as_ref().map_or(0.0, |t| t.until), s.opening_until];
    // A progress toast never ends by itself.
    if let Some(end) = ends.into_iter().filter(|&end| end > now && end.is_finite()).reduce(f64::min) {
        tx.wake_after(Duration::from_secs_f64(end - now));
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
        ..AppSpec::new(
            "asterisk-ofix",
            WINDOW,
            || App::new(Paths::from_env(), Endpoints::default(), Lang::detect(|k| std::env::var(k).ok())),
            view::view,
        )
    });
}
