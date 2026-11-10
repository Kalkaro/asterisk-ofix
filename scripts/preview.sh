#!/usr/bin/env bash
# Renders the main screens to PNGs, headless, from a made-up demo library (no real games):
#   nix develop -c scripts/preview.sh /some/dir
# With --real, from your own library and Steam artwork instead.
set -euo pipefail
out=${1:?usage: scripts/preview.sh OUT_DIR [--real]}
mkdir -p "$out"
cargo build --quiet
bin=$PWD/target/debug/asterofix

if [[ ${2:-} != --real ]]; then
  demo=$(mktemp -d)
  trap 'rm -rf "$demo"' EXIT
  nix-shell -p 'python3.withPackages (p: [p.pillow])' --run \
    "python3 -I scripts/demo_home.py '$demo'" >/dev/null
  export HOME=$demo
  unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME
fi
# English, whatever this machine's locale; the -ru, -ja and -fr shots set their own.
unset LC_ALL LC_MESSAGES LANGUAGE
export LANG=C.UTF-8
snap() { "$bin" --snap "$out/$1.png" "${@:2}" >/dev/null; }

snap library --size 1080x720
snap library-narrow --size 600x600
snap page --scene page --size 1280x760
snap page-narrow --scene page --size 600x600
snap options --scene options --size 1080x720
snap proton-menu --scene proton-menu --size 1080x720
snap prefix-menu --scene prefix-menu --size 1080x720
snap prefix-menu-narrow --scene prefix-menu --size 600x960
snap old-saves --scene old-saves --size 1080x720
snap old-saves-narrow --scene old-saves --size 600x600
LANG=fr_FR.UTF-8 snap old-saves-fr --scene old-saves --size 1080x720
snap search --scene search --size 1080x720
snap no-matches --scene no-matches --size 1080x720
snap toast --scene toast --size 1080x720
snap running --scene running --size 1280x760
snap language-menu --scene language-menu --size 1080x720
snap install --scene install --size 1080x720
snap install-narrow --scene install --size 540x600
snap install-progress --scene install-progress --size 1080x720
snap uninstall --scene uninstall --size 1080x720
snap uninstall-narrow --scene uninstall --size 540x600
snap folders --scene folders --size 1080x720
snap folders-narrow --scene folders --size 540x600
LANG=ru_RU.UTF-8 snap page-ru --scene options --size 1080x720
LANG=ru_RU.UTF-8 snap library-ru-narrow --size 600x600
LANG=ja_JP.UTF-8 snap page-ja --scene page --size 1280x760
# French has the longest labels beside Play.
LANG=fr_FR.UTF-8 snap page-fr --scene page --size 1080x720
# The pointer over the second card.
hover=$(mktemp --suffix .json)
echo '{"events": [{"t": 0.1, "type": "move", "x": 326, "y": 200}]}' >"$hover"
snap hover --script "$hover" --at 0.8 --size 1080x720
rm -f "$hover"
echo "wrote $(ls "$out" | wc -l) screenshots to $out"
