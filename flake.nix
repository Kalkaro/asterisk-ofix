{
  description = "asterisk-ofix - launch Steam Fix / Online Fix games through Proton with the Steam overlay";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAll =
        f:
        nixpkgs.lib.genAttrs systems (
          s:
          f (
            import nixpkgs {
              system = s;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
      # Rune pins this toolchain (its rust-toolchain.toml).
      toolchain =
        pkgs:
        pkgs.rust-bin.stable."1.99.1".default.override {
          extensions = [
            "rust-src"
            "clippy"
            "rustfmt"
          ];
        };
      # Linked at build time (xkbcommon for layer-shell, the Wayland client).
      buildLibs =
        pkgs: with pkgs; [
          libxkbcommon
          wayland
        ];
      # dlopen()ed at run time by winit and wgpu.
      runtimeLibs =
        pkgs: with pkgs; [
          libGL
          libxkbcommon
          vulkan-loader
          wayland
          libx11
          libxcursor
          libxi
          libxrandr
        ];
    in
    {
      packages = forAll (
        pkgs:
        let
          rust = toolchain pkgs;
          platform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };
        in
        {
          default = platform.buildRustPackage {
            pname = "asterisk-ofix";
            version = "0.1.1";
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./src
                ./fonts
                ./packaging
              ];
            };
            cargoLock = {
              lockFile = ./Cargo.lock;
              # Rune's commit, fetched alone. Without a hash nixpkgs falls back to
              # builtins.fetchGit with allRefs, which clones every branch and pull request
              # (160 MB). Update it with the rev in Cargo.toml.
              outputHashes."rune-0.0.1" = "sha256-sdskmmer1AdrlLLKN4ga7D/W02D01lztof5UloWCwUY=";
            };
            # Tests need a writable HOME and some spawn processes; `nix develop -c cargo test` runs them.
            doCheck = false;
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.makeWrapper
            ];
            buildInputs = buildLibs pkgs;
            # The app menu entry and its icon, as the AppImage carries them.
            postInstall = ''
              install -Dm644 packaging/asterisk-ofix.desktop -t $out/share/applications
              install -Dm644 packaging/asterisk-ofix.svg -t $out/share/icons/hicolor/scalable/apps
            '';
            postFixup = ''
              wrapProgram $out/bin/asterisk-ofix \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}
            '';
            meta.mainProgram = "asterisk-ofix";
          };
        }
      );

      # The package shows up in app menus: a valid desktop entry whose command and icon it ships.
      checks = forAll (pkgs: {
        desktop-entry =
          let
            app = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          in
          pkgs.runCommand "asterisk-ofix-desktop-entry" { nativeBuildInputs = [ pkgs.desktop-file-utils ]; } ''
            entry=${app}/share/applications/asterisk-ofix.desktop
            desktop-file-validate "$entry"
            grep -qx 'Exec=asterisk-ofix' "$entry"
            test -x ${app}/bin/asterisk-ofix
            grep -qx 'Icon=asterisk-ofix' "$entry"
            test -s ${app}/share/icons/hicolor/scalable/apps/asterisk-ofix.svg
            touch $out
          '';
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (toolchain pkgs)
            pkgs.pkg-config
            pkgs.mold
          ]
          ++ buildLibs pkgs;
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibs pkgs);
          # mold links the (large) binary several times faster than ld.
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
          # Interactive `nix develop` drops into fish; `nix develop -c ...` is untouched.
          shellHook = ''
            if [[ $- == *i* && -z "$ASTERISK_OFIX_IN_FISH" ]] && command -v fish >/dev/null; then
              export ASTERISK_OFIX_IN_FISH=1
              exec fish
            fi
          '';
        };
      });
    };
}
