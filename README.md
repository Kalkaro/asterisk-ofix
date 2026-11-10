<p align="center"><img src="docs/logo.svg" alt="asterofix" width="640"></p>

<h1 align="center">Run games with a Steam Fix / Online Fix on Linux</h1>

![The library, with the pointer over a game](docs/preview.png)

## Install

Arch, Fedora, Ubuntu and most other x86_64 distros (glibc 2.35 or newer)

```sh
curl -fsSL https://raw.githubusercontent.com/Kalkaro/asterofix/main/install.sh | bash
```

Run the install again to update. To uninstall, change `bash` to `bash -s -- --uninstall`
(your saves stay).

NixOS: add it as an input to your flake

```nix
inputs.asterofix.url = "github:Kalkaro/asterofix";
```

and put it in your packages:

```nix
environment.systemPackages = [ inputs.asterofix.packages.${pkgs.stdenv.hostPlatform.system}.default ];
```

To download it prebuilt instead of compiling it, add its cache (and rebuild once before adding
the package), and don't make its `nixpkgs` follow yours:

```nix
nix.settings = {
  extra-substituters = [ "https://asterofix.cachix.org" ];
  extra-trusted-public-keys = [ "asterofix.cachix.org-1:eSwsKEg8DvMbePlRof4JgPYVxjrv+sBbG11Evomy008=" ];
};
```

or run it directly without installing:

```sh
nix run github:Kalkaro/asterofix
```

It needs a native Steam install; Flatpak and Snap Steam aren't supported.

## Usage

Put your games in `~/Games`, or add another folder with **Add folder**. Any folder with a
Steam Fix / Online Fix in it (`SteamFix*.dll`/`.ini` or `OnlineFix*.dll`/`.ini`) shows up as a
game. **Add game** takes an `.exe` directly.

**Install** installs a game for you: choose the game's installer (a repack's `setup.exe`) and
its online fix in the popup. A fix downloaded next to the installer's folder is filled in for
you; it's optional. Both run silently through Proton, the game first and the fix after, into
`~/Games/asterofix/<game>`. Inno Setup and NSIS installers work. If the game's installer
fails, what it left there is removed, so you can just install again; the game's prefix (and
any saves in it) stays.

Click a game to open its page: Play, its folder and log, and its launch options (Proton, the
`.exe`, arguments and the like). The launch options' **Name** renames the game; clear it to go
back to the detected name.

The last button beside Play takes the game out of the library. For a game found in one of your
library folders it's **Uninstall**: after asking, it deletes the game's folder. Its saves, in
its Wine prefix, are kept. For a game added with **Add game** it's **Remove from library**,
which only forgets the `.exe`; no files are touched. Either way its launch options are kept, so
adding or installing it again brings them back.

Steam has to be running and logged in. If it isn't, Play starts it; press Play again once
you're logged in.

Every game gets its own Wine prefix, in `~/.local/share/asterofix/prefixes/`. Your saves
live there.

The app speaks English, Russian, Ukrainian, German, French, Spanish, Brazilian Portuguese,
Polish, Turkish, Simplified Chinese and Japanese. It follows your system's language; the language
button at the end of the header picks another.

Settings are in `~/.config/asterofix/config.json`, and each game's last log is in
`~/.local/share/asterofix/logs/`.

There's also a command line, for scripts:

```sh
asterofix --launch ~/Games/SomeGame          # start a game and wait for it to exit
asterofix --launch path/to/game.exe --print  # show the command instead of running it
asterofix --install 'Game v1.0 by Someone/setup.exe' Game-Fix.exe  # install both silently
asterofix --version
```

`asterofix --help` lists the options.

## Building

```sh
nix run .                                 # build and start it
nix build && ./result/bin/asterofix   # just build it
nix develop -c cargo run                  # quick dev build
```

Without Nix, use cargo with Rust 1.99.1, pkg-config, and the libxkbcommon and Wayland dev
packages:

```sh
cargo +1.99.1 build --release   # ./target/release/asterofix
```
