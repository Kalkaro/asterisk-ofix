#!/usr/bin/env bash
# Builds dist/asterisk-ofix-x86_64.AppImage. CI runs it on Ubuntu 22.04; its glibc (2.35) is
# the oldest the AppImage runs on, so build on an old distro, not with Nix:
#   scripts/appimage.sh
# Needs Rust (rustup), pkg-config, libxkbcommon-dev, libwayland-dev, curl and file.
#
# Only the binary goes in. It links nothing but glibc: Vulkan, libGL, X11, Wayland,
# xkbcommon and fontconfig are dlopen()ed from the system, and the GPU driver has to be the
# system's anyway.
set -euo pipefail
cd "$(dirname "$0")/.."

toolchain=${RUST_TOOLCHAIN:-1.99.1} # what Rune pins; keep in step with flake.nix
cargo "+$toolchain" build --release --locked
bin=target/release/asterisk-ofix

echo "needs:" && readelf -d "$bin" | sed -n 's/.*NEEDED.*\[\(.*\)\]/  \1/p'
echo "newest glibc symbol: $(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"

app=target/AppDir
rm -rf "$app"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/scalable/apps"
install -m755 "$bin" "$app/usr/bin/asterisk-ofix"
strip "$app/usr/bin/asterisk-ofix"
install -m644 packaging/asterisk-ofix.desktop "$app/usr/share/applications/"
install -m644 packaging/asterisk-ofix.svg "$app/usr/share/icons/hicolor/scalable/apps/"
cp packaging/asterisk-ofix.desktop packaging/asterisk-ofix.svg "$app/"
ln -s asterisk-ofix.svg "$app/.DirIcon"
ln -s usr/bin/asterisk-ofix "$app/AppRun"

# appimagetool's static runtime needs no libfuse2 on the user's system.
tool=target/appimagetool-x86_64.AppImage
if [[ ! -x $tool ]]; then
  # GitHub's downloads sometimes answer 504; v0.1.0's first build died of one.
  curl -fsSL --retry 5 --retry-all-errors -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$tool"
fi
mkdir -p dist
# Extracting instead of mounting works without FUSE (containers, CI).
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream "$app" dist/asterisk-ofix-x86_64.AppImage
ls -lh dist/asterisk-ofix-x86_64.AppImage
