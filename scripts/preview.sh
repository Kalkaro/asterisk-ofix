#!/usr/bin/env bash
# Renders the main screens to PNGs, headless, from a made-up demo library (no real games):
#   nix develop -c scripts/preview.sh /some/dir
# With --real, from your own library and Steam artwork instead.
set -euo pipefail
out=${1:?usage: scripts/preview.sh OUT_DIR [--real]}
mkdir -p "$out"
cargo build --quiet
bin=$PWD/target/debug/asterisk-ofix

if [[ ${2:-} != --real ]]; then
  demo=$(mktemp -d)
  trap 'rm -rf "$demo"' EXIT
  nix-shell -p 'python3.withPackages (p: [p.pillow])' --run \
    "python3 -I scripts/demo_home.py '$demo' fonts/GoogleSansFlex-SemiBold.ttf" >/dev/null
  export HOME=$demo
  unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME
fi
snap() { "$bin" --snap "$out/$1.png" "${@:2}" >/dev/null; }

snap library --size 1080x720
snap library-narrow --size 600x600
snap panel --scene panel --size 1280x760
snap panel-narrow --scene panel --size 760x600
snap search --scene search --size 1080x720
snap no-matches --scene no-matches --size 1080x720
snap toast --scene toast --size 1080x720
# The pointer over the second card.
hover=$(mktemp --suffix .json)
echo '{"events": [{"t": 0.1, "type": "move", "x": 326, "y": 200}]}' >"$hover"
snap hover --script "$hover" --at 0.8 --size 1080x720
rm -f "$hover"
echo "wrote $(ls "$out" | wc -l) screenshots to $out"
