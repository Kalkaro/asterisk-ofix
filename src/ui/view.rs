//! Drawing `App`: the starfield, the header, the card grid, the floating side panel for the
//! open game, and the status toast.

use super::theme::{self, *};
use super::{App, Images};
use crate::art::has_art;
use crate::background::chunked;
use crate::scan::Game;
use rune::{
    Color, Cursor, Cx, El, ScrollSpec, Spring, SpringPair, SurfaceFit, Transition, Tx, col, el, icon, row, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Select, SelectAppearance, TextInput,
};
use std::path::Path;

pub const CARD_W: f32 = 184.0;
/// Steam library capsules are 600x900.
pub const CARD_H: f32 = CARD_W * 1.5;
pub const GAP: f32 = 22.0;
pub const GUTTER: f32 = 28.0;
pub const PANEL_W: f32 = 380.0;
pub const PANEL_MARGIN: f32 = 12.0;
const HERO_H: f32 = 210.0;
/// Softens the starfield (Gaussian sigma, points). The UI above stays sharp.
pub const BG_BLUR: f32 = 0.8;
/// Header buttons drop their labels below this window width.
const SEARCH_W: f32 = 520.0;
/// Things that come and go (cards a search hides, the toast) fade over about a third of a
/// second. Rune's default presence fade is over in ~0.1 s, which reads as popping.
const FADE: SpringPair = SpringPair::new(Spring::new(0.35, 0.0), Spring::new(0.35, 0.0));
/// Google Sans Flex SemiBold's vertical metrics, in ems: ascender and descender, cap
/// height, and the middle of the asterisk's ink above the baseline.
const ASCENT: f32 = 0.966;
const DESCENT: f32 = 0.286;
const CAP_HEIGHT: f32 = 0.716;
const ASTERISK_MID: f32 = 0.5575;

/// How far below the middle of its line a point `height` ems above the baseline sits, for
/// text of `size` vertically centred in a row (the same at any line height).
fn below_line_middle(size: f32, height: f32) -> f32 {
    size * ((ASCENT - DESCENT) / 2.0 - height)
}

/// How far to lower an asterisk of `mark` size beside a title of `title` size so the
/// asterisk's centre sits on the middle of the title's capitals. The glyph normally rides
/// up near the cap height.
pub fn asterisk_drop(mark: f32, title: f32) -> f32 {
    below_line_middle(title, CAP_HEIGHT / 2.0) - below_line_middle(mark, ASTERISK_MID)
}
const SEARCH_H: f32 = 52.0;
const SEARCH_BOTTOM: f32 = 28.0;

/// Named states for headless snapshots (`--snap out.png --scene panel`).
type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("panel", |s| {
        if let Some(root) = s.games.first().map(|g| g.root.clone()) {
            s.open_panel(&root)
        }
    }),
    ("search", |s| {
        s.search = rune::TextEdit::new("or");
        s.search_open = true;
    }),
    ("no-matches", |s| {
        s.search = rune::TextEdit::new("zzzz");
        s.search_open = true;
    }),
    ("toast", |s| s.set_status("Launched Neon Harbor with Proton 9.0")),
];

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let k = theme::kit();
    let mut layer = Vec::new();
    let size = cx.layout_with("app", |r| [r[2], r[3]]).unwrap_or([super::WINDOW.0, super::WINDOW.1]);
    let backdrop = chunked(size);
    // One pixel per point: the ASCII cells keep their size on HiDPI screens, and the blur
    // hides the upscale.
    s.background.set_size(backdrop[0] as u32, backdrop[1] as u32);

    let content = col().key("content").w_full().h_full().child(header(s, &k)).child(main(s, &k, size[0], &mut layer));
    let app = el()
        .key("app")
        .w_full()
        .h_full()
        .fill(BACKGROUND)
        .child(
            el().key("backdrop")
                .absolute(0.0, 0.0)
                .size(backdrop[0], backdrop[1])
                .pixels(s.background.pixels())
                .blur(BG_BLUR)
                .inert(),
        )
        .child(content)
        .children(s.search_open.then(|| search_bar(s, &k, size[0])))
        .children(s.toast.as_ref().map(|t| toast(&t.text, t.seq, size[0], s.search_open)));
    rune_kit::root(app, layer).on_window_focus(|s: &mut App, focused, tx: &mut Tx| {
        // No point animating behind other windows; the clock picks up where it stopped.
        s.background.set_paused(!focused);
        tx.live_only();
    })
}

fn header(s: &App, k: &Kit) -> El {
    let visible = s.visible().len();
    let count =
        if visible == s.games.len() { s.games.len().to_string() } else { format!("{visible} of {}", s.games.len()) };
    let button = |label: &'static str, glyph: &'static rune::Icon, key: &'static str, f: fn(&mut App, &mut Tx)| {
        Button::new(label).icon(glyph).variant(ButtonVariant::Neutral).on_press_tx(f).build(k).key(key)
    };
    // Everything is anchored to the left edge. While a window is resized, the compositor
    // shows the last frame pinned to the left until the next one lands, so content pinned
    // to the right edge trails it and jitters (`header_is_anchored_left`).
    row()
        .key("header")
        .w_full()
        .px(GUTTER)
        .pt(20.0)
        .pb(14.0)
        .gap(10.0)
        .center_y()
        .child(
            // The font's asterisk sits up near the cap height; this lowers its centre onto the
            // middle of the capitals beside it.
            text("*")
                .key("logo")
                .font(LOGO)
                .color(TEXT)
                .offset(0.0, asterisk_drop(LOGO.size, PAGE_TITLE.size).round())
                // The glyph's side bearings already space it; the row's gap would double that.
                .mr(-7.0),
        )
        .child(text("Library").key("library-title").font(PAGE_TITLE).color(TEXT))
        .child(text(count).key("count").font(CAPTION_STRONG).color(MUTED).px(8.0).py(2.0).radius(10.0).fill(RAISED))
        .child(
            IconButton::new(&REFRESH)
                .label("Rescan")
                .help("Rescan library folders")
                .variant(IconButtonVariant::Ghost)
                .on_press(App::rescan)
                .build(k)
                .key("rescan"),
        )
        .child(button("Add folder", &FOLDER_PLUS, "add-folder", App::add_folder))
        .child(button("Add game", &PLUS, "add-game", App::add_game))
}

/// The search: hidden until you start typing, then floating over the bottom of the window.
fn search_bar(s: &App, k: &Kit, window_w: f32) -> El {
    let visible = s.visible().len();
    let count = if s.search.text.trim().is_empty() {
        format!("{} games", s.games.len())
    } else {
        format!("{visible} of {}", s.games.len())
    };
    let field = TextInput::new(&s.search)
        .placeholder("Search games")
        .label("Search")
        .plain()
        .width(None)
        .text_style(SEARCH)
        .on_edit(|s: &mut App| &mut s.search)
        // The field would take Escape to drop its focus; here it closes the search.
        .on_key(|s: &mut App, ev: &rune::KeyEvent, _: &mut Tx| {
            let escape = matches!(ev.key, rune::Key::Escape);
            if escape {
                s.close_search();
            }
            escape
        })
        .on_change(|s: &mut App, _| {
            // Deleting everything typed puts it away again.
            if s.search.text.is_empty() {
                s.close_search();
            }
        })
        .build(k);
    let pop = Spring::new(0.42, 0.18);
    row()
        .key("search-row")
        .pin(Some(GUTTER), None, Some(GUTTER + odd(window_w)), Some(SEARCH_BOTTOM))
        .justify_center()
        // On the row, which is what comes and goes, so the bar animates out as well as in.
        .transition(
            Transition::offset(0.0, 36.0)
                .combined(Transition::scale(0.92))
                .combined(Transition::opacity())
                .springs(SpringPair::new(pop, Spring::new(0.32, 0.0))),
        )
        .child(
            row()
                .key("search-bar")
                .w((window_w - 2.0 * GUTTER).min(SEARCH_W))
                .h(SEARCH_H)
                .pl(16.0)
                .pr(10.0)
                .gap(10.0)
                .center_y()
                .radius(SEARCH_H / 2.0)
                .backdrop_blur(14.0)
                .fill(RAISED.alpha(0.82))
                .border(1.0, Color::WHITE.alpha(0.10))
                .shadow(18.0, 44.0, Color::BLACK.alpha(0.75))
                .child(icon(k.glyph.search, 18.0).color(MUTED))
                .child(el().key("search").grow(1.0).min_w(0.0).child(field))
                .child(text(count).key("search-count").font(CAPTION).color(MUTED))
                .child(
                    text("esc")
                        .key("search-close")
                        .font(CAPTION_STRONG)
                        .color(MUTED)
                        .px(7.0)
                        .py(2.0)
                        .radius(6.0)
                        .border(1.0, BORDER)
                        .cursor(Cursor::Pointer)
                        .help("Close search")
                        .on_press(App::close_search),
                ),
        )
}

/// The grid, with the side panel floating over its right edge.
fn main(s: &App, k: &Kit, width: f32, layer: &mut Vec<El>) -> El {
    let visible = s.visible();
    // The grid stays mounted while a search narrows it, so cards that stop matching fade
    // out in place (and come back the same way) rather than vanishing with it.
    let grid = col()
        .key("grid-scroll")
        .w_full()
        .h_full()
        .child(
            row()
                .key("grid")
                .w_full()
                .flex_wrap()
                .gaps(GAP + 6.0, GAP)
                .px(GUTTER)
                .pt(6.0)
                .pb(GUTTER)
                .children(visible.iter().map(|g| card(s, g))),
        )
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(18.0);
    let empty = visible.is_empty().then(|| {
        empty(s, k)
            .absolute(0.0, 0.0)
            .pr(odd(width))
            .transition(Transition::opacity().combined(Transition::offset(0.0, 8.0)).springs(FADE))
    });
    el().key("main")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(grid)
        .children(empty)
        .children(s.current().map(|g| panel(s, k, g, (width - 2.0 * PANEL_MARGIN).min(PANEL_W), layer)))
}

/// Keys can't hold `/` (it separates a key path), so the folder is slugged.
pub fn card_key(root: &Path) -> String {
    format!("card-{}", crate::launch::slug(&root.to_string_lossy()))
}

fn card(s: &App, g: &Game) -> El {
    let name = s.display_name(g);
    let root = g.root.clone();
    let active = s.selected.as_deref() == Some(g.root.as_path());
    let running = s.running.contains(&g.root);
    let cover = s.images.get(&g.real_id).and_then(|i: &Images| i.cover.as_ref());
    let layer = || el().absolute(0.0, 0.0).size(CARD_W, CARD_H);

    let mut art = el()
        .key("art")
        .size(CARD_W, CARD_H)
        .radius(12.0)
        .fill(tile_color(&name))
        .clip_rounded(12.0)
        .center()
        .child(text(initials(&name)).font(MONOGRAM).color(TEXT.alpha(0.75)));
    if let Some(img) = cover {
        art = art.child(layer().pixels_fit(img.pixels(), SurfaceFit::Fill));
    }
    // Darkens the art under the pointer, more at the bottom where the name sits.
    art = art.child(layer().gradient(Color::BLACK.alpha(0.25), Color::BLACK.alpha(0.6)).group_opacity(0.0, 1.0));
    if !running {
        let root = root.clone();
        art = art.child(
            el().key("play")
                .absolute((CARD_W - 56.0) / 2.0, (CARD_H - 56.0) / 2.0)
                .size(56.0, 56.0)
                .radius(28.0)
                .fill(PRIMARY)
                .shadow(6.0, 20.0, Color::BLACK.alpha(0.5))
                .center()
                .role("button")
                .label("Play")
                .cursor(Cursor::Pointer)
                .press_scale(0.92)
                .group_opacity(0.0, 1.0)
                .on_press(move |s: &mut App| s.play(&root))
                .child(icon(&PLAY, 22.0).color(BACKGROUND)),
        );
    } else {
        art = art.child(running_badge());
    }
    // A hairline at rest, a bright ring under the pointer or while the panel is open.
    art = art.child(layer().radius(12.0).border(1.0, Color::WHITE.alpha(0.08))).child(
        layer().radius(12.0).border(2.0, Color::WHITE.alpha(0.8)).group_opacity(if active { 1.0 } else { 0.0 }, 1.0),
    );

    let open = root.clone();
    col()
        .key(card_key(&g.root))
        .transition(Transition::opacity().combined(Transition::scale(0.94)).springs(FADE))
        .w(CARD_W)
        .gap(2.0)
        .role("button")
        .label(name.clone())
        .hover_group()
        .cursor(Cursor::Pointer)
        .press_scale(0.97)
        .on_press(move |s: &mut App| s.toggle_panel(&open))
        // The second click of a double click toggled the panel shut; it stays open.
        .on_double_press(move |s: &mut App| {
            s.open_panel(&root);
            s.play(&root);
        })
        .child(art.mb(8.0))
        .child(text(name).key("name").font(BODY).color(TEXT).truncate().w_full())
}

fn running_badge() -> El {
    row()
        .key("running")
        .absolute(8.0, 8.0)
        .gap(6.0)
        .center_y()
        .px(9.0)
        .py(4.0)
        .radius(10.0)
        .fill(Color::BLACK.alpha(0.78))
        .child(el().size(7.0, 7.0).radius(3.5).fill(GREEN).shadow(0.0, 6.0, GREEN.alpha(0.8)))
        .child(text("Running").font(CAPTION_STRONG).color(TEXT))
}

fn empty(s: &App, k: &Kit) -> El {
    let none = s.games.is_empty();
    let (heading, body) = if none {
        ("No games yet", "Add a folder that contains OnlineFix games,\nor pick a game's .exe directly.".to_owned())
    } else {
        ("No matches", format!("Nothing in your library matches \u{201c}{}\u{201d}.", s.search.text.trim()))
    };
    let mut c = col()
        .key("empty")
        .w_full()
        .h_full()
        .center()
        .gap(8.0)
        .child(icon(&EMPTY, 64.0).color(FAINT))
        .child(text(heading).key("empty-heading").font(SUBHEADING).color(TEXT))
        .child(text(body).font(BODY).color(MUTED).align(rune::Align::Center));
    if none {
        c = c.child(
            row()
                .key("empty-actions")
                .gap(8.0)
                .mt(10.0)
                .child(
                    Button::new("Add folder")
                        .icon(&FOLDER_PLUS)
                        .variant(ButtonVariant::Contrast)
                        .on_press_tx(App::add_folder)
                        .build(k)
                        .key("empty-add-folder"),
                )
                .child(
                    Button::new("Add game")
                        .icon(&PLUS)
                        .variant(ButtonVariant::Neutral)
                        .on_press_tx(App::add_game)
                        .build(k),
                ),
        );
    }
    c
}

/// One game's banner, Play button and launch options, floating over the grid's right edge.
fn panel(s: &App, k: &Kit, g: &Game, width: f32, layer: &mut Vec<El>) -> El {
    let name = s.display_name(g);
    let settings = s.cfg.settings(&g.root);
    let spec = s.spec_for(g).ok();
    let detected = s.detected();
    let running = s.running.contains(&g.root);
    let inner = width - 40.0;

    let mut meta = Vec::new();
    if has_art(g.real_id) {
        meta.push(format!("Steam app {}", g.real_id));
    }
    if let Some(exe) = spec.as_ref().and_then(|s| s.exe.file_name()) {
        meta.push(exe.to_string_lossy().into_owned());
    }

    let mut hero = el().key("hero").w_full().h(HERO_H).fill(tile_color(&name).mix(Color::WHITE, 0.04));
    if let Some(img) = s.images.get(&g.real_id).and_then(|i| i.hero.as_ref()) {
        hero = hero.child(
            el().key("hero-art").absolute(0.0, 0.0).size(width, HERO_H).pixels_fit(img.pixels(), SurfaceFit::Fill),
        );
    }
    hero = hero
        .child(el().absolute(0.0, 0.0).size(width, HERO_H).gradient(SIDEBAR.alpha(0.0), SIDEBAR))
        .child(
            el().pin(None, Some(12.0), Some(12.0), None).radius(9.0).fill(Color::BLACK.alpha(0.55)).child(
                IconButton::new(k.glyph.close)
                    .label("Close")
                    .help("Close (Esc)")
                    .variant(IconButtonVariant::Ghost)
                    .on_press(App::close_panel)
                    .build(k)
                    .key("close"),
            ),
        )
        .child(
            col()
                .pin(Some(20.0), None, Some(20.0), Some(14.0))
                .gap(2.0)
                .child(text(name).key("title").font(PAGE_TITLE).color(TEXT).wrap())
                .child(text(meta.join("  ·  ")).key("meta").font(CAPTION).color(MUTED).wrap()),
        );

    let root = g.root.clone();
    let play = if running {
        Button::new("Running").variant(ButtonVariant::Neutral).control_size(ControlSize::ExtraLarge).disabled(true)
    } else {
        Button::new("Play")
            .icon(&PLAY)
            .variant(ButtonVariant::Contrast)
            .control_size(ControlSize::ExtraLarge)
            .on_press(move |s: &mut App| s.play(&root))
    };

    let proton_names: Vec<&str> = s.protons.iter().map(|p| p.name.as_str()).collect();
    let proton = Select::new("proton", &proton_names)
        .label("Proton")
        .appearance(SelectAppearance::Field)
        .placeholder("No Proton found")
        .width(inner)
        .current(spec.as_ref().map(|s| s.proton.name.as_str()).filter(|n| !n.is_empty()))
        .open(s.open_select == Some("proton"))
        .on_toggle(|s: &mut App| s.open_select = if s.open_select == Some("proton") { None } else { Some("proton") })
        .on_close(|s: &mut App| s.open_select = None)
        .on_choose(|s: &mut App, v: &str, _: &mut Tx| s.choose_proton(v))
        .build(k, layer);

    let rel = |p: &Path| p.strip_prefix(&g.root).unwrap_or(p).to_string_lossy().into_owned();
    let mut exes: Vec<String> = g.exes.iter().map(|e| rel(e)).collect();
    if !settings.exe.is_empty() && !g.exes.iter().any(|e| e.as_os_str() == settings.exe.as_str()) {
        exes.push(rel(Path::new(&settings.exe)));
    }
    let current_exe = spec.as_ref().map(|s| rel(&s.exe));
    let exe = Select::new("exe", &exes)
        .label("Executable")
        .appearance(SelectAppearance::Field)
        .placeholder("No executable found")
        .width(inner - 84.0)
        .current(current_exe.as_deref())
        .open(s.open_select == Some("exe"))
        .on_toggle(|s: &mut App| s.open_select = if s.open_select == Some("exe") { None } else { Some("exe") })
        .on_close(|s: &mut App| s.open_select = None)
        .on_choose(|s: &mut App, v: &str, _: &mut Tx| s.choose_exe(v))
        .build(k, layer);

    let d = detected.unwrap_or_default();
    let fields = col()
        .key("options")
        .w_full()
        .px(20.0)
        .pt(6.0)
        .pb(18.0)
        .gap(14.0)
        .child(play.build(k).key("panel-play").w_full())
        .child(field("Proton", proton))
        .child(
            col()
                .mt(8.0)
                .gap(2.0)
                .child(text("Launch options").font(SUBHEADING).color(TEXT))
                .child(text("Empty fields use the detected defaults.").font(CAPTION).color(MUTED)),
        )
        .child(field(
            "Executable",
            row().gap(8.0).center_y().child(exe).child(
                Button::new("Browse")
                    .variant(ButtonVariant::Neutral)
                    .on_press_tx(App::browse_exe)
                    .build(k)
                    .key("browse"),
            ),
        ))
        .child(
            row()
                .w_full()
                .gap(12.0)
                .child(
                    input(k, "fake-id", "Steam app ID", &s.form.fake_id, &d.fake_app_id.to_string(), |s| {
                        &mut s.form.fake_id
                    })
                    .grow(1.0),
                )
                .child(input(k, "args", "Arguments", &s.form.args, "none", |s| &mut s.form.args).grow(1.0)),
        )
        .child(input(k, "overrides", "DLL overrides", &s.form.overrides, &d.overrides, |s| &mut s.form.overrides))
        .child(input(k, "env", "Environment", &s.form.env, "KEY=value …", |s| &mut s.form.env))
        .child(input(k, "prefix", "Wine prefix", &s.form.prefix, &d.prefix.to_string_lossy(), |s| &mut s.form.prefix))
        .child(el().w_full().h(1.0).fill(BORDER).mt(4.0))
        .child(
            row()
                .w_full()
                .gap(4.0)
                .center_y()
                .child(ghost(k, "Folder", &FOLDER, |s, _| {
                    if let Some(root) = s.selected.clone() {
                        s.open_path(&root)
                    }
                }))
                .child(ghost(k, "Log", &DOCUMENT, |s, _| s.open_log()))
                .child(el().grow(1.0))
                .child(ghost(k, "Copy command", k.glyph.copy, App::copy_command)),
        );

    col()
        .key("panel")
        .pin(None, Some(0.0), Some(PANEL_MARGIN), Some(PANEL_MARGIN))
        .w(width)
        .fill(SIDEBAR)
        .radius(16.0)
        .border(1.0, BORDER)
        .shadow(18.0, 48.0, Color::BLACK.alpha(0.7))
        .clip_rounded(16.0)
        .transition(Transition::offset(28.0, 0.0).combined(Transition::opacity()))
        .child(
            col()
                .key("panel-scroll")
                .w_full()
                .h_full()
                .child(col().w_full().child(hero).child(fields))
                .scroll(ScrollSpec { stick_bottom: false }),
        )
}

/// 1 for an odd width. Content centred in a box of even width keeps the same sub-pixel
/// offset at every window width, so it doesn't shimmer during a live resize.
fn odd(width: f32) -> f32 {
    width.round().rem_euclid(2.0)
}

/// A small caption above a control.
fn field(label: &str, control: El) -> El {
    col().w_full().gap(6.0).child(text(label).font(CAPTION).color(MUTED)).child(control)
}

fn input(
    k: &Kit,
    key: &'static str,
    label: &str,
    edit: &rune::TextEdit,
    placeholder: &str,
    access: fn(&mut App) -> &mut rune::TextEdit,
) -> El {
    let field_el = TextInput::new(edit)
        .placeholder(placeholder)
        .label(label)
        .width(None)
        .on_edit(access)
        .on_change(|s: &mut App, _| s.save_form())
        .build(k);
    field(label, el().key(key).w_full().child(field_el))
}

fn ghost(k: &Kit, label: &'static str, glyph: &'static rune::Icon, f: fn(&mut App, &mut Tx)) -> El {
    Button::new(label).icon(glyph).variant(ButtonVariant::Ghost).on_press_tx(f).build(k)
}

/// A floating message at the bottom of the window.
fn toast(message: &str, seq: u64, window_w: f32, above_search: bool) -> El {
    // Rides above the search bar while it's open.
    let bottom = if above_search { SEARCH_BOTTOM + SEARCH_H + 12.0 } else { 24.0 };
    row()
        .key("toast-row")
        .pin(Some(GUTTER), None, Some(GUTTER + odd(window_w)), Some(bottom))
        .motion(Spring::new(0.35, 0.1))
        // The row is what goes when the toast times out; the pill inside swaps per message.
        .transition(Transition::offset(0.0, 12.0).combined(Transition::opacity()).springs(FADE))
        .justify_center()
        .child(
            row()
                .key(format!("toast-{seq}"))
                .max_w(640.0)
                .gap(10.0)
                .center_y()
                .px(16.0)
                .py(10.0)
                .radius(12.0)
                .fill(RAISED)
                .border(1.0, BORDER)
                .shadow(10.0, 30.0, Color::BLACK.alpha(0.6))
                .transition(Transition::offset(0.0, 12.0).combined(Transition::opacity()).springs(FADE))
                .child(el().size(6.0, 6.0).radius(3.0).fill(PRIMARY))
                .child(text(message).key("toast").font(BODY).color(TEXT).truncate()),
        )
}
