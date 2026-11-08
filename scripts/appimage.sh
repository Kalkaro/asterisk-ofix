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

# appimagetool 1.9.1, whose static runtime needs no libfuse2 on the user's system. Pinned and
# checked against GitHub's sha256 of it. It comes through GitHub's API: the plain
# github.com/.../releases/download links answered 504 for every repo when v0.1.0 was built.
tool=target/appimagetool-x86_64.AppImage
tool_url=https://api.github.com/repos/AppImage/appimagetool/releases/assets/324406736
tool_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
if ! echo "$tool_sha256  $tool" | sha256sum -c --status 2>/dev/null; then
  auth=()
  # CI's token, so runners sharing an IP don't hit the anonymous API limit.
  if [[ -n ${GITHUB_TOKEN:-} ]]; then auth=(-H "Authorization: Bearer $GITHUB_TOKEN"); fi
  curl -fsSL --retry 5 --retry-all-errors -H "Accept: application/octet-stream" "${auth[@]}" -o "$tool" "$tool_url"
  echo "$tool_sha256  $tool" | sha256sum -c -
  chmod +x "$tool"
fi
mkdir -p dist
# Extracting instead of mounting works without FUSE (containers, CI).
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream "$app" dist/asterisk-ofix-x86_64.AppImage
ls -lh dist/asterisk-ofix-x86_64.AppImage
