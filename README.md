<p align="center"><img src="docs/logo.svg" alt="aster*sk" width="640"></p>

# asterisk-ofix

A Linux launcher for OnlineFix games. It runs them with Proton, the way Steam would, so the
Steam overlay (Shift+Tab) and online play work, but nothing gets added to your Steam library.

![The library, with the pointer over a game](docs/preview.png)

## Installing

Arch, Fedora, Ubuntu and most other x86_64 distros (glibc 2.35 or newer), run:

```sh
curl -fsSL https://raw.githubusercontent.com/Kalkaro/asterisk-ofix/main/install.sh | bash
```

NixOS, run:

```sh
nix run github:Kalkaro/asterisk-ofix
```

Run the install again to update. To remove it (your saves stay):

```sh
curl -fsSL https://raw.githubusercontent.com/Kalkaro/asterisk-ofix/main/install.sh | bash -s -- --uninstall
```

It needs a native Steam install; Flatpak and Snap Steam aren't supported.

## Using it

Put your games in `~/Games`, or add another folder with **Add folder**. Any folder with
OnlineFix's files in it (`OnlineFix*.dll`/`.ini` or `SteamFix*.dll`/`.ini`) shows up as a
game. **Add game** takes an `.exe` directly.

- Click a game to see its settings, click it again to close them.
- Double-click a game, or hit the play button on it, to start it.
- Type anywhere to search. Esc closes the search, then the settings.

Steam has to be running and logged in. If it isn't, Play starts it; press Play again once
you're logged in.

## What it does when you press Play

- Runs the game with the Proton build you picked (your Steam default otherwise), inside the
  Steam Linux Runtime that Proton build asks for. On NixOS it runs inside Steam's own sandbox
  (or under `steam-run`), because Proton needs a normal Linux file layout.
- Loads Steam's overlay into the game.
- Sets the Steam app ID from the game's `OnlineFix.ini` / `SteamFix.ini` (usually 480).
- Tells Wine to use the OnlineFix DLLs that sit next to the game.
- Gives every game its own Wine prefix, in `~/.local/share/asterisk-ofix/prefixes/`. Your
  saves live there.

Settings are in `~/.config/asterisk-ofix/config.json`, and each game's last log is in
`~/.local/share/asterisk-ofix/logs/`.

## Building

The AppImage is built by `.github/workflows/appimage.yml` when a `v*` tag is pushed
(`scripts/appimage.sh`, on Ubuntu 22.04 so it runs on older glibc). Otherwise you need Nix
with flakes.

```sh
nix run .                                 # build and start it
nix build && ./result/bin/asterisk-ofix   # just build it
nix develop -c cargo run                  # quick dev build
```

There's also a command line, for scripts:

```sh
asterisk-ofix --launch ~/Games/SomeGame          # start a game and wait for it to exit
asterisk-ofix --launch path/to/game.exe --print  # show the command instead of running it
```

## Tests

```sh
nix develop -c cargo test
```

These run without a display and never touch your real files or start a game. One extra test
does a real launch: it runs Wine's `cmd.exe` through the whole path inside a fake Steam
sandbox and checks that it can see the sandbox and has the overlay loaded. It's slow and
needs Steam and Proton installed:

```sh
ASTERISK_OFIX_INTEGRATION=1 nix develop -c cargo test sandbox -- --nocapture
```
