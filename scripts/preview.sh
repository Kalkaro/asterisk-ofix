#!/usr/bin/env bash
# Renders the main screens to PNGs, headless, from your real library and Steam artwork:
#   nix develop -c scripts/preview.sh /some/dir
set -euo pipefail
out=${1:?usage: scripts/preview.sh OUT_DIR}
mkdir -p "$out"
cargo build --quiet
bin=target/debug/asterisk-ofix
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
trap 'rm -f "$hover"' EXIT
echo '{"events": [{"t": 0.1, "type": "move", "x": 326, "y": 200}]}' >"$hover"
snap hover --script "$hover" --at 0.8 --size 1080x720
echo "wrote $(ls "$out" | wc -l) screenshots to $out"
