//! The window: the game library, a page for one game, and status toasts.
//!
//! `App` holds all state; `view.rs` draws it. Work that finishes on other threads (artwork,
//! games and installers exiting) lands in `Inbox` and is picked up by `poll` on the UI thread.

pub mod i18n;
#[cfg(test)]
mod tests;
mod theme;
mod view;

use crate::applog::{AppLog, Host, VERSION};
use crate::art::{ArtStore, Endpoints, has_art};
use crate::background::Background;
use crate::config::{Config, GameSettings};
use crate::install::Plan;
use crate::launch::{LaunchSpec, OnExit, build_spec, default_prefix, first_non_empty, stop_game};
use crate::paths::Paths;
use crate::prefixes::Prefix;
use crate::scan::{Game, game_from_exe, scan_libraries};
use crate::steam::{self, Proton, Steam};
use i18n::{Lang, Strings, fill};
use rune::{AppSpec, FileDialogOptions, Image, ImageCache, Key, KeyEvent, TextEdit, Tx, Waker};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a status toast stays up, in seconds.
pub const TOAST_SECS: f64 = 4.0;
pub const WINDOW: (f32, f32) = (1080.0, 720.0);
/// Under 600 so Hyprland can tile the window narrow without Rune drawing it cropped.
pub const MIN_WINDOW: (f32, f32) = (540.0, 420.0);
/// Rune's double-click interval off macOS and Windows, in seconds.
pub const DOUBLE_CLICK: f64 = 0.5;

/// Starts a game, returning its log file. Tests start games with a fake one.
pub type Launcher = Box<dyn Fn(&LaunchSpec, OnExit) -> Result<PathBuf, String>>;

/// Stops the game in a folder, returning how many processes it found. Swapped out in tests.
pub type Stopper = Box<dyn Fn(&Path) -> usize>;

/// Deletes an uninstalled game's folder, on a thread of its own. Swapped out in tests.
pub type Deleter = Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>;

pub struct App {
    pub paths: Paths,
    /// asterofix's own log, for bug reports.
    pub log: Arc<AppLog>,
    /// The machine, for the log and diagnostics; set by `run`.
    pub host: Host,
    /// The game launched last, which diagnostics describe when no page is open.
    pub last_launched: Option<PathBuf>,
    pub steam: Option<Steam>,
    /// The Flatpak or Snap Steam found when there's no native one.
    pub unsupported_steam: Option<steam::Unsupported>,
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
    /// The game whose failed install's folder is being removed, off the UI thread. No other
    /// install starts until it's gone, so the removal can't reach a new one's files.
    pub removing: Option<String>,
    /// How much the install has written so far, once measured.
    pub install_size: Option<u64>,
    /// Cancel was pressed: the installer's exit is reported as cancelled.
    pub install_cancelled: bool,
    /// Stops the thread measuring the install (`measure_install`); its sizes carry
    /// `measure_id`, so one left over from an earlier install is dropped.
    measuring: Option<Arc<AtomicBool>>,
    measure_id: u64,
    /// How often the install is measured. Shorter in tests.
    pub measure_every: Duration,
    /// The Install popup, while it's open.
    pub install_form: Option<InstallForm>,
    /// The games found in library folders by the last scan, rather than added by hand. Only
    /// these can be uninstalled.
    pub from_library: HashSet<PathBuf>,
    /// The game the Uninstall popup asks about, while it's open.
    pub uninstall_confirm: Option<PathBuf>,
    /// Games whose folders are being deleted.
    pub uninstalling: HashSet<PathBuf>,
    pub form: Form,
    /// The prefixes there are, for the Prefix field's menu: read when a page or that menu
    /// opens, never per frame.
    pub prefixes: Vec<Prefix>,
    /// The open dropdown on the page.
    pub open_select: Option<&'static str>,
    /// How many times the reload button was pressed: its icon turns once for each.
    pub reload_turns: u32,
    /// The Steam app whose hero art is behind the last page opened. It stays after the page
    /// closes, so the hero fades out in place (`page_hero_fades_out_under_the_header`).
    pub hero_id: Option<u32>,
    pub toast: Option<Toast>,
    /// The UI's language: the one picked in the header's menu, or the system's.
    pub lang: Lang,
    /// Seconds since start, as of the last frame.
    pub now: f64,
    pub background: Background,

    /// Starts games in Steam's place when set: tests put a fake launcher here. Unset, games
    /// start through the Steam found last (`App::launch`), so finding Steam again never puts the
    /// real launcher back over a fake (`reload_finds_steam_and_proton_again`).
    pub launcher_override: Option<Launcher>,
    pub stopper: Stopper,
    pub deleter: Deleter,
    pub steam_running: fn() -> bool,
}

#[derive(Default)]
pub struct Images {
    pub cover: Option<Image>,
    pub hero: Option<Image>,
}

/// Where a game in the library comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Found in a library folder: Uninstall deletes its folder.
    Library,
    /// Its .exe was added by hand: Remove from library forgets it.
    Added,
    Unknown,
}

/// The page's text fields.
#[derive(Default)]
pub struct Form {
    pub name: TextEdit,
    pub fake_id: TextEdit,
    pub args: TextEdit,
    pub overrides: TextEdit,
    pub env: TextEdit,
    pub prefix: TextEdit,
}

/// What the Install popup has chosen.
#[derive(Default)]
pub struct InstallForm {
    pub game: Option<PathBuf>,
    pub fix: Option<PathBuf>,
    /// What installing them would do, or why it can't; None until one is chosen.
    pub plan: Option<Result<Plan, String>>,
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
    /// How removing a failed install's folder went (`App::remove_partial`).
    removed: Vec<Result<bool, String>>,
    /// What the install has written so far, by the measuring thread's id.
    sizes: Vec<(u64, u64)>,
    /// Uninstalled games' folders, their names, and how deleting them went.
    uninstalls: Vec<(PathBuf, String, Result<(), String>)>,
}

impl App {
    /// `system` is the language the UI starts in until one is picked (`Lang::detect`).
    pub fn new(paths: Paths, endpoints: Endpoints, system: Lang) -> App {
        let (cfg, cfg_warning) = Config::open(&paths);
        let lang = Lang::from_code(&cfg.language).unwrap_or(system);
        let log = Arc::new(AppLog::open(&paths));
        log.line(format!("asterofix {VERSION} started"));
        let mut app = App {
            log,
            host: Host::default(),
            last_launched: None,
            cfg,
            art: ArtStore::new(&paths, None, endpoints),
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
            removing: None,
            install_size: None,
            install_cancelled: false,
            measuring: None,
            measure_id: 0,
            measure_every: Duration::from_secs(1),
            install_form: None,
            from_library: HashSet::new(),
            uninstall_confirm: None,
            uninstalling: HashSet::new(),
            form: Form::default(),
            prefixes: Vec::new(),
            open_select: None,
            reload_turns: 0,
            hero_id: None,
            toast: None,
            lang,
            now: 0.0,
            background: Background::new(),
            launcher_override: None,
            stopper: Box::new(stop_game),
            deleter: Arc::new(crate::uninstall::delete),
            steam_running: steam::steam_running,
            paths,
            steam: None,
            unsupported_steam: None,
            protons: Vec::new(),
            default_proton: String::new(),
        };
        app.detect_steam(true);
        if app.steam.is_none() {
            app.set_status(app.no_steam());
        }
        if let Some(warning) = cfg_warning {
            app.set_status(warning);
        }
        app.rescan();
        app
    }

    /// Sets the machine's details, and logs them.
    pub fn set_host(&mut self, host: Host) {
        let appimage = if host.appimage { ", AppImage" } else { "" };
        self.log.line(format!("System: {} (kernel {}), {}{appimage}", host.os, host.kernel, host.session));
        self.host = host;
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
        let Inbox { art, exits, installs, removed, sizes, uninstalls } = std::mem::take(&mut *lock(&self.inbox));
        let mut changed = !art.is_empty()
            || !exits.is_empty()
            || !installs.is_empty()
            || !removed.is_empty()
            || !uninstalls.is_empty();
        for (id, size) in sizes {
            if self.measuring.is_some() && id == self.measure_id {
                changed |= self.install_size != Some(size);
                self.install_size = Some(size);
            }
        }
        for (root, name, result) in uninstalls {
            self.uninstall_done(&root, &name, result);
        }
        for result in installs {
            self.install_step_done(result);
        }
        for result in removed {
            self.partial_removed(result);
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
            let stopped = if self.stopping.contains(&root) { ", stopped from asterofix" } else { "" };
            match &result {
                Ok(()) => self.log.line(format!("{name} exited normally{stopped}")),
                Err(e) => self.log.line(format!("{name} exited with {e}{stopped}")),
            }
            if let Some(log) = self.logs.get(&root) {
                let _ = crate::launch::finish_log(log, &result);
            }
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
            Key::Escape if self.install_form.is_some() => self.close_install(),
            Key::Escape if self.uninstall_confirm.is_some() => self.uninstall_confirm = None,
            // Nothing behind a popup takes keys.
            _ if self.install_form.is_some() || self.uninstall_confirm.is_some() => return false,
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
        first_non_empty(&s.name, &self.detected_name(g)).to_owned()
    }

    /// The game's name without a rename: its Steam name, or its folder's.
    pub fn detected_name(&self, g: &Game) -> String {
        first_non_empty(&self.art.get(g.real_id).name, &g.name).to_owned()
    }

    /// The reload button: looks for Steam and its Proton builds again, rescans, and turns its
    /// icon once.
    pub fn reload(&mut self) {
        self.reload_turns = self.reload_turns.wrapping_add(1);
        self.detect_steam(false);
        if self.steam.is_none() {
            self.set_status(self.no_steam());
        }
        self.rescan();
    }

    /// Finds Steam, its Proton builds and its default tool, and logs them when they changed
    /// (or `always`). Steam installed, a Proton build added or Steam's default changed while
    /// asterofix is open show up without a restart.
    fn detect_steam(&mut self, always: bool) {
        let steam = steam::detect(&self.paths.home);
        let protons = steam.as_ref().map(Steam::protons).unwrap_or_default();
        let default_proton = steam.as_ref().map(|s| s.default_proton(&protons)).unwrap_or_default();
        let unsupported = if steam.is_none() { steam::unsupported(&self.paths.home) } else { None };
        let root = |s: &Option<Steam>| s.as_ref().map(|s| s.root.clone());
        if always || root(&steam) != root(&self.steam) || unsupported != self.unsupported_steam {
            match &steam {
                Some(steam) => self.log.line(format!("Steam: {}", steam.root.display())),
                None => self.log.line(format!("Steam: {}", steam::not_found(unsupported))),
            }
        }
        self.unsupported_steam = unsupported;
        if always || protons != self.protons || default_proton != self.default_proton {
            self.log.line(format!("Proton: {}", proton_summary(&protons, &default_proton)));
        }
        // Its library cache has artwork; what's loaded already stays.
        self.art.set_steam_root(steam.as_ref().map(|s| s.root.as_path()));
        (self.steam, self.protons, self.default_proton) = (steam, protons, default_proton);
    }

    /// Why there's no Steam, in the UI's language: none, or one asterofix can't use.
    fn no_steam(&self) -> String {
        match self.unsupported_steam {
            None => self.t().no_steam.to_owned(),
            Some(u) => fill(self.t().steam_unsupported, &[&u.name()]),
        }
    }

    /// Whether games can start: Steam is known, looked for again if it wasn't (or a test's
    /// launcher stands in for it).
    fn can_launch(&mut self) -> bool {
        if self.steam.is_none() {
            self.detect_steam(false);
        }
        self.steam.is_some() || self.launcher_override.is_some()
    }

    /// Starts `spec` through Steam, or the launcher standing in for it.
    fn launch(&self, spec: &LaunchSpec, on_exit: OnExit) -> Result<PathBuf, String> {
        match (&self.launcher_override, &self.steam) {
            (Some(launcher), _) => launcher(spec, on_exit),
            (None, Some(steam)) => steam.launch(&self.paths, spec, on_exit),
            (None, None) => Err(steam::no_steam_error(&self.paths.home)),
        }
    }

    pub fn rescan(&mut self) {
        let mut games = scan_libraries(&self.cfg.libraries);
        self.from_library = games.iter().map(|g| g.root.clone()).collect();
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
        let (libraries, manual) = (self.cfg.libraries.len(), self.cfg.manual_exes.len());
        self.log.line(format!("Library: {} games, from {libraries} folders and {manual} added .exe", self.games.len()));
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
        self.hero_id = self.current().map(|g| g.real_id);
    }

    pub fn close_page(&mut self) {
        self.selected = None;
        self.open_select = None;
        self.opening_until = 0.0;
    }

    /// Opens the menu `key`, or closes it if it's open. Opening lands whatever is still moving:
    /// a cover flying between a card and the page draws above the whole window, so it would
    /// cover the menu (`a_cover_flying_home_never_shows_over_a_menu`).
    pub fn toggle_select(&mut self, key: &'static str, tx: &mut Tx) {
        if self.open_select == Some(key) {
            self.open_select = None;
        } else {
            self.open_select = Some(key);
            if key == "prefix" {
                self.load_prefixes();
            }
            tx.settle();
        }
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
            name: TextEdit::new(s.name),
            fake_id: TextEdit::new(fake),
            args: TextEdit::new(s.args),
            overrides: TextEdit::new(s.overrides),
            env: TextEdit::new(s.env),
            prefix: TextEdit::new(s.prefix),
        };
        self.load_prefixes();
    }

    /// Reads which prefixes there are, while a page is open.
    fn load_prefixes(&mut self) {
        self.prefixes = if self.selected.is_some() { crate::prefixes::list(&self.paths) } else { Vec::new() };
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

    /// Saves what's typed into the page's fields. A new name moves the game's card to its
    /// place in the library's order.
    pub fn save_form(&mut self) {
        let form = [&self.form.fake_id, &self.form.args, &self.form.overrides, &self.form.env, &self.form.prefix]
            .map(|e| e.text.trim().to_owned());
        let [fake, args, overrides, env, prefix] = form;
        let name = self.form.name.text.trim().to_owned();
        let renamed = self.current().is_some_and(|g| self.cfg.settings(&g.root).name != name);
        self.edit(|s, _| {
            s.name = name;
            s.fake_app_id = fake.parse().unwrap_or(0);
            (s.args, s.overrides, s.env, s.prefix) = (args, overrides, env, prefix);
        });
        if renamed {
            self.sort_games();
        }
    }

    pub fn choose_proton(&mut self, name: &str) {
        let default = self.default_proton.clone();
        self.edit(|s, _| s.proton = if name == default { String::new() } else { name.to_owned() });
        self.open_select = None;
    }

    /// The prefix called `name` in the Prefix field's menu. The game's own is saved as no
    /// override, so it follows the game's folder as it always did.
    pub fn choose_prefix(&mut self, name: &str) {
        let path = crate::prefixes::prefixes_dir(&self.paths).join(name);
        let paths = self.paths.clone();
        self.edit(|s, g| {
            s.prefix = if path == default_prefix(&paths, &g.root) {
                String::new()
            } else {
                path.to_string_lossy().into_owned()
            };
        });
        let chosen = self.current().map(|g| self.cfg.settings(&g.root).prefix).unwrap_or_default();
        self.form.prefix = TextEdit::new(chosen);
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
        if self.uninstalling.contains(root) {
            return self.set_status(fill(t.uninstalling, &[&self.display_name(&g)]));
        }
        if !self.can_launch() {
            return self.set_status(self.no_steam());
        }
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
        let line = format!(
            "Launching {}: {} with {}, prefix {}",
            spec.name,
            spec.exe.display(),
            spec.proton.name,
            spec.prefix.display()
        );
        self.log.line(line);
        match self.launch(&spec, on_exit) {
            Ok(log) => {
                self.log.line(format!("{}'s log: {}", spec.name, log.display()));
                self.logs.insert(g.root.clone(), log);
                self.running.insert(g.root.clone());
                self.last_launched = Some(g.root.clone());
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

    /// The logo: copies what a bug report needs (`diagnostics`).
    pub fn copy_diagnostics(&mut self, tx: &mut Tx) {
        tx.copy(self.diagnostics());
        self.set_status(self.t().diagnostics_copied);
    }

    /// What a bug report needs, as Markdown to paste into one: asterofix's version, the
    /// system, what it found, the open (or last launched) game, and the ends of that game's
    /// log and of asterofix's own. In English, whatever the UI's language. The home folder
    /// shows as `~`; the logs keep secrets out already (`open_log`).
    pub fn diagnostics(&self) -> String {
        const LINES: usize = 40;
        let h = &self.host;
        let appimage = if h.appimage { ", AppImage" } else { "" };
        let steam =
            self.steam.as_ref().map_or(steam::not_found(self.unsupported_steam), |s| s.root.display().to_string());
        let mut out = format!("**asterofix {VERSION}**\n\n");
        out += &format!("- System: {} (kernel {}), {}{appimage}\n", h.os, h.kernel, h.session);
        out += &format!("- Steam: {steam}\n");
        out += &format!("- Proton: {}\n", proton_summary(&self.protons, &self.default_proton));
        out += &format!("- Library: {} games\n", self.games.len());
        out += &format!("- Language: {}\n", self.lang.code());
        let game = self.current().or_else(|| self.last_launched.as_deref().and_then(|root| self.game(root)));
        if let Some(g) = game {
            let name = self.display_name(g);
            let id = if has_art(g.real_id) { format!("Steam app {}", g.real_id) } else { "not on Steam".into() };
            out += &match self.spec_for(g) {
                Ok(spec) => format!(
                    "- Game: {name} ({id}), {} with {}, prefix {}\n",
                    spec.exe.display(),
                    spec.proton.name,
                    spec.prefix.display()
                ),
                Err(e) => format!("- Game: {name} ({id}), which can't launch: {e}\n"),
            };
            if let Some(tail) = self.log_for(g).and_then(|log| crate::applog::tail(&log, LINES)) {
                out += &format!("\n{name}'s log, last {LINES} lines:\n{}", fenced(&tail));
            }
        }
        if let Some(tail) = crate::applog::tail(self.log.path(), LINES) {
            out += &format!("\nasterofix's log, last {LINES} lines:\n{}", fenced(&tail));
        }
        match self.paths.home.to_str().filter(|h| h.len() > 1) {
            Some(home) => out.replace(&format!("{home}/"), "~/"),
            None => out,
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

    /// Opens the Install popup, where the game's installer and its online fix are chosen.
    pub fn open_install(&mut self) {
        match (&self.installing, &self.removing) {
            (Some((plan, _)), _) => self.set_status(fill(self.t().already_installing, &[&plan.name])),
            (_, Some(name)) => self.set_status(fill(self.t().still_removing, &[name])),
            (None, None) => {
                self.open_select = None;
                self.install_form = Some(InstallForm::default());
            }
        }
    }

    pub fn close_install(&mut self) {
        self.install_form = None;
    }

    /// Asks for the game's installer, in ~/Downloads at first.
    pub fn browse_installer(&mut self, tx: &mut Tx) {
        let form = self.install_form.as_ref();
        let dir = form.and_then(|f| f.game.as_deref()?.parent().map(Path::to_path_buf));
        let dir = dir.or_else(|| Some(self.paths.home.join("Downloads")).filter(|d| d.is_dir()));
        pick_exe(tx, self.t(), self.t().install_game_title, dir, |s, exe| s.choose_installer(exe));
    }

    /// Asks for the online fix, beside the game's installer's folder, where it's usually
    /// downloaded.
    pub fn browse_fix(&mut self, tx: &mut Tx) {
        let form = self.install_form.as_ref();
        let dir = form.and_then(|f| f.fix.as_ref().or(f.game.as_ref())?.parent().map(Path::to_path_buf));
        let dir = match form.and_then(|f| f.fix.as_ref()) {
            Some(_) => dir,
            None => dir.and_then(|d| d.parent().map(Path::to_path_buf)),
        };
        pick_exe(tx, self.t(), self.t().install_fix_title, dir, |s, exe| s.choose_fix(Some(exe)));
    }

    /// Takes `exe` as the game's installer, and the fix downloaded beside it unless one was
    /// chosen already.
    pub fn choose_installer(&mut self, exe: PathBuf) {
        let Some(form) = &mut self.install_form else { return };
        if form.fix.is_none() {
            form.fix = crate::install::find_fix(&exe);
        }
        form.game = Some(exe);
        self.plan_install();
    }

    pub fn choose_fix(&mut self, exe: Option<PathBuf>) {
        let Some(form) = &mut self.install_form else { return };
        form.fix = exe;
        self.plan_install();
    }

    /// Works out what the chosen installers would do, for the popup to show.
    fn plan_install(&mut self) {
        let Some(form) = &mut self.install_form else { return };
        let files: Vec<PathBuf> = form.game.iter().chain(&form.fix).cloned().collect();
        form.plan = (!files.is_empty()).then(|| crate::install::plan(&self.paths, &files));
    }

    /// The popup's Install: closes it and runs the installers.
    pub fn confirm_install(&mut self) {
        let Some(form) = self.install_form.take() else { return };
        self.start_install(form.game.into_iter().chain(form.fix).collect());
    }

    /// Installs the game from `files`, its installer and online fixes in any order, into
    /// ~/Games/asterofix.
    pub fn start_install(&mut self, files: Vec<PathBuf>) {
        let t = self.t();
        if self.installing.is_some() {
            return;
        }
        if let Some(name) = &self.removing {
            return self.set_status(fill(t.still_removing, &[name]));
        }
        if !self.can_launch() {
            return self.set_status(self.no_steam());
        }
        match crate::install::plan(&self.paths, &files) {
            Ok(plan) => {
                self.measure_install(plan.dest.clone());
                self.installing = Some((plan, 0));
                self.run_install_step();
            }
            Err(e) => self.set_status(fill(t.install_failed, &[&e])),
        }
    }

    /// Measures what the install has written so far, about once a second, on another thread
    /// until `end_install`. The installer's own total is inside its compressed data, so the
    /// growing size is what shows it's alive.
    fn measure_install(&mut self, dest: PathBuf) {
        let stop = Arc::new(AtomicBool::new(false));
        self.measure_id += 1;
        self.measuring = Some(stop.clone());
        let (id, every, inbox, wake) = (self.measure_id, self.measure_every, self.inbox.clone(), self.wake());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let size = crate::install::dir_size(&dest);
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                lock(&inbox).sizes.push((id, size));
                wake();
                std::thread::sleep(every);
            }
        });
    }

    /// The install is over, however it ended: nothing is measured or cancelled any more.
    fn end_install(&mut self) {
        self.installing = None;
        self.install_size = None;
        self.install_cancelled = false;
        if let Some(stop) = self.measuring.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// The progress toast's Cancel: stops the installer's processes. Its exit is then reported
    /// as cancelled, no later installer runs, and what the game's installer left is removed.
    pub fn cancel_install(&mut self) {
        let Some((plan, step)) = &self.installing else { return };
        if self.install_cancelled {
            return;
        }
        self.install_cancelled = true;
        let found = (self.stopper)(&plan.dest);
        let exe = crate::scan::file_name(&plan.steps[*step].exe);
        self.log.line(format!("Cancelling the install of {}: stopping {exe}, {found} processes", plan.name));
    }

    /// Starts the current installer of the game being installed.
    fn run_install_step(&mut self) {
        let t = self.t();
        let Some((plan, step)) = &self.installing else { return };
        let Some(proton) = self.protons.iter().find(|p| p.name == self.default_proton) else {
            self.end_install();
            return self.set_status(fill(t.install_failed, &[&t.no_proton]));
        };
        let spec = plan.spec(&self.paths, *step, proton);
        let (inbox, wake) = (self.inbox.clone(), self.wake());
        let on_exit: OnExit = Box::new(move |result| {
            lock(&inbox).installs.push(result);
            wake();
        });
        let exe = plan.steps[*step].exe.display();
        self.log.line(format!("Installing {} into {}: {exe} with {}", plan.name, plan.dest.display(), proton.name));
        match self.launch(&spec, on_exit) {
            // The progress toast shows from here until `end_install`.
            Ok(log) => self.log.line(format!("The installer's log: {}", log.display())),
            Err(e) => {
                self.end_install();
                self.set_status(fill(t.install_failed, &[&e]));
            }
        }
    }

    /// An installer ended: the next one starts, or the game is in the library.
    fn install_step_done(&mut self, result: Result<(), String>) {
        let t = self.t();
        let Some((plan, step)) = self.installing.take() else { return };
        let cancelled = self.install_cancelled;
        let exe = crate::scan::file_name(&plan.steps[step].exe);
        match &result {
            _ if cancelled => self.log.line(format!("{exe} was cancelled")),
            Ok(()) => self.log.line(format!("{exe} finished")),
            Err(e) => self.log.line(format!("{exe} exited with {e}")),
        }
        // Killed, so it "failed", but that's what Cancel asked for.
        let result = if cancelled { Err(String::new()) } else { result.map_err(|e| format!("{exe}: {e}")) };
        if let Err(e) = result.and_then(|()| plan.check(step)) {
            self.end_install();
            let message =
                if cancelled { fill(t.install_cancelled, &[&plan.name]) } else { fill(t.install_failed, &[&e]) };
            // What the game's installer left would stop the next try. A failed fix leaves the
            // game it was for alone.
            if !plan.steps[step].is_fix {
                self.remove_partial(plan);
            }
            return self.set_status(message);
        }
        if step + 1 < plan.steps.len() {
            self.installing = Some((plan, step + 1));
            return self.run_install_step();
        }
        self.end_install();
        if let Some(library) = plan.dest.parent() {
            self.add_library(library.to_path_buf());
        }
        self.close_search();
        self.open_page(&plan.dest);
        self.set_status(fill(t.installed, &[&plan.name]));
    }

    /// Removes the folder a failed game install left (`Plan::remove_partial`) on another
    /// thread: it waits for the installer's last processes, and a game can be gigabytes.
    fn remove_partial(&mut self, plan: Plan) {
        if !plan.fresh {
            return;
        }
        self.removing = Some(plan.name.clone());
        let (paths, inbox, wake) = (self.paths.clone(), self.inbox.clone(), self.wake());
        std::thread::spawn(move || {
            let result = plan.remove_partial(&paths, &crate::launch::game_running);
            lock(&inbox).removed.push(result.map_err(|e| format!("{}: {e}", plan.name)));
            wake();
        });
    }

    fn partial_removed(&mut self, result: Result<bool, String>) {
        let Some(name) = self.removing.take() else { return };
        match result {
            Ok(true) => self.log.line(format!("Removed what the install of {name} left")),
            Ok(false) => {}
            Err(e) => {
                self.log.line(format!("Couldn't remove what the install of {name} left: {e}"));
                self.set_status(fill(self.t().install_failed, &[&e]));
            }
        }
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

    /// Where the open game comes from, which says how it leaves the library: a game found in
    /// a library folder is uninstalled, one added by hand is only removed.
    pub fn origin(&self, g: &Game) -> Origin {
        if self.from_library.contains(&g.root) {
            Origin::Library
        } else if self.cfg.manual_exes.iter().any(|e| e.parent() == Some(g.root.as_path())) {
            Origin::Added
        } else {
            Origin::Unknown
        }
    }

    /// The page's Remove from library, for a game added by hand: forgets its .exe. No files
    /// are touched, and its settings stay, so adding it again brings them back.
    pub fn remove_from_library(&mut self) {
        let Some(g) = self.current().cloned() else { return };
        let (t, name) = (self.t(), self.display_name(&g));
        if self.running.contains(&g.root) {
            return self.set_status(fill(t.stop_first, &[&name]));
        }
        // Every .exe picked from its folder, or the next one would bring the card back.
        self.cfg.manual_exes.retain(|e| e.parent() != Some(g.root.as_path()));
        self.save();
        self.log.line(format!("Removed {name} ({}) from the library", g.root.display()));
        self.close_page();
        self.rescan();
        self.set_status(fill(t.removed, &[&name]));
    }

    /// Why the game in `root` can't be uninstalled now, if it can't.
    fn uninstall_refused(&self, root: &Path) -> Option<String> {
        let t = self.t();
        let g = self.game(root)?;
        let name = self.display_name(g);
        if self.running.contains(root) {
            return Some(fill(t.stop_first, &[&name]));
        }
        if self.installing.as_ref().is_some_and(|(plan, _)| plan.dest.starts_with(root) || root.starts_with(&plan.dest))
        {
            return Some(fill(t.being_installed, &[&name]));
        }
        if self.uninstalling.contains(root) {
            return Some(fill(t.uninstalling, &[&name]));
        }
        if self.origin(g) != Origin::Library {
            return Some(fill(t.uninstall_failed, &[&name, &"it was added by hand, not found in a library folder"]));
        }
        let check = crate::uninstall::check(&self.paths, &self.cfg.libraries, root, &self.prefix_of(root));
        check.err().map(|e| fill(t.uninstall_failed, &[&name, &e]))
    }

    /// The Wine prefix the game in `root` plays in: the one picked for it, or its own.
    fn prefix_of(&self, root: &Path) -> PathBuf {
        match self.cfg.settings(root).prefix.as_str() {
            "" => crate::launch::default_prefix(&self.paths, root),
            p => PathBuf::from(p),
        }
    }

    /// The page's Uninstall: asks first, in a popup naming the folder.
    pub fn ask_uninstall(&mut self) {
        let Some(root) = self.selected.clone() else { return };
        match self.uninstall_refused(&root) {
            Some(why) => self.set_status(why),
            None => {
                self.open_select = None;
                self.uninstall_confirm = Some(root);
            }
        }
    }

    /// The popup's Uninstall: deletes the game's folder on a thread, checking again first
    /// that it's still a game directly in a library. The page closes once it's gone.
    pub fn confirm_uninstall(&mut self) {
        let Some(root) = self.uninstall_confirm.take() else { return };
        if let Some(why) = self.uninstall_refused(&root) {
            return self.set_status(why);
        }
        let Some(g) = self.game(&root) else { return };
        let name = self.display_name(g);
        let (paths, libraries, prefix) = (self.paths.clone(), self.cfg.libraries.clone(), self.prefix_of(&root));
        let (inbox, wake, delete) = (self.inbox.clone(), self.wake(), self.deleter.clone());
        self.log.line(format!("Uninstalling {name}: deleting {}", root.display()));
        self.uninstalling.insert(root.clone());
        self.set_progress(fill(self.t().uninstalling, &[&name]));
        let at = root.clone();
        std::thread::spawn(move || {
            let result = crate::uninstall::check(&paths, &libraries, &at, &prefix)
                .and_then(|()| delete(&at).map_err(|e| e.to_string()));
            lock(&inbox).uninstalls.push((at, name, result));
            wake();
        });
    }

    /// A game's folder is deleted, or deleting it failed partway: the library is scanned again
    /// either way.
    fn uninstall_done(&mut self, root: &Path, name: &str, result: Result<(), String>) {
        let t = self.t();
        self.uninstalling.remove(root);
        match &result {
            Ok(()) => self.log.line(format!("Uninstalled {name}: {} is deleted", root.display())),
            Err(e) => self.log.line(format!("Uninstalling {name} failed: {e}")),
        }
        if result.is_ok() {
            // .exe files picked from inside it are gone with it.
            let before = self.cfg.manual_exes.len();
            self.cfg.manual_exes.retain(|e| !e.starts_with(root));
            if self.cfg.manual_exes.len() != before {
                self.save();
            }
            if self.selected.as_deref() == Some(root) {
                self.close_page();
            }
        }
        self.rescan();
        match result {
            Ok(()) => self.set_status(fill(t.uninstalled, &[&name])),
            Err(e) => self.set_status(fill(t.uninstall_failed, &[&name, &e])),
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

    /// Shows a message in a toast that hides itself after a while. While a game installs, it
    /// sits above the install's progress, which stays.
    pub fn set_status(&mut self, text: impl Into<String>) {
        self.show_toast(text.into(), self.now + TOAST_SECS);
    }

    /// Shows a message that stays up until the next one replaces it.
    pub fn set_progress(&mut self, text: impl Into<String>) {
        self.show_toast(text.into(), f64::INFINITY);
    }

    fn show_toast(&mut self, text: String, until: f64) {
        // Every message the user saw, errors among them, in the language they saw it in.
        self.log.line(format!("Shown: {text}"));
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

/// A log in a Markdown code block. Four backticks, so a log line with three can't end it.
fn fenced(text: &str) -> String {
    format!("````\n{text}\n````\n")
}

/// The Proton builds found, the default marked: "Proton 9.0 (default), GE-Proton10-34".
fn proton_summary(protons: &[Proton], default: &str) -> String {
    if protons.is_empty() {
        return "none found".into();
    }
    let names =
        protons.iter().map(|p| if p.name == default { format!("{} (default)", p.name) } else { p.name.clone() });
    names.collect::<Vec<_>>().join(", ")
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
            "asterofix",
            WINDOW,
            || {
                let mut app =
                    App::new(Paths::from_env(), Endpoints::default(), Lang::detect(|k| std::env::var(k).ok()));
                app.set_host(Host::detect(Path::new("/"), |k| std::env::var(k).ok()));
                crate::applog::log_panics(app.log.clone());
                app
            },
            view::view,
        )
    });
}
