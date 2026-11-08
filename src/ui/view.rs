//! Drawing `App`: the starfield, the header, the card grid, the floating side panel for the
//! open game, and the status toast.

use super::theme::{self, *};
use super::{App, Images};
use crate::art::has_art;
use crate::background::chunked;
use crate::scan::Game;
use rune::{Color, Cursor, Cx, El, ScrollSpec, SurfaceFit, Transition, Tx, col, el, icon, row, text};
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
const COMPACT_BELOW: f32 = 820.0;

/// Named states for headless snapshots (`--snap out.png --scene panel`).
type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("panel", |s| {
        if let Some(root) = s.games.first().map(|g| g.root.clone()) {
            s.open_panel(&root)
        }
    }),
    ("no-matches", |s| s.search = rune::TextEdit::new("zzzz")),
    ("toast", |s| s.set_status("Launched Pong with GE-Proton10-4")),
];

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let k = theme::kit();
    let mut layer = Vec::new();
    let size = cx.layout_with("app", |r| [r[2], r[3]]).unwrap_or([super::WINDOW.0, super::WINDOW.1]);
    let backdrop = chunked(size);
    // One pixel per point: the ASCII cells keep their size on HiDPI screens, and the blur
    // hides the upscale.
    s.background.set_size(backdrop[0] as u32, backdrop[1] as u32);

    let content =
        col().key("content").w_full().h_full().child(header(s, &k, size[0])).child(main(s, &k, size[0], &mut layer));
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
        .children(s.toast.as_ref().map(|t| toast(&t.text, t.seq, size[0])));
    rune_kit::root(app, layer).on_window_focus(|s: &mut App, focused, tx: &mut Tx| {
        // No point animating behind other windows; the clock picks up where it stopped.
        s.background.set_paused(!focused);
        tx.live_only();
    })
}

fn header(s: &App, k: &Kit, width: f32) -> El {
    let compact = width < COMPACT_BELOW;
    let visible = s.visible().len();
    let count =
        if visible == s.games.len() { s.games.len().to_string() } else { format!("{visible} of {}", s.games.len()) };
    let title =
        row().gap(10.0).center_y().child(text("Library").font(PAGE_TITLE).color(TEXT)).child(
            text(count).key("count").font(CAPTION_STRONG).color(MUTED).px(8.0).py(2.0).radius(10.0).fill(RAISED),
        );
    let mut search = TextInput::new(&s.search).placeholder("Search games").label("Search").leading_icon(k.glyph.search);
    if !s.search.text.is_empty() {
        search = search.clear_button();
    }
    let search =
        search.width(None).on_edit(|s: &mut App| &mut s.search).on_action(|s: &mut App, _| s.search.clear()).build(k);
    let button = |label: &'static str, glyph: &'static rune::Icon, key: &'static str| {
        if compact {
            IconButton::new(glyph).label(label).help(label).variant(IconButtonVariant::Neutral).build(k).key(key)
        } else {
            Button::new(label).icon(glyph).variant(ButtonVariant::Neutral).build(k).key(key)
        }
    };
    row()
        .key("header")
        .w_full()
        .px(GUTTER)
        .pt(20.0)
        .pb(14.0)
        .gap(10.0)
        .center_y()
        .child(title)
        .child(
            row()
                .grow(1.0)
                .min_w(0.0)
                .justify_end()
                .child(el().key("search").w_full().max_w(280.0).min_w(110.0).child(search)),
        )
        .child(
            IconButton::new(&REFRESH)
                .label("Rescan")
                .help("Rescan library folders")
                .variant(IconButtonVariant::Ghost)
                .on_press(App::rescan)
                .build(k)
                .key("rescan"),
        )
        .child(attach(button("Add folder", &FOLDER_PLUS, "add-folder"), |s: &mut App, tx| s.add_folder(tx)))
        .child(attach(button("Add game", &PLUS, "add-game"), |s: &mut App, tx| s.add_game(tx)))
}

/// Header buttons are built before knowing which form they take, so they get their handler
/// here.
fn attach(e: El, f: fn(&mut App, &mut Tx)) -> El {
    e.on_press_tx(f)
}

/// The grid, with the side panel floating over its right edge.
fn main(s: &App, k: &Kit, width: f32, layer: &mut Vec<El>) -> El {
    let visible = s.visible();
    let body = if visible.is_empty() {
        empty(s, k).pr(odd(width))
    } else {
        let cards = visible.iter().map(|g| card(s, g));
        col()
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
                    .children(cards),
            )
            .scroll(ScrollSpec { stick_bottom: false })
            .scroll_edge_fade(18.0)
    };
    el().key("main")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(body)
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
fn toast(message: &str, seq: u64, window_w: f32) -> El {
    row().key("toast-row").pin(Some(GUTTER), None, Some(GUTTER + odd(window_w)), Some(24.0)).justify_center().child(
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
            .transition(Transition::offset(0.0, 12.0).combined(Transition::opacity()))
            .child(el().size(6.0, 6.0).radius(3.0).fill(PRIMARY))
            .child(text(message).key("toast").font(BODY).color(TEXT).truncate()),
    )
}
