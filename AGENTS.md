# AGENTS.md

asterisk-ofix is a Rust app on the [Rune](https://github.com/anomalyco/rune) GUI framework
that launches OnlineFix / SteamFix games directly through Proton with the Steam overlay. See
README.md for how launching works.

## Commit after every finished change

**When a whole change is done (tests pass, screenshots checked), commit it.** One commit per
finished change, with a message that says what changed and why. Don't batch unrelated work
into one commit, and don't commit half-done work. Don't push unless asked.

## Tests are mandatory

**Every change ships with tests. Don't forget.**

- New behaviour gets a test, fixed bugs get a regression test that fails without the fix,
  and existing tests are updated when behaviour changes on purpose. Check that a new test
  fails when you break the code it covers.
- Run these before you call anything done:

  ```sh
  nix develop -c cargo clippy --all-targets
  nix develop -c cargo test
  ```

- Tests run in parallel threads that share one environment, so never set `HOME` or other
  env vars in a test. Everything that touches the user's files takes a `Paths`
  (`Paths::under(tempdir)` in tests), the Steam store's URLs are `Endpoints`, and launch
  commands take the host environment as an argument (`Steam::command_from`).
  `testutil::tempdir()` and `add_game` build a temp library. `art::fake::start` serves the
  Steam store API and artwork from a local socket instead of the network.
- UI tests (`src/ui/tests.rs`) drive the real view on Rune's headless `Runtime`: `Ui::new`,
  `ui.click(key)`, `ui.type_text`, `ui.key`, `ui.resize`, `ui.settle()`. Look things up by
  element key (`ui.find`, `ui.rect("card-x/play")`). Use `FakeLaunch::install` to intercept
  game launches; never start real games in unit tests.
- `nix develop -c cargo test --release perf -- --ignored --nocapture` measures one relayout
  of 40 games at a new width (about 1 ms) and one backdrop frame (about 0.5 ms at 1280x768,
  drawn at 15 fps off the UI thread). Keep both well under a frame.
- Real launches: `ASTERISK_OFIX_INTEGRATION=1 nix develop -c cargo test sandbox -- --nocapture`
  (slow; needs Steam's runtime and Proton installed).

## Checking the UI visually

```sh
nix develop -c scripts/preview.sh /some/dir
```

This renders `library.png`, `library-narrow.png`, `hover.png`, `panel.png`,
`panel-narrow.png`, `search.png`, `no-matches.png` and `toast.png` headlessly (Rune's
`--snap`, with the scenes in `view::SCENES`) from a made-up demo library:
`scripts/demo_home.py` builds a throwaway home with Pong and Game 1 to 4, covers of white
shapes on black already in the cache, and a fake Steam with one Proton build. Screenshots never show
the user's real games; add `--real` only to check something against your own library.
Look at them after any UI change.
Don't launch the app on the user's desktop to take screenshots. In a sandbox without the
GPU, add `VK_ICD_FILENAMES=/run/opengl-driver/share/vulkan/icd.d/lvp_icd.x86_64.json`
(Mesa's software Vulkan).

The README's `docs/preview.png` is `hover.png` from that script, shrunk to 1280 wide. The
logo, `docs/logo.svg`, is drawn from the bundled font's outlines (the asterisk centred on
the lowercase letters); redraw it after a font change:

```sh
nix-shell -p 'python3.withPackages (p: [p.fonttools p.uharfbuzz])' --run \
  'python3 scripts/logo.py fonts/GoogleSansFlex-SemiBold.ttf docs/logo.svg 1280 400 220'
```

## Layout

- `src/ui/mod.rs`: `App`, all UI state and actions (scan, sort, search, open the panel,
  edit and save settings, play, toasts, file pickers). Work finished on other threads
  (artwork, games exiting) goes into an inbox that `poll` drains on the UI thread.
- `src/ui/view.rs`: the view. The starfield, the header, the card grid, the side panel and
  the toast. A single click on a card opens its panel right away. A double click
  (`on_double_press`), or the play button that fades in on hover (`hover_group` +
  `group_opacity`, no view re-run), starts the game. The panel floats over the grid below
  the header (pinned in `main`); clicking the open game's card again closes it, as does
  Escape. The search is hidden until you type with no text field focused (`App::on_key`),
  then floats over the bottom of the window; Escape, emptying it, or clicking away while
  it's empty puts it away. Ctrl+F opens it too. There is no status bar: messages go
  through `App::set_status`, which shows a toast (above the search bar while it's open).
- `src/ui/theme.rs`: palette, type, the embedded Google Sans Flex font, icons (SVG bodies),
  and the rune-kit `Kit`.
- `src/background.rs`: the animated backdrop, ASCII stars on pure black, black and white
  only. A few big five-pointed stars are filled with characters chosen by cell coverage
  (`sd_star5`), and small sparkles look like `-*-` or `\*/`. Glyphs are X11's 7x13 "fixed"
  font as bitmaps. They drift, turn slightly and twinkle. Frames render on a thread at
  15 fps into a `rune::Pixels`, at one pixel per point, so the ASCII is the same size on
  HiDPI screens. The image is sized in 256 pt chunks larger than the window (`chunked`),
  so a live resize rarely changes its size. The clock stops while the window is unfocused,
  so it resumes without jumping. `.blur(BG_BLUR)` softens it; the UI above stays sharp
  (`backdrop_is_blurred_behind_a_sharp_ui`). Keep it sparse: the user asked for something
  restrained.
- `src/art.rs`: Steam names, covers (600x900) and heroes. It checks our cache and Steam's
  library cache first, then the Steam CDN.
- `src/scan.rs`, `src/steam.rs`, `src/launch.rs`, `src/vdf.rs`, `src/config.rs`,
  `src/paths.rs`: game detection, Proton/Steam discovery, launching, config and paths.

## Gotchas

- Rune's `Color` is **linear** sRGB. Palette constants in `theme.rs` are decoded from the
  sRGB hex in the comment beside them; writing `0x12 as f32 / 255.0` makes greys far too
  light.
- Element keys can't contain `/`: it separates key paths. Card keys slug the folder
  (`card_key`).
- A card's middle is its play button. A test that clicks a card's centre after hovering
  it presses Play, so `Ui::click` clicks cards near their corner.
- Rune snaps what it paints to device pixels, but layout rects stay fractional. Content
  centred in a box whose width flips between odd and even moves by half a pixel as the
  window resizes, and its text shimmers. Centre things in an even-width box (`odd()` in
  view.rs); `nothing_shimmers_while_resizing` guards it.
- During a live resize the compositor shows the last frame pinned to the window's left
  edge until the next one lands, so anything placed from the right edge trails the edge
  and jitters (the layout itself is exact on every frame). Keep the header anchored left
  (`header_is_anchored_left`). Centred floating things (search bar, toast) are the
  exception, and only show briefly.
- Hyprland tiles windows smaller than the minimum size, and then the right edge is
  cropped. Keep `MIN_WINDOW` under 600 wide and the header fitting at it
  (`header_fits_at_every_width`).
- Rune fades out removed elements by default, but in about 0.1 s, which reads as popping.
  Things that come and go (cards a search hides, the search bar, the toast) use the `FADE`
  springs, and the element that's actually removed carries the transition: a transition
  on a child of a removed row never runs (`filtered_cards_fade_out_and_back`,
  `search_bar_fades_out`).
- A text field's edits are applied with no animation, and so is everything they change,
  unless the field has `animate_edit_layout()`. The search field has it, or cards a
  keystroke filters out (and the bar, when Backspace empties it) vanish. Tests have to type
  into the focused field to see this: the first key reaches `App::on_key`, the rest the
  field.
- A focused text field takes Escape to drop its focus, so a field that should close on
  Escape handles it in its own `on_key` (the search bar does).
- Cards are fixed-size (`CARD_W`) and only reflow in a wrapping row. Stretching them made
  live resizes jitter (`cards_keep_their_size_while_resizing`).
- The side panel always floats. Opening it must never move a card, and it must not cover
  the header (`panel_floats_over_the_grid`).
- The Wine prefixes in `~/.local/share/asterisk-ofix/prefixes/` hold the games' saves.
  Never move or delete them, and keep `default_prefix` naming them the same way.
- Fonts in `fonts/` are static instances cut from the variable Google Sans Flex (OFL,
  `ofl/googlesansflex` in github.com/google/fonts), fully rounded: the user asked for the
  roundness axis (`embedded_fonts_are_rounded`). To regenerate them:
  `fonttools varLib.instancer 'GoogleSansFlex[ROND,opsz,slnt,wdth,wght].ttf' wght=400 wdth=100 opsz=drop slnt=0 ROND=100 --static -o fonts/GoogleSansFlex-Regular.ttf`
  (and `wght=600` for SemiBold). Static cuts round every text, rune-kit's controls
  included; per-style `TextStyle::variations` would miss the kit's own styles.
- `flake.nix` builds from an explicit fileset. New non-Rust files that get embedded, like
  the fonts, must be added to it.
- Rune is pinned to a `dev` rev in Cargo.toml, with the toolchain it pins (rust-toolchain
  in Rune's repo) in flake.nix. Bump them together. Rune's docs are its
  `crates/rune/README.md` and `crates/rune-kit/README.md`.
- Games get Steam's environment, which is the user's whole session. Keep secrets out with
  `is_secret_env`: games are untrusted Windows binaries, and logs and "Copy command" print
  that environment. Logs are 0600 in a 0700 dir, and secret-looking values are redacted
  (`open_log`). This leaked a real API key once (`secrets_never_reach_the_game`,
  `log_is_private_and_redacted`).
- The user prefers fast builds. Dependencies build optimised once; our crate builds at
  opt-level 1 with mold, so an incremental build takes seconds.
