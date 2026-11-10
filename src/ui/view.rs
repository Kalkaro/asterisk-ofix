//! Drawing `App`: the starfield, the header, the card grid, the page for the open game, and
//! the status toast.

use super::i18n::{Lang, Strings, fill};
use super::theme::{self, *};
use super::{App, Images};
use crate::art::has_art;
use crate::background::chunked;
use crate::launch::LaunchSpec;
use crate::scan::Game;
use rune::{
    Align, Color, Cursor, Curve, Cx, Easing, El, ScrollSpec, Side, Spring, SpringPair, SurfaceFit, Transition, Tx, col,
    el, icon, row, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Dialog, IconButton, IconButtonVariant, Kit, Select, SelectAppearance, TextInput,
};
use std::path::Path;

pub const CARD_W: f32 = 184.0;
/// Steam library capsules are 600x900.
pub const CARD_H: f32 = CARD_W * 1.5;
pub const GAP: f32 = 22.0;
pub const GUTTER: f32 = 28.0;
/// A game page's cover, and below `PAGE_WIDE` window width the smaller one. Both 2:3, as
/// the cards' covers are, so a cover flying between the two keeps its shape.
pub const COVER_W: f32 = 240.0;
pub const COVER_W_NARROW: f32 = 152.0;
const PAGE_WIDE: f32 = 820.0;
const PAGE_GAP: f32 = 32.0;
/// The page's text column stops growing here, so lines stay readable. It's wide enough for
/// Play, Folder, Log and Copy command with their labels in every language (French's are the
/// longest, at 577 points).
const INFO_MAX: f32 = 580.0;
/// Steam heroes are 1920x620.
const HERO_ASPECT: f32 = 1920.0 / 620.0;
/// Softens the starfield (Gaussian sigma, points). The UI above stays sharp.
pub const BG_BLUR: f32 = 0.8;
const SEARCH_W: f32 = 520.0;
/// What the header takes besides its texts' own widths: gutters, gaps, the logo, the icon
/// buttons, and the labelled buttons' icons and padding: 315 points, plus the 20 kept clear of
/// the window's edge and a point for each button's rounding.
const HEADER_CHROME: f32 = 338.0;
/// A labelled button is its label and this wide; with just its icon it's `ICON_BUTTON` wide.
const LABEL_PAD: f32 = 46.0;
const ICON_BUTTON: f32 = 24.0;
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

/// Named states for headless snapshots (`--snap out.png --scene page`).
type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("page", |s| {
        if let Some(root) = s.games.first().map(|g| g.root.clone()) {
            s.open_page(&root)
        }
    }),
    ("options", |s| {
        if let Some(root) = s.games.first().map(|g| g.root.clone()) {
            s.open_page(&root);
            s.options_open = true;
        }
    }),
    ("proton-menu", |s| {
        if let Some(root) = s.games.first().map(|g| g.root.clone()) {
            s.open_page(&root);
            s.options_open = true;
            s.open_select = Some("proton");
        }
    }),
    ("search", |s| {
        s.search = rune::TextEdit::new("game");
        s.search_open = true;
    }),
    ("no-matches", |s| {
        s.search = rune::TextEdit::new("zzzz");
        s.search_open = true;
    }),
    ("toast", |s| s.set_status(fill(s.t().launched, &[&"Pong", &"Proton 9.0"]))),
    ("language-menu", |s| s.open_select = Some("language")),
    // A repack and its fix chosen, as they'd come from ~/Downloads.
    ("install", |s| {
        let downloads = s.paths.home.join("Downloads");
        s.install_form = Some(super::InstallForm {
            game: Some(downloads.join("Pong v1.2 by Someone/setup.exe")),
            fix: Some(downloads.join("Pong-Multiplayer-Fix-Online.exe")),
            plan: Some(Ok(crate::install::Plan {
                name: "Pong".into(),
                dest: s.paths.home.join(crate::install::INSTALL_DIR).join("Pong"),
                steps: Vec::new(),
                fresh: true,
            })),
        });
    }),
    // A repack halfway through installing.
    ("install-progress", |s| {
        let downloads = s.paths.home.join("Downloads");
        let step = crate::install::Step {
            exe: downloads.join("Pong v1.2 by Someone/setup.exe"),
            kind: crate::install::Kind::Inno,
            is_fix: false,
        };
        let plan = crate::install::Plan {
            name: "Pong".into(),
            dest: s.paths.home.join(crate::install::INSTALL_DIR).join("Pong"),
            steps: vec![step],
            fresh: true,
        };
        s.installing = Some((plan, 0));
        s.install_size = Some(12_400_000_000);
        let t = s.t();
        s.set_status(fill(t.already_installing, &[&"Pong"]));
        s.search = rune::TextEdit::new("game");
        s.search_open = true;
    }),
    // The second game is running, with its page open.
    ("running", |s| {
        if let Some(root) = s.games.get(1).map(|g| g.root.clone()) {
            s.running.insert(root.clone());
            s.open_page(&root)
        }
    }),
];

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let k = theme::kit();
    let mut layer = Vec::new();
    let size = cx.layout_with("app", |r| [r[2], r[3]]).unwrap_or([super::WINDOW.0, super::WINDOW.1]);
    let backdrop = chunked(size);
    // One pixel per point: the ASCII cells keep their size on HiDPI screens, and the blur
    // hides the upscale.
    s.background.set_size(backdrop[0] as u32, backdrop[1] as u32);

    let content = col()
        .key("content")
        .w_full()
        .h_full()
        .child(header(s, &k, size[0], &mut layer))
        .child(main(s, &k, size[0], &mut layer));
    // An open menu covers everything: the pointer on it, or on the scrim around it, hovers
    // nothing under it, or the cards it lies over light up (`cards_under_an_open_menu_are_not_hovered`).
    let content = if s.open_select.is_some() { content.inert() } else { content };
    install_dialog(s, &k, &mut layer);
    // From the bottom: the search bar, the install's progress, then a toast.
    let floor = if s.search_open { SEARCH_BOTTOM + SEARCH_H + 12.0 } else { 24.0 };
    let progress = install_progress(s, &k, size[0], floor);
    let toast_bottom = if progress.is_some() { floor + PROGRESS_H + 10.0 } else { floor };
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
        .children(hero(s, backdrop[0]))
        .child(content)
        .children(s.search_open.then(|| search_bar(s, &k, size[0])))
        .children(progress)
        .children(s.toast.as_ref().map(|t| toast(&t.text, t.seq, size[0], toast_bottom)));
    rune_kit::root(app, layer).on_window_focus(|s: &mut App, focused, tx: &mut Tx| {
        // No point animating behind other windows; the clock picks up where it stopped.
        s.background.set_paused(!focused);
        tx.live_only();
    })
}

/// How many of the header's buttons (the language, Add folder, Add game, Install) keep their
/// labels at `width`. Some languages' labels are twice as long as English's. As the window
/// narrows the language button shows just its globe first, then Add folder its icon, then Add
/// game, then Install (`header_fits_at_every_width`).
fn header_labels(t: &Strings, count: &str, lang: Lang, width: f32) -> usize {
    let w = |s: &str, style| rune::text::measure(s, &[], style, None).0;
    let labels = [t.add_folder, t.add_game, t.install];
    // What each button takes beyond its icon, in the order they give it up. `HEADER_CHROME`
    // counts the language button as an icon button, and the others' icons and padding.
    let language = w(lang.short(), BODY_STRONG) + LABEL_PAD + LANGUAGE_CHEVRON_GAP + LANGUAGE_CHEVRON - ICON_BUTTON;
    let drops = [language].into_iter().chain(labels.iter().map(|l| w(l, BODY_STRONG) + LABEL_PAD - ICON_BUTTON));
    let mut need = HEADER_CHROME
        + language
        + w(t.library, PAGE_TITLE)
        + w(count, CAPTION_STRONG)
        + labels.iter().map(|l| w(l, BODY_STRONG)).sum::<f32>();
    for (i, drop) in drops.enumerate() {
        if width >= need {
            return 1 + labels.len() - i;
        }
        need -= drop;
    }
    0
}

fn header(s: &App, k: &Kit, width: f32, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let visible = s.visible().len();
    let count =
        if visible == s.games.len() { s.games.len().to_string() } else { fill(t.of, &[&visible, &s.games.len()]) };
    // Those before `unlabelled` show just their icons: 0 is the language button.
    let unlabelled = 4 - header_labels(t, &count, s.lang, width);
    let button =
        |i: usize, label: &'static str, glyph: &'static rune::Icon, key: &'static str, f: fn(&mut App, &mut Tx)| {
            if i >= unlabelled {
                Button::new(label).icon(glyph).variant(ButtonVariant::Neutral).on_press_tx(f).build(k).key(key)
            } else {
                IconButton::new(glyph).label(label).help(label).on_press_tx(f).build(k).key(key)
            }
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
                .mr(-7.0)
                .role("button")
                .label(t.copy_diagnostics)
                .help(t.copy_diagnostics)
                .cursor(Cursor::Pointer)
                .on_press_tx(App::copy_diagnostics),
        )
        .child(text(t.library).key("library-title").font(PAGE_TITLE).color(TEXT))
        .child(text(count).key("count").font(CAPTION_STRONG).color(MUTED).px(8.0).py(2.0).radius(10.0).fill(RAISED))
        .child(reload_button(s, k))
        .child(button(1, t.add_folder, &FOLDER_PLUS, "add-folder", App::add_folder))
        .child(button(2, t.add_game, &PLUS, "add-game", App::add_game))
        .child(button(3, t.install, &DOWNLOAD, "install", |s, _| s.open_install()))
        .child(language_button(s, k, unlabelled == 0, layer))
}

/// The reload button: an icon button whose arrow turns once, clockwise as it points, each time
/// it's pressed. The kit's `IconButton` can't turn its icon alone, so this is a `Button` with
/// the icon for its label, cut to the icon button's square (`reload_icon_turns_once_per_press`).
fn reload_button(s: &App, k: &Kit) -> El {
    let turn = s.reload_turns as f32 * std::f32::consts::TAU;
    Button::new(s.t().rescan)
        .help(s.t().rescan_help)
        .variant(ButtonVariant::Neutral)
        .label_with(move |_, ink| {
            icon(&REFRESH, 16.0)
                .key("arrow")
                .color(ink)
                .rotate(turn)
                // Quick off the mark, easing to a stop.
                .rotate_motion(Curve::tween(0.55, Easing::cubic(0.3, 0.0, 0.1, 1.0)))
        })
        .on_press(App::reload)
        .build(k)
        .key("rescan")
        .w(ICON_BUTTON)
        .px(0.0)
}

/// The language button at the header's end, framed like the buttons beside it: the globe,
/// the current language's code and a chevron, or just the globe where there's no room. Its
/// menu lists every language in itself, all at once, after its code.
fn language_button(s: &App, k: &Kit, labelled: bool, layer: &mut Vec<El>) -> El {
    const KEY: &str = "language";
    let open = s.open_select == Some(KEY);
    if open {
        let names: Vec<String> = Lang::ALL.iter().map(|l| l.name().to_owned()).collect();
        let code = |i: usize| {
            Some(text(Lang::ALL[i].short()).key("code").font(CAPTION_STRONG).color(MUTED).w(LANGUAGE_CODE_W))
        };
        // Coloured like the button it opens from.
        let fill = k.c.bg_button_neutral;
        let menu = Menu { key: KEY, label: s.t().language, width: 220.0, rows: Lang::ALL.len() as f32, fill };
        menu.build(k, &names, Some(s.lang.name()), App::choose_language, &code, layer);
    }
    let toggle = |s: &mut App, tx: &mut Tx| s.toggle_select(KEY, tx);
    if !labelled {
        let label = s.t().language;
        return IconButton::new(&GLOBE)
            .label(label)
            .help(label)
            .expanded(open)
            .on_press_tx(toggle)
            .build(k)
            .key("language-trigger");
    }
    let (short, chevron) = (s.lang.short(), k.glyph.chevron_down);
    Button::new(s.t().language)
        .icon(&GLOBE)
        .variant(ButtonVariant::Neutral)
        .expanded(open)
        .label_with(move |font, ink| {
            row()
                .gap(LANGUAGE_CHEVRON_GAP)
                .center_y()
                .child(text(short).key("code").font(font).color(ink))
                .child(icon(chevron, LANGUAGE_CHEVRON).color(MUTED))
        })
        .on_press_tx(toggle)
        .build(k)
        .key("language-trigger")
}

/// The language button's chevron after its code, and the gap before it.
const LANGUAGE_CHEVRON: f32 = 12.0;
const LANGUAGE_CHEVRON_GAP: f32 = 4.0;
/// The column of codes before the names in the language menu.
const LANGUAGE_CODE_W: f32 = 22.0;

/// The search: hidden until you start typing, then floating over the bottom of the window.
fn search_bar(s: &App, k: &Kit, window_w: f32) -> El {
    let t = s.t();
    let visible = s.visible().len();
    let count = if s.search.text.trim().is_empty() {
        (t.games)(s.games.len())
    } else {
        fill(t.of, &[&visible, &s.games.len()])
    };
    let field = TextInput::new(&s.search)
        .placeholder(t.search_placeholder)
        .label(t.search)
        .plain()
        .width(None)
        .text_style(SEARCH)
        // Rune applies a field's edits with no animation, and with them whatever they change:
        // cards a keystroke filters out vanished, and so did the bar when Backspace emptied
        // it. This keeps the typed text immediate and animates the rest.
        .animate_edit_layout()
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
                        .help(t.close_search)
                        .on_press(App::close_search),
                ),
        )
}

/// The open game's hero art, faint behind the header and the top of its page, fading into
/// the starfield. Sized with the backdrop's chunks and pinned top-left, so a live resize rarely
/// moves it.
/// The open game's hero art, faint behind the page. It stays mounted after the page closes
/// and fades out in place: Rune paints a removed element above everything else while it
/// fades, so the hero tinted the header's buttons with the game's colours.
fn hero(s: &App, width: f32) -> Option<El> {
    let img = s.images.get(&s.hero_id?).and_then(|i| i.hero.as_ref())?;
    let h = (width / HERO_ASPECT).round();
    Some(
        el().key("hero")
            .absolute(0.0, 0.0)
            .size(width, h)
            .inert()
            .clip_fade_linear(0.0, 0.0, 0.0, (h * 0.7).round())
            .opacity(if s.current().is_some() { 1.0 } else { 0.0 })
            .opacity_motion(FADE)
            // Fades in the first time a page opens.
            .transition(Transition::opacity().springs(FADE))
            .child(
                el().key("hero-art")
                    .absolute(0.0, 0.0)
                    .size(width, h)
                    .pixels_fit(img.pixels(), SurfaceFit::Fill)
                    .opacity(0.3)
                    .blur(2.0),
            ),
    )
}

/// The grid, or the open game's page in its place. The grid stays mounted under the page,
/// hidden, so it comes back scrolled where it was.
fn main(s: &App, k: &Kit, width: f32, layer: &mut Vec<El>) -> El {
    let visible = s.visible();
    let page = s.current();
    // Just after a click on a card opens its page, a second click still goes to the card.
    let opening = s.page_opening();
    // The grid stays mounted while a search narrows it, so cards that stop matching fade
    // out in place (and come back the same way) rather than vanishing with it.
    let mut grid = col()
        .key("grid-scroll")
        .w_full()
        .h_full()
        .opacity(if page.is_some() { 0.0 } else { 1.0 })
        .opacity_motion(Spring::new(0.3, 0.0))
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
    if page.is_some() && !opening {
        grid = grid.inert();
    }
    let empty = visible.is_empty().then(|| {
        empty(s, k)
            .absolute(0.0, 0.0)
            .pr(odd(width))
            .transition(Transition::opacity().combined(Transition::offset(0.0, 8.0)).springs(FADE))
    });
    el().key("main").w_full().grow(1.0).min_h(0.0).child(grid).children(empty).children(page.map(|g| {
        let p = game_page(s, k, g, width, layer);
        if opening { p.inert() } else { p }
    }))
}

/// Keys can't hold `/` (it separates a key path), so the folder is escaped. Escaped, not
/// slugged: a slug gave "A B" and "A_B" the same key.
pub fn card_key(root: &Path) -> String {
    format!("card-{}", root.to_string_lossy().replace('%', "%25").replace('/', "%2F"))
}

/// A game's cover flies between its card and its page.
fn cover_id(root: &Path) -> String {
    format!("cover-{}", card_key(root))
}

fn card(s: &App, g: &Game) -> El {
    let t = s.t();
    let name = s.display_name(g);
    let root = g.root.clone();
    let running = s.running.contains(&g.root);
    let cover = s.images.get(&g.real_id).and_then(|i: &Images| i.cover.as_ref());
    let layer = || el().absolute(0.0, 0.0).size(CARD_W, CARD_H);

    let art = if s.selected.is_some() {
        // The grid is hidden under a page, and the open game's cover is on it; it flies back
        // here when the page closes. A cover still flying home goes too: a flight draws over
        // the whole window whatever the grid's opacity, so it would show over the page.
        el().key("art-away").size(CARD_W, CARD_H)
    } else {
        let mut art = el()
            .key("art")
            .size(CARD_W, CARD_H)
            .radius(12.0)
            .fill(tile_color(&name))
            .clip_rounded(12.0)
            .matched(cover_id(&g.root))
            .center()
            .child(text(initials(&name)).font(MONOGRAM).color(TEXT.alpha(0.75)));
        if let Some(img) = cover {
            art = art.child(layer().pixels_fit(img.pixels(), SurfaceFit::Fill));
        }
        // Darkens the art under the pointer, more at the bottom where the name sits, and
        // always while the game runs, so the Stop button stands out on light covers.
        let dim = if running { 1.0 } else { 0.0 };
        art = art.child(
            layer().key("dim").gradient(Color::BLACK.alpha(0.25), Color::BLACK.alpha(0.6)).group_opacity(dim, 1.0),
        );
        let at = root.clone();
        art = if !running {
            // Fades in under the pointer.
            art.child(
                cover_button("play", t.play, &PLAY).group_opacity(0.0, 1.0).on_press(move |s: &mut App| s.play(&at)),
            )
        } else {
            // Always there while the game runs.
            art.child(running_badge(t))
                .child(cover_button("stop", t.stop, &STOP).on_press(move |s: &mut App| s.stop(&at)))
        };
        // A hairline at rest, a bright ring under the pointer.
        art.child(layer().radius(12.0).border(1.0, Color::WHITE.alpha(0.08)))
            .child(layer().radius(12.0).border(2.0, Color::WHITE.alpha(0.8)).group_opacity(0.0, 1.0))
    };

    let at = root.clone();
    let card = col()
        .key(card_key(&g.root))
        .transition(Transition::opacity().combined(Transition::scale(0.94)).springs(FADE))
        .w(CARD_W)
        .gap(2.0)
        .role("button")
        .label(name.clone())
        .hover_group()
        .cursor(Cursor::Pointer)
        .press_scale(0.97)
        .on_press(move |s: &mut App| s.open_from_card(&at))
        .on_double_press(move |s: &mut App| s.play(&root))
        .child(art.mb(8.0))
        .child(text(name).key("name").font(BODY).color(TEXT).truncate().w_full());
    // While a page opens, the grid takes presses only for the second click of a double click
    // on the card that opened it; a click on another card would open that game instead.
    if s.page_opening() && s.selected.as_deref() != Some(g.root.as_path()) { card.inert() } else { card }
}

/// The round button in the middle of a card's cover.
fn cover_button(key: &str, label: &str, glyph: &'static rune::Icon) -> El {
    el().key(key)
        .absolute((CARD_W - 56.0) / 2.0, (CARD_H - 56.0) / 2.0)
        .size(56.0, 56.0)
        .radius(28.0)
        .fill(PRIMARY)
        .shadow(6.0, 20.0, Color::BLACK.alpha(0.5))
        .center()
        .role("button")
        .label(label)
        .cursor(Cursor::Pointer)
        .press_scale(0.92)
        .child(icon(glyph, 22.0).color(BACKGROUND))
}

fn running_badge(t: &Strings) -> El {
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
        .child(text(t.running).font(CAPTION_STRONG).color(TEXT))
}

fn empty(s: &App, k: &Kit) -> El {
    let t = s.t();
    let none = s.games.is_empty();
    let (heading, body) = if none {
        (t.no_games, t.no_games_body.to_owned())
    } else {
        (t.no_matches, fill(t.no_matches_body, &[&s.search.text.trim()]))
    };
    let mut c = col()
        .key("empty")
        .w_full()
        .h_full()
        .center()
        .gap(8.0)
        .child(icon(&EMPTY, 64.0).color(FAINT))
        .child(text(heading).key("empty-heading").font(SUBHEADING).color(TEXT))
        .child(text(body).key("empty-body").font(BODY).color(MUTED).align(rune::Align::Center));
    if none {
        c = c.child(
            row()
                .key("empty-actions")
                .gap(8.0)
                .mt(10.0)
                .child(
                    Button::new(t.add_folder)
                        .icon(&FOLDER_PLUS)
                        .variant(ButtonVariant::Contrast)
                        .on_press_tx(App::add_folder)
                        .build(k)
                        .key("empty-add-folder"),
                )
                .child(
                    Button::new(t.add_game)
                        .icon(&PLUS)
                        .variant(ButtonVariant::Neutral)
                        .on_press_tx(App::add_game)
                        .build(k),
                )
                .child(
                    Button::new(t.install)
                        .icon(&DOWNLOAD)
                        .variant(ButtonVariant::Neutral)
                        .on_press(App::open_install)
                        .build(k)
                        .key("empty-install"),
                ),
        );
    }
    c
}

/// One game, in the grid's place: its cover, name, Play and launch options. Everything is
/// placed from the left edge, so nothing trails a live resize.
fn game_page(s: &App, k: &Kit, g: &Game, width: f32, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let name = s.display_name(g);
    let spec = s.spec_for(g).ok();
    let running = s.running.contains(&g.root);
    let cover_w = if width >= PAGE_WIDE { COVER_W } else { COVER_W_NARROW };
    // Even, so the two fields that share a line get whole-point widths and don't shimmer.
    let info_w = ((width - 2.0 * GUTTER - cover_w - PAGE_GAP).clamp(0.0, INFO_MAX) / 2.0).floor() * 2.0;

    let mut cover = el()
        .key("cover")
        .size(cover_w, cover_w * 1.5)
        .radius(14.0)
        .fill(tile_color(&name))
        .clip_rounded(14.0)
        .matched(cover_id(&g.root))
        .shadow(12.0, 40.0, Color::BLACK.alpha(0.6))
        .center()
        .child(text(initials(&name)).font(MONOGRAM).color(TEXT.alpha(0.75)));
    if let Some(img) = s.images.get(&g.real_id).and_then(|i| i.cover.as_ref()) {
        cover = cover
            .child(el().absolute(0.0, 0.0).size(cover_w, cover_w * 1.5).pixels_fit(img.pixels(), SurfaceFit::Fill));
    }
    cover = cover
        .child(el().absolute(0.0, 0.0).size(cover_w, cover_w * 1.5).radius(14.0).border(1.0, Color::WHITE.alpha(0.08)));
    if running {
        cover = cover.child(running_badge(t));
    }

    let mut meta = Vec::new();
    if has_art(g.real_id) {
        meta.push(fill(t.steam_app, &[&g.real_id]));
    }
    if let Some(spec) = &spec {
        if let Some(exe) = spec.exe.file_name() {
            meta.push(exe.to_string_lossy().into_owned());
        }
        if !spec.proton.name.is_empty() {
            meta.push(spec.proton.name.clone());
        }
    }

    let root = g.root.clone();
    let play = if running {
        Button::new(t.stop)
            .icon(&STOP)
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::ExtraLarge)
            .on_press(move |s: &mut App| s.stop(&root))
    } else {
        Button::new(t.play)
            .icon(&PLAY)
            .variant(ButtonVariant::Contrast)
            .control_size(ControlSize::ExtraLarge)
            .on_press(move |s: &mut App| s.play(&root))
    };
    let actions = row()
        .key("actions")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(el().w(PLAY_W).mr(8.0).child(play.build(k).key("page-play").w_full()))
        .children(page_actions(k, t, page_labels(t, info_w)));

    let info = col()
        .key("info")
        .w(info_w)
        .gap(6.0)
        .child(text(name).key("title").font(GAME_TITLE).color(TEXT).wrap())
        .child(text(meta.join("  ·  ")).key("meta").font(BODY).color(MUTED).wrap())
        .child(actions.mt(20.0))
        .child(el().w_full().h(1.0).fill(BORDER).mt(22.0).mb(4.0))
        .child(options_toggle(t, s.options_open))
        .children(s.options_open.then(|| launch_options(s, k, g, spec.as_ref(), info_w, layer)));

    col()
        .key("page")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .transition(Transition::offset(0.0, 10.0).combined(Transition::opacity()).springs(FADE))
        .child(
            col()
                .key("page-scroll")
                .w_full()
                .h_full()
                .child(
                    col()
                        .w_full()
                        .px(GUTTER)
                        .pb(GUTTER)
                        .gap(14.0)
                        .child(
                            el().self_start().ml(-10.0).child(
                                Button::new(t.back)
                                    .icon(&BACK)
                                    .variant(ButtonVariant::Ghost)
                                    .help(t.back_help)
                                    .on_press(App::close_page)
                                    .build(k)
                                    .key("back"),
                            ),
                        )
                        .child(row().items_start().gap(PAGE_GAP).child(cover).child(info)),
                )
                .scroll(ScrollSpec { stick_bottom: false })
                .scroll_edge_fade(18.0),
        )
}

/// How many of the page's Folder, Log and Copy command keep their labels beside Play in a
/// column `width` wide. Some languages' labels are much longer than English's; as the column
/// narrows Copy command shows just its icon first, then Log, then Folder, so the row never
/// wraps (`page_actions_stay_on_one_line`).
fn page_labels(t: &Strings, width: f32) -> usize {
    let labels = [t.folder, t.log, t.copy_command];
    // Play and its margin, and the gaps before the three.
    let mut need = PLAY_W + 8.0 + 3.0 * 8.0;
    // A point each for rounding.
    need += labels.iter().map(|l| rune::text::measure(l, &[], BODY_STRONG, None).0 + LABEL_PAD + 1.0).sum::<f32>();
    for (i, label) in labels.iter().enumerate().rev() {
        if width >= need {
            return i + 1;
        }
        need -= rune::text::measure(label, &[], BODY_STRONG, None).0 + LABEL_PAD + 1.0 - ICON_BUTTON;
    }
    0
}

/// The page's Play button.
const PLAY_W: f32 = 160.0;

/// Folder, Log and Copy command; just their icons where their labels don't fit (`page_labels`).
fn page_actions(k: &Kit, t: &Strings, labelled: usize) -> [El; 3] {
    let mut i = 0;
    let mut action = |key: &str, label: &'static str, glyph: &'static rune::Icon, f: fn(&mut App, &mut Tx)| {
        i += 1;
        if i > labelled {
            IconButton::new(glyph).label(label).help(label).variant(IconButtonVariant::Ghost).on_press_tx(f).build(k)
        } else {
            ghost(k, label, glyph, f)
        }
        .key(key)
    };
    [
        action("folder", t.folder, &FOLDER, |s, _| {
            if let Some(root) = s.selected.clone() {
                s.open_path(&root)
            }
        }),
        action("log", t.log, &DOCUMENT, |s, _| s.open_log()),
        action("copy-command", t.copy_command, k.glyph.copy, App::copy_command),
    ]
}

/// "Launch options", with a caret that turns as they open.
fn options_toggle(t: &Strings, open: bool) -> El {
    row()
        .key("options-toggle")
        .gap(8.0)
        .center_y()
        .py(8.0)
        .self_start()
        .role("button")
        .label(t.launch_options)
        .cursor(Cursor::Pointer)
        .on_press(App::toggle_options)
        .child(icon(&CARET, 16.0).key("caret").color(MUTED).rotate(if open {
            std::f32::consts::FRAC_PI_2
        } else {
            0.0
        }))
        .child(text(t.launch_options).font(SUBHEADING).color(TEXT))
}

/// The game's settings: Proton, which .exe, and what it starts with.
fn launch_options(s: &App, k: &Kit, g: &Game, spec: Option<&LaunchSpec>, width: f32, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let settings = s.cfg.settings(&g.root);
    let protons: Vec<String> = s.protons.iter().map(|p| p.name.clone()).collect();
    let current_proton = spec.map(|s| s.proton.name.as_str()).filter(|n| !n.is_empty());
    let proton = Picker { key: "proton", label: "Proton", placeholder: t.no_proton, width }.build(
        s,
        k,
        &protons,
        current_proton,
        App::choose_proton,
        layer,
    );

    let rel = |p: &Path| p.strip_prefix(&g.root).unwrap_or(p).to_string_lossy().into_owned();
    let mut exes: Vec<String> = g.exes.iter().map(|e| rel(e)).collect();
    if !settings.exe.is_empty() && !g.exes.iter().any(|e| e.as_os_str() == settings.exe.as_str()) {
        exes.push(rel(Path::new(&settings.exe)));
    }
    let current_exe = spec.map(|s| rel(&s.exe));
    let exe = Picker { key: "exe", label: t.executable, placeholder: t.no_executable, width: width - 84.0 }.build(
        s,
        k,
        &exes,
        current_exe.as_deref(),
        App::choose_exe,
        layer,
    );

    let d = s.detected().unwrap_or_default();
    col()
        .key("options")
        .w_full()
        .pt(6.0)
        .gap(14.0)
        .transition(Transition::offset(0.0, -6.0).combined(Transition::opacity()).springs(FADE))
        .child(text(t.defaults_hint).font(CAPTION).color(MUTED).mt(-10.0))
        .child(field("Proton", proton))
        .child(field(
            t.executable,
            row().gap(8.0).center_y().child(exe).child(
                Button::new(t.browse)
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
                    input(k, "fake-id", t.steam_app_id, &s.form.fake_id, &d.fake_app_id.to_string(), |s| {
                        &mut s.form.fake_id
                    })
                    .grow(1.0),
                )
                .child(input(k, "args", t.arguments, &s.form.args, t.none, |s| &mut s.form.args).grow(1.0)),
        )
        .child(input(k, "overrides", t.dll_overrides, &s.form.overrides, &d.overrides, |s| &mut s.form.overrides))
        .child(input(k, "env", t.environment, &s.form.env, "KEY=value …", |s| &mut s.form.env))
        .child(input(k, "prefix", t.wine_prefix, &s.form.prefix, &d.prefix.to_string_lossy(), |s| &mut s.form.prefix))
}

/// What the kit fills an open Select field with: its hover tint over the page, blended as the
/// window blends them. A field's menu takes the same colour.
fn open_field_fill(k: &Kit) -> Color {
    let (top, bottom) = (k.c.hover, k.c.bg_base);
    let a = top.a + bottom.a * (1.0 - top.a);
    let srgb = rune::theme::blends_srgb();
    let enc = |c: f32| if srgb { rune::theme::srgb_encode(c) } else { c };
    let dec = |c: f32| if srgb { rune::theme::srgb_decode(c) } else { c };
    let mix = |t: f32, b: f32| dec((enc(t) * top.a + enc(b) * bottom.a * (1.0 - top.a)) / a);
    Color { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a }
}

/// A dropdown field: the kit's Select field, opening a menu of our own under its left edge
/// and as wide as it. The kit's menu hung off the field's right end, 160 points wide, and cut
/// long names short (`picker_menu_lines_up_with_its_field`).
struct Picker {
    key: &'static str,
    label: &'static str,
    placeholder: &'static str,
    width: f32,
}

impl Picker {
    fn build(
        self,
        s: &App,
        k: &Kit,
        options: &[String],
        current: Option<&str>,
        choose: fn(&mut App, &str),
        layer: &mut Vec<El>,
    ) -> El {
        let key = self.key;
        let open = s.open_select == Some(key);
        let field = Select::new(key, options)
            .label(self.label)
            .appearance(SelectAppearance::Field)
            .placeholder(self.placeholder)
            .width(self.width)
            .current(current)
            .open(open)
            .trigger(k)
            // The kit's `on_toggle` gets no `Tx`, and opening has to land a cover in flight.
            .on_press_tx(move |s: &mut App, tx: &mut Tx| s.toggle_select(key, tx));
        if open {
            let fill = open_field_fill(k);
            Menu { key, label: self.label, width: self.width, rows: Menu::ROWS, fill }.build(
                k,
                options,
                current,
                choose,
                &|_| None,
                layer,
            );
        }
        field
    }
}

/// A dropdown's menu, under the left edge of the element keyed `{key}-trigger`.
struct Menu {
    key: &'static str,
    label: &'static str,
    width: f32,
    /// How many rows it shows before it scrolls.
    rows: f32,
    /// Its colour: its trigger's (`menus_match_their_triggers`).
    fill: Color,
}

impl Menu {
    /// A dropdown shows six and a half rows at most, so a longer list reads as one that scrolls.
    const ROWS: f32 = 6.5;
    const ROW_H: f32 = 32.0;

    /// `leading` gives what an option shows before its name.
    fn build(
        self,
        k: &Kit,
        options: &[String],
        current: Option<&str>,
        choose: fn(&mut App, &str),
        leading: &dyn Fn(usize) -> Option<El>,
        layer: &mut Vec<El>,
    ) {
        let key = self.key;
        let rows = options.iter().enumerate().map(|(i, option)| {
            let chosen = current == Some(option.as_str());
            let value = option.clone();
            row()
                .key(format!("{key}-option{i}"))
                .role("option")
                .label(option.clone())
                .focusable(true)
                .focus_selected(chosen)
                .focus_highlight(Color::WHITE.alpha(0.07))
                .cursor(Cursor::Pointer)
                .h(Self::ROW_H)
                // The menu's 2 and these 6 put the names under the field's own text.
                .pl(6.0)
                .pr(8.0)
                .gap(8.0)
                .center_y()
                .radius(6.0)
                .on_press(move |s: &mut App| choose(s, &value))
                .children(leading(i))
                .child(text(option.clone()).font(BODY).color(TEXT).truncate().grow(1.0).shrink())
                .child(icon(k.glyph.check_small, 14.0).color(TEXT).opacity(if chosen { 1.0 } else { 0.0 }))
        });
        let max_h = 4.0 + self.rows * (Self::ROW_H + 2.0);
        // Pressing anywhere else closes it, as Escape does (`App::on_key`).
        layer.push(rune_kit::scrim(format!("{key}-scrim"), |s: &mut App| s.open_select = None));
        layer.push(
            col()
                .key(format!("{key}-menu"))
                .w(self.width)
                .min_h(0.0)
                .padding(2.0)
                .radius(10.0)
                .fill(self.fill)
                .border(1.0, k.c.border_base)
                .shadow(10.0, 30.0, Color::BLACK.alpha(0.6))
                .clip_rounded(10.0)
                .anchored_fit(format!("{key}-trigger"), Side::Bottom, Align::Start, 4.0)
                .focus_return(format!("{key}-trigger"))
                .transition(Transition::offset(0.0, -4.0).combined(Transition::opacity()).springs(FADE))
                .child(
                    col()
                        .key(format!("{key}-options"))
                        .role("listbox")
                        .label(self.label)
                        .focus_group()
                        .w_full()
                        .max_h(max_h)
                        .min_h(0.0)
                        .shrink()
                        .gap(2.0)
                        .children(rows)
                        .scroll(ScrollSpec { stick_bottom: false })
                        .scroll_edge_fade(12.0),
                ),
        );
    }
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

/// The Install popup, centred over the window: the game's installer and its online fix, where
/// the game will go (or why it can't), and Install.
fn install_dialog(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(form) = &s.install_form else { return };
    let t = s.t();
    let home = &s.paths.home;
    let chosen = |key: &'static str, name_key: &'static str, exe: &Option<std::path::PathBuf>| {
        // A repack's setup.exe says little without its folder.
        let shown = exe.as_deref().map(|p| match (p.parent(), p.file_name()) {
            (Some(dir), Some(name)) => {
                Path::new(&crate::scan::file_name(dir)).join(name).to_string_lossy().into_owned()
            }
            _ => p.to_string_lossy().into_owned(),
        });
        el().key(key)
            .grow(1.0)
            .min_w(0.0)
            .h(32.0)
            .px(10.0)
            .center_y()
            .radius(6.0)
            .fill(RAISED)
            .border(1.0, BORDER)
            .child(
                text(shown.unwrap_or_else(|| t.not_chosen.to_owned()))
                    .font(BODY)
                    .key(name_key)
                    .color(if exe.is_some() { TEXT } else { FAINT })
                    .truncate(),
            )
    };
    let browse = |key: &'static str, f: fn(&mut App, &mut Tx)| {
        Button::new(t.browse).variant(ButtonVariant::Neutral).on_press_tx(f).build(k).key(key)
    };
    let fix_row =
        row().w_full().gap(8.0).center_y().child(chosen("install-fix", "install-fix-name", &form.fix)).children(
            form.fix.is_some().then(|| {
                IconButton::new(k.glyph.close)
                    .label(t.remove)
                    .help(t.remove)
                    .variant(IconButtonVariant::Ghost)
                    .on_press(|s: &mut App| s.choose_fix(None))
                    .build(k)
                    .key("install-fix-remove")
            }),
        );
    let status = form.plan.as_ref().map(|plan| match plan {
        Ok(plan) => {
            let dest = match plan.dest.strip_prefix(home) {
                Ok(rel) => format!("~/{}", rel.to_string_lossy()),
                Err(_) => plan.dest.to_string_lossy().into_owned(),
            };
            text(fill(t.installs_to, &[&dest])).key("install-status").font(CAPTION).color(MUTED)
        }
        Err(e) => text(e.clone()).key("install-status").font(CAPTION).color(TEXT),
    });
    let ready = matches!(form.plan, Some(Ok(_)));
    let body = col()
        .key("install-form")
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(14.0)
        .child(field(
            t.game_installer,
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(chosen("install-game", "install-game-name", &form.game))
                .child(browse("install-game-browse", App::browse_installer)),
        ))
        .child(field(t.online_fix, fix_row.child(browse("install-fix-browse", App::browse_fix))))
        .children(status)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .mt(4.0)
                .justify_end()
                .child(
                    Button::new(t.cancel)
                        .variant(ButtonVariant::Ghost)
                        .on_press(App::close_install)
                        .build(k)
                        .key("install-cancel"),
                )
                .child(
                    Button::new(t.install)
                        .icon(&DOWNLOAD)
                        .variant(ButtonVariant::Contrast)
                        .disabled(!ready)
                        .on_press(App::confirm_install)
                        .build(k)
                        .key("install-confirm"),
                ),
        );
    Dialog::new("install", t.install_title)
        .open(true)
        .description(t.install_description)
        .width(520.0)
        .fit()
        .on_close(App::close_install)
        .child(body)
        .build(k, layer);
}

/// The install's progress toast, and how tall it is.
const PROGRESS_W: f32 = 440.0;
pub const PROGRESS_H: f32 = 60.0;

/// While a game installs, a toast that stays until it's done: what's being installed, how much
/// it has written so far, a bar, and Cancel. The installer's total is inside its compressed
/// data, so the bar can't fill: a segment slides along it, and the growing size shows it's
/// alive. Other toasts sit above it.
fn install_progress(s: &App, k: &Kit, window_w: f32, bottom: f32) -> Option<El> {
    let (plan, step) = s.installing.as_ref()?;
    let t = s.t();
    let title = match () {
        _ if s.install_cancelled => t.cancelling_install,
        _ if plan.steps[*step].is_fix => t.installing_fix,
        _ => t.installing,
    };
    let title = fill(title, &[&plan.name]);
    let size = s.install_size.map(t.bytes).unwrap_or_default();
    // A quarter of the track crosses it, then comes round from the left again.
    let bar = el()
        .key("install-bar")
        .w_full()
        .h(4.0)
        .radius(2.0)
        .fill(Color::WHITE.alpha(0.1))
        .clip_rounded(2.0)
        .role("progressbar")
        .label(title.clone())
        .child(
            el().key("install-bar-fill")
                .w_frac(0.25)
                .h_full()
                .radius(2.0)
                .fill(TEXT)
                .looping(rune::LoopSpec::slide(1.4, [-1.0, 0.0], [4.0, 0.0])),
        );
    let lines = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(10.0)
        .child(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .child(text(title).key("install-progress-title").font(BODY).color(TEXT).truncate().grow(1.0).shrink())
                .child(text(size).key("install-progress-size").font(CAPTION.features("'tnum' 1")).color(MUTED)),
        )
        .child(bar);
    let cancel = Button::new(t.cancel)
        .variant(ButtonVariant::Neutral)
        .disabled(s.install_cancelled)
        .on_press(App::cancel_install)
        .build(k)
        .key("install-progress-cancel");
    // Even, like the row it's centred in, so it doesn't shimmer as the window resizes.
    let w = (window_w - 2.0 * GUTTER - odd(window_w)).min(PROGRESS_W);
    Some(
        row()
            .key("install-progress-row")
            .pin(Some(GUTTER), None, Some(GUTTER + odd(window_w)), Some(bottom))
            .motion(Spring::new(0.35, 0.1))
            // On the row, which is what goes when the install ends.
            .transition(Transition::offset(0.0, 12.0).combined(Transition::opacity()).springs(FADE))
            .justify_center()
            .child(
                row()
                    .key("install-progress")
                    .w(w)
                    .h(PROGRESS_H)
                    .pl(16.0)
                    .pr(12.0)
                    .gap(16.0)
                    .center_y()
                    .radius(12.0)
                    .fill(RAISED)
                    .border(1.0, BORDER)
                    .shadow(10.0, 30.0, Color::BLACK.alpha(0.6))
                    .child(lines)
                    .child(cancel),
            ),
    )
}

/// A floating message at the bottom of the window, `bottom` points up from its edge: above the
/// search bar and the install's progress while they show.
fn toast(message: &str, seq: u64, window_w: f32, bottom: f32) -> El {
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
