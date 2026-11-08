//! The real view, driven through Rune's headless runtime: pointer, keys, resizes and the
//! virtual clock. Nothing here touches the real machine: each test gets its own temp HOME,
//! a fake Steam store, and a fake launcher instead of Proton.

use super::view::{self, CARD_W, PANEL_MARGIN, card_key};
use super::{App, MIN_WINDOW, TOAST_SECS};
use crate::art::fake;
use crate::config::Config;
use crate::launch::{LaunchSpec, OnExit};
use crate::paths::Paths;
use crate::testutil::{TempDir, add_game, tempdir};
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
        let mut app = App::new(Paths::under(home.path()), store.endpoints);
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
        let keys: Vec<&str> = path.split('/').collect();
        let all = self.elements();
        let found = all.iter().enumerate().find(|(_, e)| {
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
        Some((found.1.rect, found.1.opacity))
    }

    /// The opacity of `key` while it animates away, if it's on its way out.
    fn leaving(&self, key: &str) -> Option<f32> {
        self.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && e.exiting).map(|e| e.opacity)
    }

    fn text(&self, key: &str) -> String {
        self.find(key).and_then(|e| e.text).unwrap_or_default()
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
    assert!(ui.find("panel").is_none() && ui.find("empty").is_none(), "only the library shows");
}

#[test]
fn empty_library_offers_to_add_games() {
    let ui = Ui::new(&[]);
    assert_eq!(ui.text("empty-heading"), "No games yet");
    assert!(ui.find("empty-actions").is_some());
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
    ui.rt.update(move |s, _| s.open_panel(&root));
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
    assert_eq!(ui.rt.state.selected, Some(ui.game("Pong")), "the first click opened the panel");
    ui.settle();
    assert!(ui.find("running").is_some(), "the card shows the running badge");

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
fn single_click_opens_the_side_panel() {
    let mut ui = Ui::new(&[("Pong", 0), ("Hatch", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    let (a, b) = (ui.card("Pong"), ui.card("Hatch"));

    ui.single_click(&a);
    ui.settle();
    assert!(ui.find("panel").is_some(), "a click opens the side panel");
    assert_eq!(fl.count(), 0, "a single click doesn't launch");
    assert_eq!(ui.text("title"), "Pong");
    assert!(ui.text("meta").contains("Pong.exe"), "meta: {}", ui.text("meta"));

    ui.single_click(&b);
    ui.settle();
    assert_eq!(ui.text("title"), "Hatch", "another card switches the panel");
    ui.single_click(&b);
    ui.settle();
    assert!(ui.find("panel").is_none() && ui.rt.state.selected.is_none(), "clicking the open game closes it");
    ui.single_click(&b);
    ui.settle();
    assert_eq!(ui.rt.state.selected, Some(ui.game("Hatch")), "and clicking again opens it");

    ui.key(Key::Escape);
    ui.settle();
    assert!(ui.find("panel").is_none() && ui.rt.state.selected.is_none(), "Escape closes the panel");

    ui.single_click(&a);
    ui.settle();
    ui.click("panel-play");
    assert_eq!(fl.count(), 1, "the panel's Play button launches");
    ui.click("close");
    ui.settle();
    assert!(ui.find("panel").is_none(), "the close button closes the panel");
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
/// pointer, and the panel's button turns into Stop too.
#[test]
fn running_games_can_be_stopped() {
    let mut ui = Ui::new(&[("Pong", 0)]);
    let fl = FakeLaunch::install(&mut ui);
    let card = ui.card("Pong");
    ui.double_click(&card);
    assert_eq!(fl.count(), 1);
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

    // The panel's button plays, then stops.
    ui.click("panel-play");
    assert_eq!(fl.count(), 2);
    ui.settle();
    ui.click("panel-play");
    assert_eq!(fl.stops().len(), 2, "the panel's button stops a running game");
    fl.killed();
    ui.settle();
    ui.click("panel-play");
    assert_eq!(fl.count(), 3, "and plays it again once it's gone");
}

/// The panel floats over the grid: opening it never moves a card, at any window size, and
/// it never covers the header.
#[test]
fn panel_floats_over_the_grid() {
    let mut ui = Ui::new(&[("A1", 0), ("A2", 0), ("A3", 0), ("A4", 0), ("A5", 0)]);
    for w in [640.0, 900.0, 1300.0] {
        ui.resize(w, 700.0);
        let before = ui.cards();
        let root = ui.game("A1");
        ui.rt.update(move |s, _| s.open_panel(&root));
        ui.settle();
        assert_eq!(ui.cards(), before, "width {w}: a card moved when the panel opened");
        let [px, py, pw, _] = ui.rect("panel");
        assert!((px + pw - (w - PANEL_MARGIN)).abs() < 0.5, "width {w}: the panel ends at {}", px + pw);
        let [_, hy, _, hh] = ui.rect("add-game");
        assert!(hy + hh <= py, "width {w}: the panel covers the header");
        ui.rt.update(|s, _| s.close_panel());
        ui.settle();
    }
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
    ui.rt.update(move |s, _| s.open_panel(&open));
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
    ui.rt.update(move |s, _| s.open_panel(&open));
    ui.rt.update(|s, _| s.choose_proton("GE-Proton10-4"));
    assert_eq!(Config::load(&ui.paths()).settings(&root).proton, "GE-Proton10-4");
    ui.rt.update(|s, _| s.choose_proton("Proton 9.0"));
    assert!(!Config::load(&ui.paths()).games.contains_key(&root));
}

/// Cards keep one size while the window resizes; only the column count changes. Stretching
/// them made a live resize jitter.
#[test]
fn cards_keep_their_size_while_resizing() {
    let names = ["A1", "A2", "A3", "A4", "A5", "A6", "A7"].map(|n| (n, 0));
    let mut ui = Ui::new(&names);
    for with_panel in [false, true] {
        if with_panel {
            let root = ui.game("A1");
            ui.rt.update(move |s, _| s.open_panel(&root));
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
        assert_eq!(prev_cols, 7, "panel={with_panel}: a wide window fits all 7 cards in a row");
    }
}

/// Tiling compositors (Hyprland) can give the window less room than its minimum size, and
/// then it's drawn cropped. The minimum stays small, and at it the header still fits.
#[test]
fn header_fits_at_every_width() {
    assert!(MIN_WINDOW.0 < 600.0);
    let mut ui = Ui::new(&[("A Game With A Really Long Name That Goes On", 0)]);
    for w in [MIN_WINDOW.0, 760.0, 900.0, 1300.0] {
        ui.resize(w, 600.0);
        for key in ["rescan", "add-folder", "add-game"] {
            let [x, _, bw, _] = ui.rect(key);
            assert!(x + bw <= w - 20.0, "width {w}: {key} ends at {}", x + bw);
        }
    }
}

/// During a live resize the compositor shows the last frame pinned to the window's left
/// edge until the next lands, so anything placed from the right edge trails it and jitters.
/// Nothing in the header moves when the window's width changes.
#[test]
fn header_is_anchored_left() {
    let mut ui = Ui::new(&[("Hatch", 0)]);
    let keys = ["logo", "library-title", "count", "rescan", "add-folder", "add-game"];
    ui.resize(1300.0, 700.0);
    let wide: Vec<_> = keys.iter().map(|k| ui.rect(k)).collect();
    let mut w = 1299.0;
    while w >= MIN_WINDOW.0 {
        ui.rt.set_viewport((w, 700.0), 1.0);
        ui.rt.frame(1.0 / 60.0);
        for (k, r) in keys.iter().zip(&wide) {
            assert_eq!(&ui.rect(k), r, "width {w}: {k} moved with the window's right edge");
        }
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
    ui.rt.update(move |s, _| s.open_panel(&root));
    check_no_shimmer(&mut ui);
    // The centred empty state too.
    ui.rt.update(|s, _| s.search = rune::TextEdit::new("zzzz"));
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
    ui.rt.update(move |s, _| s.open_panel(&root));
    ui.settle();
    assert_eq!(ui.text("title"), "Zebra Party");
    assert!(ui.find("hero-art").is_some(), "the panel shows the hero");
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
fn a_hand_picked_exe_opens_its_panel() {
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
        assert_eq!(ui.rt.state.selected.as_deref(), Some(root.as_path()), "its panel opens");
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
    let mut app = App::new(paths, fake::start(&[]).endpoints);
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
    ui.rt.update(move |s, _| s.open_panel(&root));
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
