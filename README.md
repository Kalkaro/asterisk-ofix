<p align="center"><img src="docs/logo.svg" alt="aster*sk" width="640"></p>

<h1 align="center">Run games with OnlineFix on Linux</h1>

A Linux launcher for OnlineFix games. It runs them with Proton, the way Steam would, so the
Steam overlay (Shift+Tab) and online play work, but nothing gets added to your Steam library.

![The library, with the pointer over a game](docs/preview.png)

## Install

Arch, Fedora, Ubuntu and most other x86_64 distros (glibc 2.35 or newer)

```sh
curl -fsSL https://raw.githubusercontent.com/Kalkaro/asterisk-ofix/main/install.sh | bash
```

NixOS

```sh
nix run github:Kalkaro/asterisk-ofix
```

Run the install again to update. To uninstall, change `bash` to `bash -s -- --uninstall`
(your saves stay).

It needs a native Steam install; Flatpak and Snap Steam aren't supported.

## Usage

Put your games in `~/Games`, or add another folder with **Add folder**. Any folder with
OnlineFix's files in it (`OnlineFix*.dll`/`.ini` or `SteamFix*.dll`/`.ini`) shows up as a
game. **Add game** takes an `.exe` directly.

Steam has to be running and logged in. If it isn't, Play starts it; press Play again once
you're logged in.

Every game gets its own Wine prefix, in `~/.local/share/asterisk-ofix/prefixes/`. Your saves
live there.

Settings are in `~/.config/asterisk-ofix/config.json`, and each game's last log is in
`~/.local/share/asterisk-ofix/logs/`.

There's also a command line, for scripts:

```sh
asterisk-ofix --launch ~/Games/SomeGame          # start a game and wait for it to exit
asterisk-ofix --launch path/to/game.exe --print  # show the command instead of running it
```

## Building

```sh
nix run .                                 # build and start it
nix build && ./result/bin/asterisk-ofix   # just build it
nix develop -c cargo run                  # quick dev build
```

Without Nix, use cargo with Rust 1.99.1, pkg-config, and the libxkbcommon and Wayland dev
packages:

```sh
cargo +1.99.1 build --release   # ./target/release/asterisk-ofix
```
