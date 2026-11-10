# AGENTS.md

asterofix is a Rust app on the [Rune](https://github.com/anomalyco/rune) GUI framework
that launches Steam Fix / Online Fix games directly through Proton with the Steam overlay. See
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
- Real launches: `ASTEROFIX_INTEGRATION=1 nix develop -c cargo test sandbox -- --nocapture`
  (slow; needs Steam's runtime and Proton installed).

## Checking the UI visually

```sh
nix develop -c scripts/preview.sh /some/dir
```

This renders `library.png`, `library-narrow.png`, `hover.png`, `page.png`,
`page-narrow.png`, `options.png`, `proton-menu.png`, `prefix-menu.png`, `prefix-menu-narrow.png`, `old-saves.png`, `old-saves-narrow.png`, `search.png`, `no-matches.png`, `toast.png`, `running.png`,
`language-menu.png`, `install.png`, `install-narrow.png`, `install-progress.png`, `uninstall.png`, `uninstall-narrow.png`, `folders.png`, `folders-narrow.png`, and in other languages `page-ru.png`, `library-ru-narrow.png`, `page-ja.png`, `page-fr.png` and `old-saves-fr.png` headlessly (Rune's
`--snap`, with the scenes in `view::SCENES`) from a made-up demo library:
`scripts/demo_home.py` builds a throwaway home with Pong and Game 1 to 4, black and white
covers drawn like box art (a little scene each, no titles) already in the cache, and a fake Steam with a few Proton builds (Proton 9.0
its default). Screenshots never show
the user's real games; add `--real` only to check something against your own library.
Look at them after any UI change.
Don't launch the app on the user's desktop to take screenshots. In a sandbox without the
GPU, add `VK_ICD_FILENAMES=/run/opengl-driver/share/vulkan/icd.d/lvp_icd.x86_64.json`
(Mesa's software Vulkan).

The README's `docs/preview.png` is `hover.png` from that script, shrunk to 1280 wide. The
logo, `docs/logo.svg`, is drawn from the bundled font's outlines ("asterofix" with the asterisk in
the o); redraw it after a font change:

```sh
nix-shell -p 'python3.withPackages (p: [p.fonttools p.uharfbuzz])' --run \
  'python3 scripts/logo.py fonts/GoogleSansFlex-SemiBold.ttf docs/logo.svg 1280 400 220'
```

## Layout

- `src/ui/mod.rs`: `App`, all UI state and actions (scan, sort, search, open a game's page,
  edit and save settings, play, toasts, file pickers). Work finished on other threads
  (artwork, games exiting) goes into an inbox that `poll` drains on the UI thread. Reload
  finds Steam, its Proton builds and its default again (`detect_steam`), and so does Play
  when no Steam is known. Games and installers start through `App::launch`, with the Steam
  found last; tests' fakes go in `launcher_override`, which nothing else sets.
- `src/ui/view.rs`: the view. The starfield, the header, the card grid, the game page and
  the toast. A single click on a card opens the game's page right away, in the grid's
  place: its cover (which flies there from the card, `matched`), name, Play, Folder, Log,
  Copy command, and the launch options, collapsed until asked for. The options start with a
  Name field that renames the game (`GameSettings.name`; its placeholder is the detected name,
  `App::detected_name`), which re-sorts the library. The game's hero art
  sits faint behind it, fading into the stars. The grid stays mounted under the page,
  hidden and inert, so it comes back scrolled where it was; Back or Escape goes back, and
  typing a search does too. A double click (`on_double_press`), or the play button that
  fades in on hover (`hover_group` + `group_opacity`, no view re-run), starts the game.
  The first click of a double click opens the page, so for `DOUBLE_CLICK` after it the
  card that opened it still takes presses, no other card does, and neither does the page
  (`App::page_opening`, `other_cards_ignore_clicks_while_a_page_opens`). While a game runs,
  a Stop button sits in Play's place on its card, always shown, and the page's Play turns
  into Stop. Stop signals every process carrying `GAME_MARKER` (the game's folder, set in
  its environment at launch), inside Steam's sandbox too (`stop_game`). The search is hidden until you type with no text field focused (`App::on_key`),
  then floats over the bottom of the window; Escape, emptying it, or clicking away while
  it's empty puts it away. Ctrl+F opens it too. The arrows, Home and End move a cursor
  through the grid (`App::move_cursor`, by `App::grid_cols`, which the view sets as it lays
  the grid out), and its card shows as hovered (`group_opacity` from `App::cursor`). Return
  opens its page and Return on the page plays, unless a text field is being typed in
  (`App::editing`); in the search field Up, Down and Return do the same, and Left and Right
  stay the text's. A button the pointer pressed keeps focus, so these ignore it. There is no status bar: messages go
  through `App::set_status`, which shows a toast (above the search bar while it's open).
- `src/ui/theme.rs`: palette, type, the embedded Google Sans Flex font, icons (SVG bodies),
  and the rune-kit `Kit`.
- `src/ui/i18n.rs`: the UI's languages and every string the user reads, one `Strings` per
  language (a missing string doesn't build). Templates number their arguments (`{0}`), since
  word order differs; `fill` puts them in. The language is the system's (`Lang::detect`,
  given the environment so tests never read the real one) until one is picked from the language
  button at the header's end (the globe and the language's code, `Lang::short`), which saves its
  code as `language` in config.json. The menu lists each language in itself after its code, all
  at once; no flags, they were the only colour in the UI. New UI text goes in every language, never as a literal
  in the view (`translations_keep_every_placeholder`, `languages_are_translated`). Errors
  from `launch.rs` and the like stay English: the CLI prints them too.
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
- `src/install.rs`: Install (the header's button, and `--install`). A popup in the middle of
  the window (rune-kit's `Dialog`, `install_dialog` in view.rs) takes the game's installer and
  its online fix, each with Browse (Rune's file dialog picks one file at a time); choosing the
  installer fills in the fix downloaded beside it (`find_fix`), and the popup shows where the
  game will go or why it can't. Install runs them silently
  (Inno Setup `/VERYSILENT /DIR=`, NSIS `/S /D=`) through the same Proton launch as games, into
  `~/Games/asterofix/<name>` and the prefix the game will play in. The name comes from the
  installer's name or its folder's, minus versions, "by <repacker>" and fix words.
  `winebrowser.exe=d` stops installers opening their readme in a browser. Steps chain through
  the inbox (`App::install_step_done`), so the launcher stays on the UI thread. While one runs,
  `install_progress` in view.rs stays at the bottom with what it has written so far
  (`App::measure_install`, a thread polling `install::dir_size`), a sliding bar (the installer's
  total isn't known) and Cancel, which stops the installer with the game `stopper`; other toasts
  sit above it. `~/Games/asterofix` becomes a library, and a library inside
  another is scanned as a library, not taken for a game. `plan` never installs a game into a
  folder that has files, so when the game's installer fails, what it left is this install's
  alone and `Plan::remove_partial` removes it (on a thread, after its processes are gone), and
  installing again just works. It removes only a `fresh` plan's own folder right under
  `~/Games/asterofix`, never the prefix, and a failed fix leaves the game alone. Cancelling
  cleans up the same way.
- Library folders: the header's Add folder opens the Library folders popup (`folders_dialog`
  in view.rs, `App::folders_open`) once `cfg.libraries` has any, and the folder picker while
  it's empty. The popup lists them, each with a button that stops looking there
  (`App::remove_library`: it drops the folder from the config and rescans, deleting nothing and
  keeping games' settings), and has its own Add folder.
- `src/uninstall.rs`: how a game leaves the library, from the last of its page's actions after
  Copy command (an icon unless its label fits, `page_labels`). Which one depends on
  `App::origin`: a game the last scan found in a library folder (`App::from_library`) gets
  Uninstall, one added with Add game (its `.exe` in `manual_exes`) gets Remove from library,
  which only drops its `.exe`s from the config. Uninstall asks first in a popup naming the
  folder (`uninstall_dialog` in view.rs), then deletes it on a thread (`App::deleter`, swapped
  out in tests) and reports through the inbox (`App::uninstall_done`): the page closes, the
  library rescans, a toast says so. `uninstall::check` runs when Uninstall is pressed, when it's
  confirmed and again on the thread just before deleting: a real folder (not a link) directly
  in a library, not a library or one's parent, not holding home or asterofix's own folders or
  the game's Wine prefix, and still a game. It's refused while the game runs or an installer
  writes to it. Deleting never follows links, and per-game settings are kept for either action,
  so a game added or installed again gets them back. Never weaken these checks: a hand-added
  `.exe`'s "game folder" can be ~/Downloads.
- `src/art.rs`: Steam names, covers (600x900) and heroes. It checks our cache and Steam's
  library cache first, then asks the store once for the batch and downloads the images from
  the Steam CDN on `DOWNLOAD_WORKERS` threads, covers first, each stored (and `done`) as it
  lands. An app whose store call or image download failed is asked for again by the next
  scan; one the store has no image for isn't. Art already loaded is only ever added to.
  `art::fake` can fail images, leave apps imageless, slow images down and count them.
- `src/instance.rs`: one window per user. The window holds an flock on `window.lock` in the
  runtime dir (`Paths::runtime_dir`, `$XDG_RUNTIME_DIR/asterofix`) and listens on `window.sock`
  beside it. A later launch finds the lock taken, sends its `XDG_ACTIVATION_TOKEN` down the
  socket, prints a line and exits 0; the window comes forward with that token
  (`AppHandle::activate`) and shows a toast. The kernel drops the lock when the process dies,
  and Rust opens it close-on-exec, so games launched from the window don't hold it
  (`games_never_inherit_the_lock`). Headless runs and the CLI never take it (`ui::run`).
- `src/config.rs`: config.json. The window and `--install` can run at once, each with its own
  copy, so `Config::save` never writes a copy over the file: under `config.json.lock` it reads
  the file again and merges three ways from `base`, the copy last read or written (the
  window's `saved_cfg`), on the JSON, so new fields merge too: objects key by key, lists as
  sets, and the edit being saved wins a clash (`a_save_keeps_what_another_process_saved`,
  `an_install_from_the_cli_survives_the_windows_next_save`). The window also takes in outside
  changes when it gets focus (`App::reload_config`). Change `cfg` and call `App::save`; never
  write config.json any other way.
- `src/scan.rs`, `src/steam.rs`, `src/launch.rs`, `src/vdf.rs`,
  `src/paths.rs`: game detection, Proton/Steam discovery, launching and paths.
- `src/cli.rs`: the command line (`cli::parse`, pure). Unknown flags are errors (exit 2), so
  Rune's own flags (`--snap`, `--scene`, ...) are listed there, copied from Rune's
  `tooling/args.rs`; update the list when bumping Rune.

## AppImage and install.sh

`scripts/appimage.sh` builds `dist/asterofix-x86_64.AppImage`: just the binary, which
links only glibc (everything else is dlopen()ed from the system), plus
`packaging/asterofix.desktop` and the icon `packaging/asterofix.svg` (drawn by
`scripts/icon.py` from the font's asterisk). CI (`.github/workflows/appimage.yml`) builds it
on Ubuntu 22.04, runs `install.sh` and a headless `--snap` in Ubuntu, Debian, Fedora and Arch
containers, and attaches it to the release on a `v*` tag, but only once the tests pass too.
They're `.github/workflows/test.yml`: `cargo clippy --all-targets -- -D warnings` (so keep
clippy clean) and `cargo test`, on Ubuntu 22.04 with the pinned toolchain, on every branch
push and pull request, and called from `appimage.yml` for a tag. The UI tests need no GPU or
Vulkan driver; the job installs DejaVu and Noto CJK, the fonts they fall back to for Cyrillic
and CJK text. `install.sh` is the curl one-liner
in the README: it fetches the release's AppImage into `~/.local/bin` with a menu entry, and
`--uninstall` removes those but never `~/.local/share/asterofix` (the saves). Keep
`RUST_TOOLCHAIN` in `appimage.sh` and both workflows in step with flake.nix. Don't run the
installer on the user's machine; check it in CI. Run `shellcheck` and `actionlint` on changes.

## Gotchas

- Rune's `Color` is **linear** sRGB. Palette constants in `theme.rs` are decoded from the
  sRGB hex in the comment beside them; writing `0x12 as f32 / 255.0` makes greys far too
  light.
- Element keys can't contain `/`: it separates key paths. Card keys escape it in the
  folder's path (`card_key`); a slug gave "A B" and "A_B" one key.
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
  cropped. Keep `MIN_WINDOW` under 600 wide and the header fitting at it, in every language
  (`header_fits_at_every_width`). Where the measured labels don't fit (`header_labels`), the
  buttons show just their icons one at a time: the language button first, then Add folder, then
  Add game, then Install.
  English keeps Add game's and Install's labels at the minimum.
- The bundled font covers Latin scripts only. Cyrillic falls back to the system's DejaVu Sans,
  and Chinese and Japanese to Noto CJK, which Rune finds itself.
- Icons are one colour (they tint like glyphs).
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
- The game page replaces the grid without unmounting it (`game_page_replaces_the_grid`),
  and is placed from the left edge like the header (`game_page_is_anchored_left_and_fits`).
  Its text column has an even width, or the two fields sharing a line shimmer.
- The page's dropdowns are `Picker`s: rune-kit's Select field with a menu of our own, under
  the field's left edge and as wide as it. The kit's menu hangs off the field's right end
  at 160 points and truncates long Proton names (`picker_menu_lines_up_with_its_field`).
  The Wine prefix field stays a text field (a path can be typed or pasted) with a chevron at
  its end that opens the same kind of menu, listing the prefixes there are, read when the page
  or the menu opens (`prefix_menu_lists_the_prefixes_there_are`).
- A `matched` element in flight draws above the whole window at its own opacity, ignoring
  its ancestors'. So no card in the hidden grid shows its cover while a page is open: a
  cover flying home when another page opened sat on top of it
  (`a_cover_flying_home_never_shows_over_another_page`).
- The Wine prefixes in `~/.local/share/asterofix/prefixes/` hold the games' saves.
  Never move or delete them, and keep `default_prefix` naming them the same way: after the
  game's folder, plus a hash of its path only when another live game with that folder name
  owns the plain one (`.asterofix-game` in the prefix, written on launch by
  `claim_default_prefix`; `games_in_same_named_folders_get_their_own_prefixes`).
- Fonts in `fonts/` are static instances cut from the variable Google Sans Flex (OFL,
  `ofl/googlesansflex` in github.com/google/fonts), fully rounded: the user asked for the
  roundness axis (`embedded_fonts_are_rounded`). To regenerate them:
  `fonttools varLib.instancer 'GoogleSansFlex[ROND,opsz,slnt,wdth,wght].ttf' wght=400 wdth=100 opsz=drop slnt=0 ROND=100 --static -o fonts/GoogleSansFlex-Regular.ttf`
  (and `wght=600` for SemiBold). Static cuts round every text, rune-kit's controls
  included; per-style `TextStyle::variations` would miss the kit's own styles.
- `flake.nix` builds from an explicit fileset. New non-Rust files that get embedded, like
  the fonts, must be added to it. `packaging/` is in it so the package installs the app
  menu entry and icon (`nix flake check` builds `checks.desktop-entry`, which validates it).
- Rune is pinned to a `dev` rev in Cargo.toml, with the toolchain it pins (rust-toolchain
  in Rune's repo) in flake.nix. Bump them together, and Rune's `outputHashes` hash in
  flake.nix too (set it to `pkgs.lib.fakeHash`, and `nix build .#default.cargoDeps` prints
  the right one). Without that hash nixpkgs clones Rune with every branch and pull request. Rune's docs are its
  `crates/rune/README.md` and `crates/rune-kit/README.md`.
- Games get Steam's environment, which is the user's whole session. Keep secrets out with
  `is_secret_env`: games are untrusted Windows binaries (`secrets_never_reach_the_game`).
  Logs and "Copy command" never print inherited values:
  `LaunchCommand::shell_line` writes them as `KEY="$KEY"` (only `DISPLAY`,
  `WAYLAND_DISPLAY` and `XAUTHORITY` keep theirs), because the name check misses secrets
  and those get pasted into bug reports (`printed_command_shows_no_inherited_values`). Logs
  are 0600 in a 0700 dir, and secret-looking values of our own are redacted (`open_log`,
  `log_is_private_and_redacted`).
- The user prefers fast builds. Dependencies build optimised once; our crate builds at
  opt-level 1 with mold, so an incremental build takes seconds.
