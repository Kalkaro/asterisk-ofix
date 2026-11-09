//! The real view, driven through Rune's headless runtime: pointer, keys, resizes and the
//! virtual clock. Nothing here touches the real machine: each test gets its own temp HOME,
//! a fake Steam store, and a fake launcher instead of Proton.

use super::i18n::{self, Lang};
use super::view::{self, CARD_W, COVER_W, COVER_W_NARROW, GUTTER, card_key};
use super::{App, DOUBLE_CLICK, MIN_WINDOW, TOAST_SECS};
use crate::art::fake;
use crate::config::Config;
use crate::launch::{LaunchSpec, OnExit};
use crate::paths::Paths;
use crate::testutil::{TempDir, add_game, inno_installer, tempdir};
use rune::{ElementInfo, Env, Key, KeyEvent, Mods, Runtime};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const SIZE: (f32, f32) = (1080.0, 720.0);

struct Ui {
    rt: Runtime<App>,
    home: TempDir,
}

impl Ui {
    /// A window on a temp HOME whose ~/Games holds `games` (folder name, Steam app id).
    fn new(games: &[(&str, u32)]) -> Ui {
        Ui::with_store(games, fake::start(&[]))
    }

    fn with_store(games: &[(&str, u32)], store: fake::FakeSteam) -> Ui {
        let home = tempdir();
        let lib = home.path().join("Games");
        std::fs::create_dir_all(&lib).unwrap();
        for (folder, id) in games {
            add_game(&lib, folder, *id);
        }
        let mut app = App::new(Paths::under(home.path()), store.endpoints, Lang::En);
        app.setup(std::sync::Arc::new(|| {}));
        let mut rt = Runtime::new(app, view::view, SIZE, Env::new(true, 1.0));
        rt.poll = Some(super::poll);
        rt.on_key = Some(App::on_key);
        let mut ui = Ui { rt, home };
        ui.settle();
        ui
    }

    fn game(&self, folder: &str) -> PathBuf {
        self.home.path().join("Games").join(folder)
    }

    fn paths(&self) -> Paths {
        Paths::under(self.home.path())
    }

    /// Runs frames until springs and fades have landed.
    fn settle(&mut self) {
        self.rt.frame(0.0);
        for _ in 0..90 {
            self.rt.frame(1.0 / 60.0);
        }
    }

    fn elements(&self) -> Vec<ElementInfo> {
        self.rt.elements()
    }

    fn find(&self, key: &str) -> Option<ElementInfo> {
        self.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting)
    }

    /// The drawn rect of `key`, or of a key path like "card-x/play" (each key a keyed
    /// ancestor of the next).
    fn rect(&self, key: &str) -> [f32; 4] {
        self.presented(key).unwrap_or_else(|| panic!("no {key:?}")).0
    }

    fn presented(&self, path: &str) -> Option<([f32; 4], f32)> {
        self.lookup(path).map(|e| (e.rect, e.opacity))
    }

    /// The element at a key path like "card-x/play".
    fn lookup(&self, path: &str) -> Option<ElementInfo> {
        let keys: Vec<&str> = path.split('/').collect();
        let all = self.elements();
        let found = all.iter().position(|e| {
            if e.key.as_deref() != keys.last().copied() || e.exiting {
                return false;
            }
            // Walk up, matching the leading keys in order.
            let mut want = keys.iter().rev().skip(1).peekable();
            let mut at = e.parent;
            while let (Some(i), Some(k)) = (at, want.peek()) {
                if all[i].key.as_deref() == Some(**k) {
                    want.next();
                }
                at = all[i].parent;
            }
            want.peek().is_none()
        })?;
        all.into_iter().nth(found)
    }

    /// The opacity of `key` while it animates away, if it's on its way out.
    fn leaving(&self, key: &str) -> Option<f32> {
        self.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && e.exiting).map(|e| e.opacity)
    }

    /// What the text at `path` (a key, or a key path) shows.
    fn text(&self, path: &str) -> String {
        self.lookup(path).and_then(|e| e.text).unwrap_or_default()
    }

    fn click(&mut self, key: &str) {
        let [x, y, w, h] = self.rect(key);
        // A card's middle is its play button, so cards are clicked near their corner.
        let (x, y) = if key.starts_with("card-") && !key.contains('/') {
            (x + 16.0, y + 16.0)
        } else {
            (x + w / 2.0, y + h / 2.0)
        };
        self.rt.pointer_move(x, y);
        self.rt.pointer_down(x, y);
        self.rt.pointer_up(x, y);
        self.rt.frame(1.0 / 60.0);
    }

    /// A click as a fresh single click, after any earlier one stopped counting.
    fn single_click(&mut self, key: &str) {
        for _ in 0..60 {
            self.rt.frame(1.0 / 60.0);
        }
        self.click(key);
    }

    fn double_click(&mut self, key: &str) {
        self.single_click(key);
        self.click(key);
    }

    fn key(&mut self, key: Key) {
        self.rt.key(KeyEvent { key, mods: Mods::default() });
        self.rt.frame(1.0 / 60.0);
    }

    fn type_text(&mut self, s: &str) {
        self.key(Key::Text(s.into()))
    }

    fn card_names(&self) -> Vec<String> {
        self.elements()
            .into_iter()
            .filter(|e| e.key.as_deref() == Some("name") && !e.exiting)
            .filter_map(|e| e.text)
            .collect()
    }

    fn card(&self, folder: &str) -> String {
        card_key(&self.game(folder))
    }

    fn toast(&self) -> String {
        self.text("toast")
    }

    fn resize(&mut self, w: f32, h: f32) {
        self.rt.set_viewport((w, h), 1.0);
        self.settle();
    }

    fn cards(&self) -> Vec<[f32; 4]> {
        self.elements()
            .into_iter()
            .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("card-")) && !e.exiting)
            .map(|e| e.rect)
            .collect()
    }
}

/// Swaps in a launcher that records what it was asked to start, and a stopper that records
/// what it was asked to stop.
#[derive(Clone, Default)]
struct FakeLaunch(Rc<RefCell<(Vec<LaunchSpec>, Option<OnExit>)>>, Rc<RefCell<Vec<PathBuf>>>);

impl FakeLaunch {
    fn install(ui: &mut Ui) -> FakeLaunch {
        let fake = FakeLaunch::default();
        let record = fake.clone();
        ui.rt.state.steam_running = || true;
        ui.rt.state.launcher = Some(Box::new(move |spec: &LaunchSpec, on_exit: OnExit| {
            let mut r = record.0.borrow_mut();
            r.0.push(spec.clone());
            r.1 = Some(on_exit);
            Ok(PathBuf::from("/nonexistent/fake.log"))
        }));
        let stops = fake.1.clone();
        ui.rt.state.stopper = Box::new(move |root: &Path| {
            stops.borrow_mut().push(root.to_owned());
            1
        });
        fake
    }

    fn stops(&self) -> Vec<PathBuf> {
        self.1.borrow().clone()
    }

    /// The game ends as a signal ends it.
    fn killed(&self) {
        self.0.borrow_mut().1.take().expect("a running game")(Err("signal: 15 (SIGTERM)".into()))
    }

    fn count(&self) -> usize {
        self.0.borrow().0.len()
    }

    fn exit(&self) {
        self.0.borrow_mut().1.take().expect("a running game")(Ok(()))
    }
}

#[test]
fn library_shows_sorted_cards() {
    let ui = Ui::new(&[("Charlie", 0), ("alpha", 0), ("Bravo", 0)]);
    assert_eq!(ui.card_names(), ["alpha", "Bravo", "Charlie"]);
    assert_eq!(ui.text("count"), "3");
    assert!(ui.find("page").is_none() && ui.find("empty").is_none(), "only the library shows");
}

#[test]
fn empty_library_offers_to_add_games() {
    let ui = Ui::new(&[]);
    assert_eq!(ui.text("empty-heading"), "No games yet");
    assert!(ui.find("empty-actions").is_some());
}

/// The empty library says which games it finds, by both fixes' names, and the longer text
/// still fits the narrowest window in every language (it doesn't wrap).
#[test]
fn empty_library_names_both_fixes_and_fits() {
    let mut ui = Ui::new(&[]);
    ui.resize(MIN_WINDOW.0, 600.0);
    for lang in Lang::ALL {
        ui.rt.update(move |s, _| s.lang = lang);
        ui.settle();
        let body = ui.text("empty-body");
        assert!(body.contains("Steam Fix / Online Fix") && !body.contains("OnlineFix"), "{lang:?}: {body:?}");
        let [x, _, w, _] = ui.rect("empty-body");
        assert!(x >= 16.0 && x + w <= MIN_WINDOW.0 - 16.0, "{lang:?}: the text runs from {x} to {}", x + w);
    }
}

#[test]
fn search_is_hidden_until_you_type() {
    let mut ui = Ui::new(&[("Hidden Road", 0), ("Hatch", 0), ("Pong", 0)]);
    assert!(ui.find("search-bar").is_none(), "no search until you type");

    // Typing with nothing focused opens the bar with what was typed, and keeps typing in it.
    ui.type_text("H");
    ui.type_text("a");
    ui.settle();
    assert!(ui.find("search-bar").is_some());
    assert_eq!(ui.rt.state.search.text, "Ha");
    assert!(ui.rt.state.search.focused, "the bar takes the keyboard");
    assert_eq!(ui.card_names(), ["Hatch"]);
    assert_eq!(ui.text("count"), "1 of 3");
    assert_eq!(ui.text("search-count"), "1 of 3");

    ui.type_text("zzz");
    ui.settle();
    assert!(ui.card_names().is_empty());
    assert_eq!(ui.text("empty-heading"), "No matches");
    assert!(ui.find("empty-actions").is_none(), "no add buttons for an empty search");

    ui.key(Key::Escape);
    ui.settle();
    assert!(ui.find("search-bar").is_none(), "Escape puts the search away");
    assert_eq!(ui.card_names().len(), 3, "and brings every card back");
    assert!(!ui.rt.state.search_open && ui.rt.state.search.text.is_empty());
}

#[test]
fn deleting_the_search_puts_it_away() {
    let mut ui = Ui::new(&[("Hidden Road", 0), ("Hatch", 0)]);
    ui.type_text("x");
    ui.settle();
    assert!(ui.find("search-bar").is_some());
    ui.key(Key::Backspace);
    ui.settle();
    assert!(ui.find("search-bar").is_none(), "an emptied search hides");
    assert_eq!(ui.card_names().len(), 2);
}

/// The bar floats over the bottom of the window, centred, and slides in.
#[test]
fn search_bar_floats_at_the_bottom() {
    let mut ui = Ui::new(&[("Hatch", 0)]);
    ui.type_text("h");
    ui.rt.frame(1.0 / 60.0);
    let (_, fading) = ui.presented("search-bar").unwrap();
    assert!(fading < 0.9, "it animates in (opacity {fading} on its second frame)");
    ui.settle();
    let [x, y, w, h] = ui.rect("search-bar");
    assert_eq!(ui.presented("search-bar").unwrap().1, 1.0);
    assert!((y + h - (SIZE.1 - 28.0)).abs() < 0.5, "28 pt above the bottom edge, at {}", y + h);
    assert!(((x + w / 2.0) - SIZE.0 / 2.0).abs() <= 0.5, "centred: {x} + {w}");

    // A toast while it's open sits above it.
    ui.rt.update(|s, _| s.set_status("Launch command copied"));
    ui.settle();
    let [_, ty, _, th] = ui.rect("toast");
    assert!(ty + th <= y, "the toast overlaps the search bar");
}

#[test]
fn typing_in_a_field_doesnt_open_search() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let root = ui.game("Pong");
    ui.rt.update(move |s, _| {
        s.open_page(&root);
        s.options_open = true;
    });
    ui.settle();
    ui.click("args");
    ui.type_text("-w");
    assert!(!ui.rt.state.search_open);
    assert_eq!(ui.rt.state.form.args.text, "-w");
}

#[test]
fn ctrl_f_opens_search() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    ui.key(Key::Command('f'));
    ui.settle();
    assert!(ui.rt.state.search_open && ui.rt.state.search.focused);
    assert!(ui.find("search-bar").is_some());
}

#[test]
fn double_click_plays() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    let card = ui.card("Pong");
    ui.double_click(&card);
    assert_eq!(fl.count(), 1);
    assert_eq!(fl.0.borrow().0[0].exe, ui.game("Pong").join("Pong.exe"));
    assert_eq!(ui.rt.state.selected, Some(ui.game("Pong")), "the first click opened the page");
    ui.settle();
    assert!(ui.find("running").is_some(), "the running badge shows");

    ui.key(Key::Escape);
    ui.settle();
    ui.double_click(&card);
    assert_eq!(fl.count(), 1, "a running game isn't started again");
    assert!(ui.toast().contains("already running"), "toast: {:?}", ui.toast());

    fl.exit();
    ui.settle();
    assert!(ui.find("running").is_none(), "the badge goes once the game exits");
    assert_eq!(ui.toast(), "Pong exited");
}

/// Folders that differ only in punctuation got the same card key (both slugged to
/// card-..._A_B), so one card answered for both.
#[test]
fn cards_of_similar_folders_are_told_apart() {
    let mut ui = Ui::new(&[("A B", 0), ("A_B", 0)]);
    let (a, b) = (ui.card("A B"), ui.card("A_B"));
    assert_ne!(a, b);
    assert!(!a.contains('/') && !b.contains('/'), "keys can't hold /");
    ui.single_click(&b);
    ui.settle();
    assert_eq!(ui.rt.state.selected, Some(ui.game("A_B")));
    ui.key(Key::Escape);
    ui.single_click(&a);
    ui.settle();
    assert_eq!(ui.rt.state.selected, Some(ui.game("A B")));
}

#[test]
fn slow_clicks_dont_play() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    let card = ui.card("Pong");
    ui.single_click(&card);
    ui.single_click(&card);
    assert_eq!(fl.count(), 0, "two clicks a second apart are not a double click");
}

#[test]
fn double_click_without_steam_says_so() {
    let mut ui = Ui::new(&[("Pong", 0)]); // the temp HOME has no Steam
    let card = ui.card("Pong");
    ui.double_click(&card);
    assert!(ui.toast().contains("Steam installation not found"), "toast: {:?}", ui.toast());
}

#[test]
fn single_click_opens_the_game_page() {
    let mut ui = Ui::new(&[("Pong", 0), ("Hatch", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    let (a, b) = (ui.card("Pong"), ui.card("Hatch"));

    ui.single_click(&a);
    ui.settle();
    assert!(ui.find("page").is_some(), "a click opens the game's page");
    assert_eq!(fl.count(), 0, "a single click doesn't launch");
    assert_eq!(ui.text("title"), "Pong");
    assert!(ui.text("meta").contains("Pong.exe"), "meta: {}", ui.text("meta"));

    ui.key(Key::Escape);
    ui.settle();
    assert!(ui.find("page").is_none() && ui.rt.state.selected.is_none(), "Escape goes back to the library");

    ui.single_click(&b);
    ui.settle();
    assert_eq!(ui.text("title"), "Hatch");
    ui.click("page-play");
    assert_eq!(fl.count(), 1, "the page's Play button launches");
    ui.click("back");
    ui.settle();
    assert!(ui.find("page").is_none(), "the Back button goes back to the library");
}

/// The page takes the grid's place: the grid stays mounted underneath, hidden and inert, so
/// it comes back as it was, and the page sits under the header, inside the window.
#[test]
fn game_page_replaces_the_grid() {
    let names: Vec<String> = (0..24).map(|i| format!("Game {i:02}")).collect();
    let games: Vec<(&str, u32)> = names.iter().map(|n| (n.as_str(), 0)).collect();
    let mut ui = Ui::new(&games);
    // Scroll the grid down a little.
    let first = ui.card("Game 00");
    let top = ui.rect(&first)[1];
    ui.rt.wheel(SIZE.0 / 2.0, SIZE.1 / 2.0, -200.0);
    ui.settle();
    let scrolled = ui.cards();
    assert_ne!(ui.rect(&first)[1], top, "the wheel scrolls the grid");

    ui.single_click(&ui.card("Game 05"));
    ui.settle();
    let grid = ui.find("grid-scroll").unwrap();
    assert!(grid.opacity < 0.01 && grid.inert, "the grid hides under the page: {grid:?}");
    let [px, py, pw, ph] = ui.rect("page");
    let [_, hy, _, hh] = ui.rect("add-game");
    assert!(hy + hh <= py, "the page covers the header");
    assert!(px >= 0.0 && px + pw <= SIZE.0 && py + ph <= SIZE.1, "the page leaves the window");

    ui.key(Key::Escape);
    ui.settle();
    assert_eq!(ui.cards(), scrolled, "the grid comes back scrolled where it was");
    let grid = ui.find("grid-scroll").unwrap();
    assert!(grid.opacity > 0.99 && !grid.inert);
}

/// Just after a click opens a page the grid still takes presses, so the second click of a
/// double click reaches the card. Then the page takes over.
#[test]
fn page_takes_presses_once_a_double_click_cant_happen() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    ui.single_click(&ui.card("Pong"));
    assert!(ui.find("page").unwrap().inert && !ui.find("grid-scroll").unwrap().inert);
    for _ in 0..(DOUBLE_CLICK * 60.0) as usize + 2 {
        ui.rt.frame(1.0 / 60.0);
    }
    assert!(!ui.find("page").unwrap().inert, "the page takes presses once the interval is over");
    assert!(ui.find("grid-scroll").unwrap().inert);
}

/// While a page opens, a click on another card used to open that game instead: only the card
/// that opened it takes presses, for the second click of a double click.
#[test]
fn other_cards_ignore_clicks_while_a_page_opens() {
    let mut ui = Ui::new(&[("Hatch", 0), ("Pong", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    let (pong, hatch) = (ui.card("Pong"), ui.card("Hatch"));
    ui.single_click(&pong);
    for _ in 0..5 {
        ui.rt.frame(1.0 / 60.0);
    }
    assert!(ui.rt.state.page_opening());
    ui.click(&hatch);
    ui.settle();
    assert_eq!(ui.rt.state.selected, Some(ui.game("Pong")), "a click on another card switched the page");
    assert_eq!(ui.text("title"), "Pong");
    assert_eq!(fl.count(), 0);
}

/// The cover flies from the card to the page, and back when the page closes.
#[test]
fn cover_flies_between_card_and_page() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let card = ui.card("Pong");
    let from = ui.rect(&format!("{card}/art"));
    ui.single_click(&card);
    for _ in 0..3 {
        ui.rt.frame(1.0 / 60.0);
    }
    let mid = ui.rect("cover");
    ui.settle();
    let to = ui.rect("cover");
    assert_eq!(to[2], COVER_W);
    for i in 0..4 {
        let (lo, hi) = (from[i].min(to[i]), from[i].max(to[i]));
        assert!(mid[i] > lo && mid[i] < hi || from[i] == to[i], "mid-flight {mid:?} isn't between {from:?} and {to:?}");
    }
    assert!(ui.find("art").is_none(), "the card's cover is away on the page");

    ui.key(Key::Escape);
    ui.settle();
    assert_eq!(ui.rect(&format!("{card}/art")), from, "and lands back on the card");
}

/// A cover flying home from a page draws above the whole window, whatever the grid's
/// opacity. Opening another page while it flew left it on top of the new page for most of a
/// second.
#[test]
fn a_cover_flying_home_never_shows_over_another_page() {
    let mut ui = Ui::new(&[("Hatch", 0), ("Pong", 0)]);
    let (pong, hatch) = (ui.card("Pong"), ui.card("Hatch"));
    ui.single_click(&pong);
    ui.settle();
    ui.key(Key::Escape);
    ui.rt.frame(1.0 / 60.0);
    ui.click(&hatch);
    for _ in 0..30 {
        ui.rt.frame(1.0 / 60.0);
    }
    let shown: Vec<_> = ui
        .elements()
        .into_iter()
        .filter(|e| e.key.as_deref() == Some("art") && e.opacity > 0.01)
        .map(|e| (e.path, e.opacity))
        .collect();
    assert!(shown.is_empty(), "a card's cover shows over the page: {shown:?}");
    assert_eq!(ui.text("title"), "Hatch");
}

/// A cover in flight draws above the whole window, so opening a menu lands it: one still
/// flying home from a page covered the language menu, and one flying to a page would cover
/// its pickers.
#[test]
fn a_cover_flying_home_never_shows_over_a_menu() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let card = ui.card("Pong");
    let art = format!("{card}/art");
    // In flight a cover grows or shrinks between the card's size and the page's.
    let flying = |ui: &Ui, path: &str| {
        let e = ui.elements().into_iter().find(|e| e.path.ends_with(path) && !e.exiting).unwrap();
        (e.scale - 1.0).abs() > 0.001 || e.rect[2..] != e.target[2..]
    };
    ui.single_click(&card);
    ui.settle();
    ui.key(Key::Escape);
    for _ in 0..4 {
        ui.rt.frame(1.0 / 60.0);
    }
    assert!(flying(&ui, &art), "the cover flies home from the page");
    ui.click("language-trigger");
    assert!(ui.find("language-menu").is_some());
    assert!(!flying(&ui, &art), "the cover lands as the menu opens");

    ui.key(Key::Escape);
    ui.settle();
    let root = ui.game("Pong");
    ui.rt.update(move |s, _| {
        s.protons = vec![crate::steam::Proton { name: "Proton 9.0".into(), dir: "/p".into(), runtime: None }];
        s.options_open = true;
        s.open_page(&root);
    });
    for _ in 0..4 {
        ui.rt.frame(1.0 / 60.0);
    }
    assert!(flying(&ui, "cover"), "the cover flies to the page");
    ui.click("proton-trigger");
    assert!(ui.find("proton-menu").is_some());
    assert!(!flying(&ui, "cover"), "the cover lands as the picker opens");
}

#[test]
fn launch_options_start_collapsed() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let root = ui.game("Pong");
    ui.rt.update(move |s, _| s.open_page(&root));
    ui.settle();
    assert!(ui.find("options").is_none() && ui.find("args").is_none(), "options are tucked away");
    ui.click("options-toggle");
    ui.settle();
    assert!(ui.rt.state.options_open, "the toggle opens them");
    assert!(ui.find("options").is_some() && ui.find("args").is_some(), "the toggle shows them");
    ui.click("options-toggle");
    ui.settle();
    assert!(ui.find("options").is_none(), "and hides them again");
}

/// Typing searches the library, so on a game's page it goes back to the grid.
#[test]
fn typing_on_a_page_searches_the_library() {
    let mut ui = Ui::new(&[("Pong", 0), ("Hatch", 0)]);
    ui.single_click(&ui.card("Pong"));
    ui.settle();
    ui.type_text("h");
    ui.settle();
    assert!(ui.find("page").is_none() && ui.rt.state.selected.is_none());
    assert_eq!(ui.card_names(), ["Hatch"]);
    assert!(!ui.find("grid-scroll").unwrap().inert);
}

/// The page is placed from the left edge: its parts don't move sideways as the window's width
/// changes (but for the cover's one step down in narrow windows), and nothing leaves it.
#[test]
fn game_page_is_anchored_left_and_fits() {
    let mut ui = Ui::new(&[("A Game With A Really Long Name That Goes On", 0)]);
    let root = ui.game("A Game With A Really Long Name That Goes On");
    ui.rt.update(move |s, _| {
        s.open_page(&root);
        s.options_open = true;
    });
    let keys = ["back", "cover", "title", "page-play"];
    for (from, to) in [(1300.0, 820.0), (819.0, MIN_WINDOW.0)] {
        ui.resize(from, 700.0);
        // Across only: a title that wraps onto more lines moves what's under it down.
        let at: Vec<_> = keys.iter().map(|k| ui.rect(k)[0]).collect();
        let mut w = from;
        while w >= to {
            ui.rt.set_viewport((w, 700.0), 1.0);
            ui.rt.frame(1.0 / 60.0);
            for (k, x) in keys.iter().zip(&at) {
                assert_eq!(ui.rect(k)[0], *x, "width {w}: {k} moved with the window's right edge");
            }
            let ([_, py, _, ph], [_, cy, _, ch]) = (ui.rect("page-play"), ui.rect("copy-command"));
            assert_eq!(py + ph / 2.0, cy + ch / 2.0, "width {w}: the actions wrapped onto a second line");
            for k in ["title", "actions", "options", "prefix", "copy-command"] {
                let [x, _, bw, _] = ui.rect(k);
                assert!(x + bw <= w - GUTTER + 0.5, "width {w}: {k} ends at {}", x + bw);
            }
            w -= 13.0;
        }
        let want = if from > 820.0 { COVER_W } else { COVER_W_NARROW };
        assert_eq!(ui.rect("cover")[2], want);
    }
}

#[test]
fn hover_play_button_launches_in_one_click() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    let card = ui.card("Pong");
    let play = format!("{card}/play");
    let opacity = |ui: &Ui| ui.presented(&play).map(|(_, o)| o).unwrap_or(0.0);
    assert!(opacity(&ui) < 0.01, "the play button is hidden until hover");

    let [x, y, w, h] = ui.rect(&format!("{card}/art"));
    ui.rt.pointer_move(x + w / 2.0, y + h / 4.0);
    ui.settle();
    assert!(opacity(&ui) > 0.99, "the play button fades in on hover");

    // Centred on the cover.
    let [px, py, pw, ph] = ui.rect(&play);
    assert!(((px + pw / 2.0) - (x + w / 2.0)).abs() < 0.5 && ((py + ph / 2.0) - (y + h / 2.0)).abs() < 0.5);

    ui.click(&play);
    assert_eq!(fl.count(), 1, "the hover play button launches with one click");
    ui.settle();
    assert!(ui.presented(&play).is_none(), "a running game offers no play button");
}

/// A running game's card shows a Stop button where Play was, always, not just under the
/// pointer, and the page's button turns into Stop too.
#[test]
fn running_games_can_be_stopped() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    let card = ui.card("Pong");
    ui.double_click(&card);
    assert_eq!(fl.count(), 1);
    ui.key(Key::Escape);
    ui.settle();
    // The pointer leaves the card.
    let [x, y, w, h] = ui.rect(&format!("{card}/art"));
    ui.rt.pointer_move(x + w / 2.0, y + h + 150.0);
    ui.settle();

    let stop = format!("{card}/stop");
    let ([sx, sy, sw, sh], opacity) = ui.presented(&stop).expect("a running game has a stop button");
    assert!(opacity > 0.99, "the stop button shows without hovering: {opacity}");
    assert!(((sx + sw / 2.0) - (x + w / 2.0)).abs() < 0.5 && ((sy + sh / 2.0) - (y + h / 2.0)).abs() < 0.5);
    assert!(ui.presented(&format!("{card}/play")).is_none(), "no play button while it runs");
    let dim = ui.presented(&format!("{card}/dim")).map_or(0.0, |(_, o)| o);
    assert!(dim > 0.99, "the cover is dimmed behind the stop button: {dim}");
    assert!(ui.find("running").is_some(), "the badge stays");

    ui.click(&stop);
    assert_eq!(fl.stops(), [ui.game("Pong")]);
    assert_eq!(ui.toast(), "Stopping Pong");
    fl.killed();
    ui.settle();
    assert_eq!(ui.toast(), "Pong stopped", "stopping isn't reported as a crash");
    assert!(ui.presented(&stop).is_none() && ui.find("running").is_none());

    // The page's button plays, then stops.
    ui.single_click(&card);
    ui.settle();
    ui.click("page-play");
    assert_eq!(fl.count(), 2);
    ui.settle();
    ui.click("page-play");
    assert_eq!(fl.stops().len(), 2, "the page's button stops a running game");
    fl.killed();
    ui.settle();
    ui.click("page-play");
    assert_eq!(fl.count(), 3, "and plays it again once it's gone");
}

#[test]
fn status_toast_hides_itself() {
    let mut ui = Ui::new(&[]);
    ui.rt.update(|s, _| s.set_status("Launch command copied"));
    ui.settle();
    assert_eq!(ui.toast(), "Launch command copied");
    let [_, y, _, h] = ui.rect("toast");
    assert!(y + h <= SIZE.1, "the toast is inside the window");
    for _ in 0..(TOAST_SECS * 60.0) as usize {
        ui.rt.frame(1.0 / 60.0);
    }
    ui.settle();
    assert!(ui.find("toast").is_none(), "the toast hides after {TOAST_SECS} s");
}

#[test]
fn editing_saves_settings() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let root = ui.game("Pong");
    let open = root.clone();
    ui.rt.update(move |s, _| {
        s.open_page(&open);
        s.options_open = true;
    });
    ui.settle();

    ui.click("args");
    ui.type_text("-windowed");
    assert_eq!(Config::load(&ui.paths()).settings(&root).args, "-windowed");

    ui.key(Key::Command('a'));
    ui.key(Key::Backspace);
    assert!(!Config::load(&ui.paths()).games.contains_key(&root), "clearing the only override drops the entry");
}

#[test]
fn choosing_the_default_proton_saves_nothing() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let root = ui.game("Pong");
    ui.rt.update(|s, _| {
        s.protons = ["GE-Proton10-4", "Proton 9.0"]
            .map(|n| crate::steam::Proton { name: n.into(), dir: "/p".into(), runtime: None })
            .to_vec();
        s.default_proton = "Proton 9.0".into();
    });
    let open = root.clone();
    ui.rt.update(move |s, _| s.open_page(&open));
    ui.rt.update(|s, _| s.choose_proton("GE-Proton10-4"));
    assert_eq!(Config::load(&ui.paths()).settings(&root).proton, "GE-Proton10-4");
    ui.rt.update(|s, _| s.choose_proton("Proton 9.0"));
    assert!(!Config::load(&ui.paths()).games.contains_key(&root));
}

/// Pressing reload rescans and turns its arrow once, clockwise, easing to a stop; each press
/// adds a turn. The button stays the square the other icon buttons are.
#[test]
fn reload_icon_turns_once_per_press() {
    use std::f32::consts::TAU;
    let mut ui = Ui::new(&[("Pong", 0)]);
    let [_, _, w, h] = ui.rect("rescan");
    assert_eq!((w, h), (24.0, ui.rect("install")[3]), "the reload button is an icon button's square");
    let turn = |ui: &Ui| ui.rt.presented_pose("arrow").expect("no arrow").rotation();
    assert_eq!(turn(&ui), 0.0);

    add_game(&ui.home.path().join("Games"), "Hatch", 0);
    ui.click("rescan");
    for _ in 0..6 {
        ui.rt.frame(1.0 / 60.0);
    }
    let mid = turn(&ui);
    assert!(mid > 0.0 && mid < TAU, "the arrow is partway round a tenth of a second in: {mid}");
    ui.settle();
    assert!((turn(&ui) - TAU).abs() < 1e-3, "one press, one turn: {}", turn(&ui));
    assert!(ui.rt.state.games.iter().any(|g| g.root == ui.game("Hatch")), "reload rescans");

    ui.click("rescan");
    ui.settle();
    assert!((turn(&ui) - 2.0 * TAU).abs() < 1e-3, "a second press, a second turn: {}", turn(&ui));
    assert_eq!(ui.rect("rescan")[2], 24.0, "the button doesn't change size");
}

/// A menu is coloured like what it opens from: the language menu like the header's buttons
/// (the reload button among them, which had no fill at all), and the Proton and executable
/// menus like their open fields. They were a lighter, bluer grey than either.
#[test]
fn menus_match_their_triggers() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let fill = |ui: &Ui, key: &str| ui.find(key).unwrap_or_else(|| panic!("no {key:?}")).fill;
    let button = fill(&ui, "install");
    assert!(button.a > 0.0, "the buttons are filled");
    assert_eq!(fill(&ui, "rescan"), button, "the reload button is framed like the others");
    assert_eq!(fill(&ui, "language-trigger"), button);
    ui.click("language-trigger");
    ui.settle();
    assert_eq!(fill(&ui, "language-menu"), button, "the language menu");

    ui.key(Key::Escape);
    let root = ui.game("Pong");
    ui.rt.update(move |s, _| {
        s.open_page(&root);
        s.options_open = true;
    });
    ui.settle();
    for key in ["proton", "exe"] {
        ui.click(&format!("{key}-trigger"));
        ui.settle();
        let field = fill(&ui, &format!("{key}-trigger"));
        assert!(field.a > 0.0 && field != button, "{key}: the open field is filled, darker than a button");
        assert_eq!(fill(&ui, &format!("{key}-menu")), field, "{key}: the menu is its field's colour");
        ui.key(Key::Escape);
        ui.settle();
    }
}

/// The kit's dropdown menu hung off the right end of its field, 160 points wide, and cut
/// long Proton names short. The menu sits under the field's left edge, as wide as the field.
#[test]
fn picker_menu_lines_up_with_its_field() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let root = ui.game("Pong");
    let names = ["GE-Proton11-12-x86_64", "GE-Proton10-34", "Proton - Experimental", "Proton 9.0"];
    let open = root.clone();
    ui.rt.update(move |s, _| {
        s.protons = names.map(|n| crate::steam::Proton { name: n.into(), dir: "/p".into(), runtime: None }).to_vec();
        s.default_proton = "Proton 9.0".into();
        s.open_page(&open);
        s.options_open = true;
    });
    ui.settle();
    ui.click("proton-trigger");
    ui.settle();
    let [fx, fy, fw, fh] = ui.rect("proton-trigger");
    let [mx, my, mw, _] = ui.rect("proton-menu");
    assert_eq!((mx, mw), (fx, fw), "the menu lines up with its field");
    assert!(my >= fy + fh && my - (fy + fh) <= 6.0, "the menu hangs just under the field: {my}");
    let [ox, _, ow, _] = ui.rect("proton-option0");
    assert!(ox >= mx && ox + ow <= mx + mw && ow > fw - 10.0, "an option spans the menu: {ox} + {ow}");

    ui.click("proton-option0");
    ui.settle();
    assert!(ui.find("proton-menu").is_none(), "choosing closes the menu");
    assert_eq!(Config::load(&ui.paths()).settings(&root).proton, "GE-Proton11-12-x86_64");

    // Pressing outside or Escape closes it too.
    ui.click("proton-trigger");
    ui.settle();
    ui.click("proton-scrim");
    ui.settle();
    assert!(ui.find("proton-menu").is_none() && ui.rt.state.open_select.is_none());
    ui.click("proton-trigger");
    ui.settle();
    ui.key(Key::Escape);
    ui.settle();
    assert!(ui.find("proton-menu").is_none(), "Escape closes it");
    assert!(ui.find("page").is_some(), "and leaves the page open");
}

/// Cards keep one size while the window resizes; only the column count changes. Stretching
/// them made a live resize jitter.
#[test]
fn cards_keep_their_size_while_resizing() {
    let names = ["A1", "A2", "A3", "A4", "A5", "A6", "A7"].map(|n| (n, 0));
    let mut ui = Ui::new(&names);
    // The grid under an open page too: it comes back as it was.
    for with_page in [false, true] {
        if with_page {
            let root = ui.game("A1");
            ui.rt.update(move |s, _| s.open_page(&root));
        }
        let mut prev_cols = 0;
        let mut w = 560.0;
        while w <= 1900.0 {
            ui.resize(w, 720.0);
            let cards = ui.cards();
            let mut cols: Vec<i32> = cards.iter().map(|r| r[0].round() as i32).collect();
            cols.dedup();
            cols.sort();
            cols.dedup();
            for r in &cards {
                assert_eq!(r[2], CARD_W, "width {w}: a card is {} wide", r[2]);
                assert!(r[0] + r[2] <= w, "width {w}: a card overflows the window");
            }
            assert!(cols.len() >= prev_cols, "width {w}: columns went down from {prev_cols} to {}", cols.len());
            prev_cols = cols.len();
            w += 37.0;
        }
        assert_eq!(prev_cols, 7, "page={with_page}: a wide window fits all 7 cards in a row");
    }
}

/// Tiling compositors (Hyprland) can give the window less room than its minimum size, and
/// then it's drawn cropped. The minimum stays small, and at it the header still fits, in every
/// language: where the labels don't fit, the buttons show just their icons.
#[test]
fn header_fits_at_every_width() {
    assert!(MIN_WINDOW.0 < 600.0);
    let mut ui = Ui::new(&[("A Game With A Really Long Name That Goes On", 0)]);
    let labelled = |ui: &Ui, key: &str| ui.rect(key)[2] > 60.0;
    for lang in Lang::ALL {
        ui.rt.update(move |s, _| s.lang = lang);
        for w in [MIN_WINDOW.0, 600.0, 680.0, 760.0, 900.0, 1300.0] {
            ui.resize(w, 600.0);
            for key in ["rescan", "add-folder", "add-game", "install", "language-trigger"] {
                let [x, _, bw, _] = ui.rect(key);
                assert!(x + bw <= w - 20.0, "{lang:?} at width {w}: {key} ends at {}", x + bw);
            }
            // The language button gives up its code before Add folder its label.
            let code = ui.lookup("language-trigger/code").is_some();
            assert!(
                !code || labelled(&ui, "add-folder"),
                "{lang:?} at width {w}: Add folder lost its label while the language button kept its code"
            );
            if w == MIN_WINDOW.0 && lang == Lang::En {
                for key in ["add-game", "install"] {
                    assert!(labelled(&ui, key), "English keeps {key}'s label even in the narrowest window");
                }
            }
        }
        for key in ["add-folder", "add-game", "install", "language-trigger"] {
            assert!(labelled(&ui, key), "{lang:?}: a wide window has room for {key}'s label");
        }
        assert_eq!(ui.text("language-trigger/code"), lang.short());
    }
}

/// The language button at the header's end shows the current language's code, and lists
/// every language by its own name after its code; picking one translates the UI at once and
/// is remembered over the system's language.
#[test]
fn picking_a_language_translates_the_ui_and_remembers_it() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    assert_eq!(ui.text("library-title"), "Library");
    assert_eq!(ui.text("language-trigger/code"), "EN");
    assert_eq!(ui.find("language-trigger").and_then(|e| e.label).as_deref(), Some("Language"));
    ui.click("language-trigger");
    ui.settle();
    let [tx, ty, _, th] = ui.rect("language-trigger");
    let [mx, my, mw, mh] = ui.rect("language-menu");
    assert!(my >= ty + th && my - (ty + th) <= 6.0, "the menu hangs under the button: {my}");
    assert!(mx >= tx - mw && mx + mw <= SIZE.0, "the menu stays in the window: {mx} + {mw}");
    let names: Vec<String> = (0..Lang::ALL.len())
        .map(|i| ui.find(&format!("language-option{i}")).and_then(|e| e.label).unwrap_or_default())
        .collect();
    assert_eq!(names, Lang::ALL.map(|l| l.name()));
    for (i, lang) in Lang::ALL.into_iter().enumerate() {
        assert_eq!(ui.text(&format!("language-option{i}/code")), lang.short());
        // Every language shows at once: none is cut off at the menu's end.
        let [_, oy, _, oh] = ui.rect(&format!("language-option{i}"));
        assert!(oy + oh <= my + mh, "{lang:?} is cut off at the menu's end: {} > {}", oy + oh, my + mh);
    }

    let ru = Lang::ALL.iter().position(|&l| l == Lang::Ru).unwrap();
    ui.click(&format!("language-option{ru}"));
    ui.settle();
    assert!(ui.find("language-menu").is_none(), "choosing closes the menu");
    assert_eq!(ui.text("library-title"), "Библиотека");
    assert_eq!(ui.text("language-trigger/code"), "RU");
    assert_eq!(Config::load(&ui.paths()).language, "ru");

    // The pick wins over the system's language next time.
    let app = App::new(ui.paths(), fake::start(&[]).endpoints, Lang::De);
    assert_eq!(app.lang, Lang::Ru);
    // With nothing picked, the system's language is used.
    let fresh = tempdir();
    assert_eq!(App::new(Paths::under(fresh.path()), fake::start(&[]).endpoints, Lang::Ja).lang, Lang::Ja);
}

/// An open menu covers the grid: the pointer on the menu, or on the scrim around it, never
/// hovers the cards under it.
#[test]
fn cards_under_an_open_menu_are_not_hovered() {
    let games: Vec<(String, u32)> = (0..8).map(|i| (format!("Game {i}"), 0)).collect();
    let games: Vec<(&str, u32)> = games.iter().map(|(n, id)| (n.as_str(), *id)).collect();
    let mut ui = Ui::new(&games);
    ui.click("language-trigger");
    ui.settle();
    let [mx, my, mw, mh] = ui.rect("language-menu");
    let inside = |[x, y, w, h]: [f32; 4], (px, py): (f32, f32)| px > x && px < x + w && py > y && py < y + h;
    // A point on the menu over a card's cover, and one on the scrim over another cover.
    let on_menu = (mx + mw / 2.0, my + mh / 2.0);
    let covers: Vec<(String, [f32; 4])> =
        games.iter().map(|(n, _)| ui.card(n)).map(|c| (c.clone(), ui.rect(&format!("{c}/art")))).collect();
    let (under, _) = covers.iter().find(|(_, r)| inside(*r, on_menu)).expect("a cover lies under the menu");
    let (aside, [ax, ay, aw, ah]) =
        covers.iter().find(|(_, r)| !inside([mx, my, mw, mh], (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0))).unwrap();
    let play = |ui: &Ui, card: &str| ui.presented(&format!("{card}/play")).map_or(0.0, |(_, o)| o);

    for (card, (x, y)) in [(under, on_menu), (aside, (ax + aw / 2.0, ay + ah / 4.0))] {
        ui.rt.pointer_move(x, y);
        ui.settle();
        assert!(ui.find("language-menu").is_some());
        assert!(play(&ui, card) < 0.01, "a card under the open menu shows its play button");
    }
    // Once the menu is closed, the card under the pointer hovers again.
    ui.key(Key::Escape);
    ui.settle();
    assert!(ui.find("language-menu").is_none());
    ui.rt.pointer_move(ax + aw / 2.0 + 1.0, ay + ah / 4.0);
    ui.settle();
    assert!(play(&ui, aside) > 0.99, "with the menu closed the card hovers");
}

/// Toasts are translated too, with the game's name where the language puts it.
#[test]
fn toasts_speak_the_language() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    ui.rt.update(|s, _| s.lang = Lang::Ru);
    let root = ui.game("Pong");
    ui.rt.update(move |s, _| s.play(&root));
    fl.exit();
    ui.settle();
    assert_eq!(ui.toast(), i18n::fill(i18n::RU.exited, &[&"Pong"]));
    assert_eq!(ui.toast(), "Игра Pong завершилась");
}

/// During a live resize the compositor shows the last frame pinned to the window's left
/// edge until the next lands, so anything placed from the right edge trails it and jitters.
/// Nothing in the header moves when the window's width changes, except where a button gives
/// up its label for the narrower window: then the buttons after it close up.
#[test]
fn header_is_anchored_left() {
    let mut ui = Ui::new(&[("Hatch", 0)]);
    let fixed = ["logo", "library-title", "count", "rescan"];
    let buttons = ["add-folder", "add-game", "install", "language-trigger"];
    let rects = |ui: &Ui, keys: &[&str]| keys.iter().map(|k| ui.rect(k)).collect::<Vec<_>>();
    ui.resize(1300.0, 700.0);
    let left = rects(&ui, &fixed);
    let mut prev = rects(&ui, &buttons);
    let mut w = 1299.0;
    while w >= MIN_WINDOW.0 {
        ui.rt.set_viewport((w, 700.0), 1.0);
        ui.rt.frame(1.0 / 60.0);
        assert_eq!(rects(&ui, &fixed), left, "width {w}: the header's start moved with the window's right edge");
        let now = rects(&ui, &buttons);
        let collapsed = now.iter().zip(&prev).any(|(a, b)| a[2] < b[2]);
        for ((k, a), b) in buttons.iter().zip(&now).zip(&prev) {
            if collapsed {
                assert!(a[0] <= b[0], "width {w}: {k} moved right");
            } else {
                assert_eq!(a, b, "width {w}: {k} moved with the window's right edge");
            }
        }
        prev = now;
        w -= 7.0;
    }
}

/// Text at a fractional position whose fraction changes as the window resizes shimmers.
/// Rune snaps what it paints to device pixels; this checks the layout keeps each text's
/// sub-pixel offset steady at the whole-pixel sizes a compositor gives the window.
#[test]
fn nothing_shimmers_while_resizing() {
    let mut ui = Ui::new(&[("Sky Ride", 0), ("Quiet Games Night", 0), ("Robot Party", 0), ("Hatch", 0)]);
    let root = ui.game("Hatch");
    ui.rt.update(move |s, _| {
        s.open_page(&root);
        s.options_open = true;
    });
    check_no_shimmer(&mut ui);
    // The centred empty state too.
    ui.rt.update(|s, _| {
        s.close_page();
        s.search = rune::TextEdit::new("zzzz");
    });
    check_no_shimmer(&mut ui);
}

fn check_no_shimmer(ui: &mut Ui) {
    ui.rt.update(|s, _| s.set_status("Launched Hatch with GE-Proton10-4"));
    let mut phase = std::collections::HashMap::new();
    let mut w = 640.0;
    while w <= 1500.0 {
        ui.resize(w, 720.0);
        for e in ui.elements().into_iter().filter(|e| e.text.as_ref().is_some_and(|t| !t.is_empty()) && !e.exiting) {
            let f = [e.rect[0].fract(), e.rect[1].fract()];
            if let Some(prev) = phase.insert(e.path.clone(), f) {
                let moved = (prev[0] - f[0]).abs().max((prev[1] - f[1]).abs());
                assert!(moved < 0.01, "width {w}: {:?} moved by a fraction of a pixel", e.text);
            }
        }
        w += 17.0;
    }
}

#[test]
fn steam_artwork_and_names_arrive() {
    let store = fake::start(&[(1000002, "Zebra Party")]);
    // Sorts first until its Steam name arrives.
    let mut ui = Ui::with_store(&[("Aardvark", 1000002), ("Middle", 0)], store);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while ui.rt.state.images.get(&1000002).is_none_or(|i| i.cover.is_none() || i.hero.is_none()) {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for artwork");
        std::thread::sleep(std::time::Duration::from_millis(10));
        ui.rt.frame(1.0 / 60.0);
    }
    ui.settle();
    assert_eq!(ui.card_names(), ["Middle", "Zebra Party"]);
    let images = &ui.rt.state.images[&1000002];
    assert_eq!(images.cover.as_ref().unwrap().wait().unwrap(), [600, 900], "the 2x capsule");
    assert_eq!(images.hero.as_ref().unwrap().wait().unwrap()[0], 1920, "heroes are shrunk to 1920 wide");

    let root = ui.game("Aardvark");
    ui.rt.update(move |s, _| s.open_page(&root));
    ui.settle();
    assert_eq!(ui.text("title"), "Zebra Party");
    assert!(ui.find("hero-art").is_some(), "the page shows the hero");
    let order: Vec<_> = ui.elements().into_iter().filter_map(|e| e.key).collect();
    let pos = |k: &str| order.iter().position(|x| x == k).unwrap();
    assert!(pos("backdrop") < pos("hero") && pos("hero") < pos("content"), "the hero sits behind the UI");
}

/// The starfield is blurred a little, behind the UI, which stays sharp. It's laid out in
/// whole chunks larger than the window, so a live resize rarely changes its size.
#[test]
fn backdrop_is_blurred_behind_a_sharp_ui() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let backdrop = ui.find("backdrop").unwrap();
    assert!(backdrop.blur > 0.0 && backdrop.blur <= 3.0, "blur {}", backdrop.blur);
    assert_eq!(&backdrop.rect[2..], &[1280.0, 768.0]);
    assert_eq!(ui.find("header").unwrap().blur, 0.0, "the UI isn't blurred");
    let order: Vec<_> = ui.elements().into_iter().filter_map(|e| e.key).collect();
    let pos = |k: &str| order.iter().position(|x| x == k).unwrap();
    assert!(pos("backdrop") < pos("content"), "the starfield is drawn first");

    ui.resize(1100.0, 700.0);
    assert_eq!(&ui.rect("backdrop")[2..], &[1280.0, 768.0], "a small resize keeps the backdrop's size");
}

#[test]
fn losing_focus_pauses_the_backdrop() {
    let mut ui = Ui::new(&[]);
    ui.rt.window_focus(false);
    ui.rt.frame(1.0 / 60.0);
    assert!(ui.rt.state.background.is_paused());
    ui.rt.window_focus(true);
    ui.rt.frame(1.0 / 60.0);
    assert!(!ui.rt.state.background.is_paused());
}

#[test]
fn a_hand_picked_exe_opens_its_page() {
    let mut ui = Ui::new(&[]);
    let exe = ui.home.path().join("Elsewhere/Tool/Tool.exe");
    crate::testutil::write(&exe, "x");
    let picked = exe.clone();
    ui.rt.update(move |s, _| s.add_manual_exe(picked));
    ui.settle();
    assert_eq!(ui.card_names(), ["Tool"]);
    assert_eq!(ui.rt.state.selected.as_deref(), Some(exe.parent().unwrap()));
    assert_eq!(Config::load(&ui.paths()).manual_exes, [exe]);
}

/// Picking an exe from a game already in the library used to do nothing visible when the exe
/// sat in the game's folder, and made a second card (with its own settings and prefix) when
/// it sat in a subfolder. It becomes that game's executable instead.
#[test]
fn a_hand_picked_exe_from_a_known_game_becomes_its_executable() {
    for exe in ["Editor.exe", "Bin/Win64/Pong-Shipping.exe"] {
        let mut ui = Ui::new(&[("Pong", 0)]);
        let root = ui.game("Pong");
        let exe = root.join(exe);
        crate::testutil::write(&exe, "x");
        let picked = exe.clone();
        ui.rt.update(move |s, _| s.add_manual_exe(picked));
        ui.settle();
        assert_eq!(ui.card_names(), ["Pong"], "{}: no second card", exe.display());
        assert_eq!(ui.rt.state.selected.as_deref(), Some(root.as_path()), "its page opens");
        let g = ui.rt.state.current().unwrap().clone();
        assert_eq!(ui.rt.state.spec_for(&g).unwrap().exe, exe, "and it starts the picked exe");
        assert!(Config::load(&ui.paths()).manual_exes.is_empty(), "it isn't a game of its own");
    }
}

/// Logs were only remembered for games launched since the app started, so after a restart
/// the Log button said there was none.
#[test]
fn the_log_from_an_earlier_run_opens() {
    let ui = Ui::new(&[("Pong", 0)]);
    let g = ui.rt.state.game(&ui.game("Pong")).unwrap().clone();
    assert_eq!(ui.rt.state.log_for(&g), None, "no launch, no log");
    let log = ui.paths().data_dir().join("logs/Pong.log");
    crate::testutil::write(&log, "$ proton");
    assert_eq!(ui.rt.state.log_for(&g), Some(log));
}

#[test]
fn a_broken_config_says_so() {
    let home = tempdir();
    let paths = Paths::under(home.path());
    crate::testutil::write(&paths.config_dir().join("config.json"), "{ not json");
    let mut app = App::new(paths, fake::start(&[]).endpoints, Lang::En);
    app.setup(std::sync::Arc::new(|| {}));
    let toast = app.toast.as_ref().map(|t| t.text.clone()).unwrap_or_default();
    assert!(toast.starts_with("Unreadable config.json kept as config.json.broken-"), "toast: {toast:?}");
}

#[test]
fn add_folder_asks_for_a_folder_and_scans_it() {
    let mut ui = Ui::new(&[]);
    let lib = ui.home.path().join("More");
    add_game(&lib, "Hatch", 0);
    ui.click("empty-add-folder");
    let (kind, _) = ui.rt.file_dialog().expect("a folder picker");
    assert_eq!(kind, rune::FileDialogKind::Folder);
    ui.rt.resolve_file_dialog(Some(lib.clone())).unwrap();
    ui.settle();
    assert_eq!(ui.card_names(), ["Hatch"]);
    assert!(Config::load(&ui.paths()).libraries.contains(&lib));
}

impl Ui {
    /// Gives the app one Proton build, its default, as a fake Steam would.
    fn with_proton(&mut self) {
        self.rt.update(|s, _| {
            s.protons = vec![crate::steam::Proton { name: "GE".into(), dir: "/p".into(), runtime: None }];
            s.default_proton = "GE".into();
        });
    }

    /// A repack's setup.exe in ~/Downloads and its fix beside the folder, as they download.
    fn downloads(&self) -> (PathBuf, PathBuf) {
        let downloads = self.home.path().join("Downloads");
        let setup = inno_installer(&downloads.join("Lucid Cats v2.4 by Pioneer/setup.exe"));
        (setup, inno_installer(&downloads.join("Lucid-Cats-Multiplayer-Fix-Online.exe")))
    }
}

/// Install opens a popup in the middle of the window. Choosing the game's installer (in
/// ~/Downloads) fills in the fix downloaded beside its folder, and shows where the game will
/// go. Install closes it and runs them silently one after the other, the game's first. The game
/// lands in ~/Games/asterofix, which joins the libraries, and its page opens.
#[test]
fn install_runs_the_games_installer_then_its_fix() {
    let mut ui = Ui::new(&[]);
    ui.with_proton();
    let fl = FakeLaunch::install(&mut ui);
    let (setup, fix) = ui.downloads();
    ui.click("empty-install");
    ui.settle();
    let [x, y, w, h] = ui.rect("install-dialog");
    let centre = |a: f32, len: f32, of: f32| (a + len / 2.0 - of / 2.0).abs() <= 1.0;
    assert!(centre(x, w, SIZE.0) && centre(y, h, SIZE.1), "the popup is centred: {:?}", [x, y, w, h]);
    ui.click("install-confirm");
    assert_eq!(fl.count(), 0, "nothing to install until the game's installer is chosen");
    assert!(ui.find("install-dialog").is_some());

    ui.click("install-game-browse");
    let (kind, options) = ui.rt.file_dialog().expect("a picker for the game's installer");
    assert_eq!(kind, rune::FileDialogKind::Open);
    assert_eq!(options.directory.as_deref(), Some(ui.home.path().join("Downloads").as_path()));
    ui.rt.resolve_file_dialog(Some(setup.clone())).unwrap();
    ui.settle();
    let form = |ui: &Ui| ui.rt.state.install_form.as_ref().map(|f| (f.game.clone(), f.fix.clone()));
    assert_eq!(form(&ui), Some((Some(setup.clone()), Some(fix.clone()))), "the fix beside it is found");
    assert_eq!(ui.text("install-game-name"), "Lucid Cats v2.4 by Pioneer/setup.exe");
    assert_eq!(ui.text("install-fix-name"), "Downloads/Lucid-Cats-Multiplayer-Fix-Online.exe");
    assert_eq!(ui.text("install-status"), "Installs to ~/Games/asterofix/Lucid Cats");

    // Removed, and chosen again by hand, where it was downloaded.
    ui.click("install-fix-remove");
    ui.settle();
    assert_eq!(form(&ui), Some((Some(setup.clone()), None)));
    assert_eq!(ui.text("install-fix-name"), "Not chosen");
    ui.click("install-fix-browse");
    let (_, options) = ui.rt.file_dialog().expect("a picker for the fix");
    assert_eq!(options.directory.as_deref(), Some(ui.home.path().join("Downloads").as_path()));
    ui.rt.resolve_file_dialog(Some(fix.clone())).unwrap();
    ui.settle();

    ui.click("install-confirm");
    ui.settle();
    assert!(ui.find("install-dialog").is_none(), "the popup closes");
    let dest = ui.home.path().join("Games/asterofix/Lucid Cats");
    assert_eq!(fl.count(), 1);
    let spec = fl.0.borrow().0[0].clone();
    assert_eq!((&spec.exe, &spec.game_dir), (&setup, &dest));
    assert!(spec.args.contains("/VERYSILENT"), "{}", spec.args);
    assert_eq!(ui.toast(), "Installing Lucid Cats\u{2026}");
    // It stays up for as long as the installer takes.
    for _ in 0..(TOAST_SECS * 2.0 * 60.0) as usize {
        ui.rt.frame(1.0 / 60.0);
    }
    assert_eq!(ui.toast(), "Installing Lucid Cats\u{2026}");
    ui.click("install");
    ui.settle();
    assert!(ui.find("install-dialog").is_none() && ui.toast() == "Already installing Lucid Cats");

    add_game(dest.parent().unwrap(), "Lucid Cats", 0);
    fl.exit();
    ui.settle();
    assert_eq!(fl.count(), 2, "the fix runs once the game's installer is done");
    assert_eq!(fl.0.borrow().0[1].exe, fix);
    assert_eq!(ui.toast(), "Installing the online fix for Lucid Cats\u{2026}");

    fl.exit();
    ui.settle();
    assert_eq!(ui.toast(), "Installed Lucid Cats");
    assert_eq!(ui.card_names(), ["Lucid Cats"], "a game in the new library, not one called asterofix");
    assert_eq!(ui.rt.state.selected.as_deref(), Some(dest.as_path()));
    assert!(Config::load(&ui.paths()).libraries.contains(&dest.parent().unwrap().to_path_buf()));
    assert!(ui.rt.state.installing.is_none());
}

/// What can't be installed says why in the popup, and Install stays off. Escape closes it,
/// and typing while it's open doesn't start a search behind it.
#[test]
fn the_install_popup_says_why_it_cant_install() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    ui.with_proton();
    let fl = FakeLaunch::install(&mut ui);
    let (setup, _) = ui.downloads();
    add_game(&ui.home.path().join("Games/asterofix"), "Lucid Cats", 0);
    ui.click("install");
    ui.settle();
    ui.rt.update(move |s, _| s.choose_installer(setup));
    ui.settle();
    assert!(ui.text("install-status").ends_with("Lucid Cats already exists"), "{:?}", ui.text("install-status"));
    ui.click("install-confirm");
    assert_eq!(fl.count(), 0);

    ui.type_text("p");
    assert!(!ui.rt.state.search_open);
    ui.key(Key::Escape);
    ui.settle();
    assert!(ui.find("install-dialog").is_none() && ui.rt.state.install_form.is_none());
}

/// A failed installer stops the install: its fix never runs over a half-installed game.
#[test]
fn a_failed_installer_stops_the_install() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    ui.with_proton();
    let fl = FakeLaunch::install(&mut ui);
    let (setup, fix) = ui.downloads();
    ui.rt.update(move |s, _| s.start_install(vec![setup, fix]));
    ui.settle();
    fl.0.borrow_mut().1.take().unwrap()(Err("exit status: 2".into()));
    ui.settle();
    assert_eq!(ui.toast(), "Install failed: setup.exe: exit status: 2");
    assert_eq!(fl.count(), 1);
    assert!(ui.rt.state.installing.is_none());

    // One that exits cleanly having installed nothing fails too.
    let (setup, fix) = ui.downloads();
    ui.rt.update(move |s, _| s.start_install(vec![setup, fix]));
    ui.settle();
    fl.exit();
    ui.settle();
    assert!(ui.toast().contains("without installing anything"), "toast: {:?}", ui.toast());
    assert_eq!(fl.count(), 2);
}

/// One relayout of a 40-game library at a new width (the app's share of a live resize) and
/// one backdrop frame (drawn at 15 fps off the UI thread). Both must stay well under a frame:
///
///     nix develop -c cargo test --release perf -- --ignored --nocapture
#[test]
#[ignore]
fn perf() {
    let names: Vec<String> = (0..40).map(|i| format!("Game {i}")).collect();
    let games: Vec<(&str, u32)> = names.iter().map(|n| (n.as_str(), 0)).collect();
    let mut ui = Ui::new(&games);
    let root = ui.game("Game 0");
    ui.rt.update(move |s, _| s.open_page(&root));
    ui.settle();
    let start = std::time::Instant::now();
    for i in 0..200 {
        ui.rt.set_viewport((900.0 + (i % 400) as f32, 700.0), 1.0);
        ui.rt.frame(1.0 / 60.0);
    }
    eprintln!("relayout at a new width: {:?}", start.elapsed() / 200);

    let stars = crate::background::Stars::new();
    for (w, h) in [(1280, 768), (2560, 1536)] {
        let start = std::time::Instant::now();
        for i in 0..20 {
            std::hint::black_box(stars.render(w, h, i as f64 / 15.0));
        }
        eprintln!("backdrop frame {w}x{h}: {:?}", start.elapsed() / 20);
    }
}

/// Cards a search no longer matches fade out where they stood, and fade back when it's
/// cleared, rather than popping in and out of existence.
#[test]
fn filtered_cards_fade_out_and_back() {
    let mut ui = Ui::new(&[("Hatch", 0), ("Pong", 0)]);
    let card = ui.card("Hatch");
    // As a person types: "h" opens the bar, and the next keys go into the focused field,
    // whose edits Rune applies without animation unless the field opts in.
    ui.type_text("h");
    ui.settle();
    assert!(ui.rt.state.search.focused);
    ui.type_text("z");
    ui.rt.frame(1.0 / 60.0);
    let fading = ui.leaving(&card).expect("the card animates out");
    assert!(fading > 0.0 && fading < 1.0, "opacity {fading} on its way out");
    // Slow enough to see: Rune's default fade is nearly gone (0.14) after 0.1 s.
    for _ in 0..5 {
        ui.rt.frame(1.0 / 60.0);
    }
    let later = ui.leaving(&card).unwrap_or(0.0);
    assert!(later > 0.3, "the fade is too quick to see: opacity {later} after 0.1 s");
    ui.settle();
    assert!(ui.card_names().is_empty() && ui.leaving(&card).is_none(), "gone once faded");
    assert_eq!(ui.text("empty-heading"), "No matches");

    ui.key(Key::Escape);
    ui.rt.frame(1.0 / 60.0);
    let (_, coming) = ui.presented(&card).expect("the card is back");
    assert!(coming < 1.0, "it fades back in (opacity {coming})");
    ui.settle();
    assert_eq!(ui.presented(&card).unwrap().1, 1.0);
}

#[test]
fn search_bar_fades_out() {
    for close in ["escape", "backspace"] {
        search_bar_fades_out_on(close);
    }
}

fn search_bar_fades_out_on(close: &str) {
    let mut ui = Ui::new(&[("Hatch", 0)]);
    ui.type_text("h");
    ui.settle();
    // Backspace empties the field from inside it, an edit Rune would apply instantly.
    ui.key(if close == "escape" { Key::Escape } else { Key::Backspace });
    ui.rt.frame(1.0 / 60.0);
    let fading = ui.leaving("search-row").unwrap_or_else(|| panic!("the bar animates out on {close}"));
    assert!(fading > 0.0 && fading < 1.0, "{close}: opacity {fading} on its way out");
    for _ in 0..5 {
        ui.rt.frame(1.0 / 60.0);
    }
    let later = ui.leaving("search-row").unwrap_or(0.0);
    assert!(later > 0.3, "{close}: the fade is too quick to see: opacity {later} after 0.1 s");
    ui.settle();
    assert!(ui.leaving("search-row").is_none() && ui.find("search-bar").is_none());
}

#[test]
fn toast_fades_out() {
    let mut ui = Ui::new(&[]);
    ui.rt.update(|s, _| s.set_status("hello"));
    ui.settle();
    let mut faded = None;
    for _ in 0..(TOAST_SECS * 60.0) as usize + 30 {
        ui.rt.frame(1.0 / 60.0);
        if let Some(o) = ui.leaving("toast-row") {
            faded = Some(o);
            break;
        }
    }
    let o = faded.expect("the toast animates out when it times out");
    assert!(o > 0.0 && o < 1.0, "opacity {o}");
}

/// The asterisk mark sits just left of the title, its centre on the middle of the capitals.
#[test]
fn asterisk_mark_beside_the_title() {
    let ui = Ui::new(&[]);
    let [lx, ly, lw, lh] = ui.rect("logo");
    let [tx, ty, _, th] = ui.rect("library-title");
    assert!(lx + lw <= tx + 1.0 && tx - (lx + lw) < 8.0, "the asterisk is right before the title");
    assert_eq!(lh, th, "set on the title's line");
    assert_eq!(ly - ty, 8.0, "lowered onto the capitals' middle, in whole points");
}

/// The drop comes from the font's metrics. At the title's own size it's the 0.1995 em
/// measured from the glyph outlines: (asterisk ink centre - cap height / 2) / em.
#[test]
fn asterisk_drop_from_font_metrics() {
    use super::view::asterisk_drop;
    assert!((asterisk_drop(22.0, 22.0) - 0.1995 * 22.0).abs() < 0.01, "{}", asterisk_drop(22.0, 22.0));
    // Bigger marks drop further: their centre rides higher above the shared line middle.
    assert!(asterisk_drop(40.0, 22.0) > asterisk_drop(22.0, 22.0));
    assert_eq!(asterisk_drop(40.0, 22.0).round(), 8.0);
}
