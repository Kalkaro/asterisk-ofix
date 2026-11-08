# asterisk-ofix

Launches OnlineFix / SteamFix games directly through Proton with the Steam overlay
(Shift+Tab) attached. Nothing is added to your Steam library. Written in Rust with
[Rune](https://github.com/anomalyco/rune).

## How it launches

For the selected game it does what Steam does for a Proton title:

- runs `<proton>/proton waitforexitandrun game.exe` inside the Steam Linux Runtime the
  Proton build asks for (from its `toolmanifest.vdf`). It joins the running Steam's
  sandbox with `nsenter` when Steam runs in one (NixOS), else wraps it in `steam-run` there
- preloads Steam's `gameoverlayrenderer.so` and enables the Vulkan overlay layer
  (`ENABLE_VK_LAYER_VALVE_steam_overlay_1=1`)
- sets `SteamAppId` / `SteamGameId` / `SteamOverlayGameId` to the fake app ID from
  `OnlineFix.ini` / `SteamFix.ini` (usually 480)
- sets `WINEDLLOVERRIDES` for the OnlineFix DLLs found next to the exe
- uses its own Wine prefix per game in `~/.local/share/asterisk-ofix/prefixes/`

Steam must be running and logged in.

Games are found by scanning library folders (default `~/Games`) for `OnlineFix*.dll/ini` or
`SteamFix*.dll/ini`. Settings live in `~/.config/asterisk-ofix/config.json`; logs in
`~/.local/share/asterisk-ofix/logs/`.

## Using it

Click a game to open its panel (Proton build, executable, launch options). Double-click it,
or press the play button that appears under the pointer, to start it. `Ctrl+F` searches,
`Esc` closes the panel.

## Build / run

```sh
nix develop -c cargo run                  # dev build
nix build && ./result/bin/asterisk-ofix   # package
nix run .
```

## Tests

```sh
nix develop -c cargo test                                          # everything, headless
nix develop -c cargo test --release perf -- --ignored --nocapture  # frame costs
ASTERISK_OFIX_INTEGRATION=1 nix develop -c cargo test sandbox -- --nocapture  # a real launch
```

The integration test starts a fake "Steam" sandbox with `steam-run` (private `/tmp`, like
NixOS Steam), launches Wine's `cmd.exe` through the real path (nsenter → env → Steam Linux
Runtime → Proton) and checks that it runs, can see the sandbox's `/tmp`, has
`gameoverlayrenderer.so` loaded and the overlay env set. Its Wine prefix is cached in
`~/.cache/asterisk-ofix-test/prefix`.

## CLI

```sh
asterisk-ofix --launch ~/Games/SomeGame          # launch without the window, wait for it
asterisk-ofix --launch path/to/game.exe --print  # print the full command instead
```
