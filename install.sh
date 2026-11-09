#!/usr/bin/env bash
# Installs the latest asterisk-ofix AppImage for this user, with a menu entry:
#   curl -fsSL https://raw.githubusercontent.com/Kalkaro/asterisk-ofix/main/install.sh | bash
# Uninstall (your games' saves in ~/.local/share/asterisk-ofix stay):
#   curl -fsSL https://raw.githubusercontent.com/Kalkaro/asterisk-ofix/main/install.sh | bash -s -- --uninstall
#
# ASTERISK_OFIX_URL overrides where the AppImage comes from (tests use a file:// URL).
set -euo pipefail

# Everything is in main, so a download cut off halfway runs nothing.
main() {
  local url=${ASTERISK_OFIX_URL:-https://github.com/Kalkaro/asterisk-ofix/releases/latest/download/asterisk-ofix-x86_64.AppImage}
  local bin_dir="$HOME/.local/bin"
  local data=${XDG_DATA_HOME:-$HOME/.local/share}
  local app="$bin_dir/asterisk-ofix"
  local desktop="$data/applications/asterisk-ofix.desktop"
  local icon="$data/icons/hicolor/scalable/apps/asterisk-ofix.svg"

  if [[ ${1:-} == --uninstall ]]; then
    rm -f "$app" "$desktop" "$icon"
    refresh_menus "$data"
    say "removed asterisk-ofix. Your Wine prefixes and saves are still in $data/asterisk-ofix."
    return
  fi

  [[ $(uname -m) == x86_64 ]] || die "only x86_64 is built, this is $(uname -m)"
  if grep -qsE '^ID="?nixos"?$' /etc/os-release; then
    die "on NixOS, use the flake instead: nix run github:Kalkaro/asterisk-ofix"
  fi

  # Global, so the EXIT trap still sees it after main returns.
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  say "downloading $url"
  fetch "$url" "$tmp/asterisk-ofix.AppImage"
  chmod +x "$tmp/asterisk-ofix.AppImage"
  # The icon comes out of the AppImage itself: one download.
  (cd "$tmp" && ./asterisk-ofix.AppImage --appimage-extract asterisk-ofix.svg >/dev/null)

  mkdir -p "$bin_dir" "$(dirname "$desktop")" "$(dirname "$icon")"
  mv -f "$tmp/asterisk-ofix.AppImage" "$app"
  install -m644 "$tmp/squashfs-root/asterisk-ofix.svg" "$icon"

  # The AppImage mounts itself with FUSE; without fusermount it can unpack itself instead.
  local exec=$app
  if ! command -v fusermount3 >/dev/null && ! command -v fusermount >/dev/null; then
    exec="env APPIMAGE_EXTRACT_AND_RUN=1 $app"
    say "note: FUSE isn't installed, so it unpacks itself on every start (a bit slower). Install fuse3 to fix that."
  fi
  cat >"$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=asterisk-ofix
GenericName=Game Launcher
Comment=Launch Steam Fix / Online Fix games through Proton with the Steam overlay
Exec=$exec
Icon=asterisk-ofix
Terminal=false
Categories=Game;
StartupWMClass=asterisk-ofix
EOF
  refresh_menus "$data"

  say "installed asterisk-ofix to $app"
  case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) say "note: $bin_dir isn't on your PATH; start it from your app menu, or add it to PATH." ;;
  esac
  if [[ ! -d $HOME/.local/share/Steam && ! -d $HOME/.steam/steam && -d $HOME/.var/app/com.valvesoftware.Steam ]]; then
    say "warning: only Flatpak Steam was found, and asterisk-ofix needs a native Steam install."
  fi
}

fetch() {
  if command -v curl >/dev/null; then
    curl -fL --retry 3 --retry-all-errors --progress-bar -o "$2" "$1"
  elif command -v wget >/dev/null; then
    wget -q --show-progress --tries=3 -O "$2" "$1"
  else
    die "needs curl or wget"
  fi
}

refresh_menus() {
  if command -v update-desktop-database >/dev/null; then update-desktop-database -q "$1/applications" || true; fi
  if command -v gtk-update-icon-cache >/dev/null; then gtk-update-icon-cache -qtf "$1/icons/hicolor" 2>/dev/null || true; fi
}

say() { printf 'asterisk-ofix: %s\n' "$*"; }
die() {
  say "$*" >&2
  exit 1
}

main "$@"
