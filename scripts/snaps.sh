#!/bin/sh
# Renders every scene headlessly (no window, no permissions) and tiles them into one sheet.
#   scripts/snaps.sh [out-dir]
set -e
cd "$(dirname "$0")/.."
OUT="${1:-out/snaps}"
BIN=target/release/pointer
command -v cargo >/dev/null || . "$HOME/.cargo/env"
mkdir -p "$OUT"
cargo build --release --quiet
snap() { "$BIN" --scene "$1" --snap "$OUT/$1.png" --at "$2" --scale "${SCALE:-1}" >/dev/null 2>&1 && echo "  $1 @ $2"; }
snap idle 1.5
snap dark 1.5
snap sweep 1.22
snap tap 1.22
snap charge 1.9
snap crack 1.9
snap spotlight 1.5
snap laser 1.5
snap pen 2.3
snap focus 1.9
snap help 1.5
snap perf 1.5
if command -v ffmpeg >/dev/null; then
  ffmpeg -v error -y \
    -i "$OUT/idle.png" -i "$OUT/dark.png" -i "$OUT/sweep.png" -i "$OUT/tap.png" \
    -i "$OUT/charge.png" -i "$OUT/crack.png" -i "$OUT/spotlight.png" -i "$OUT/laser.png" \
    -i "$OUT/pen.png" -i "$OUT/focus.png" -i "$OUT/help.png" -i "$OUT/perf.png" \
    -filter_complex "xstack=inputs=12:layout=0_0|w0_0|w0+w1_0|0_h0|w0_h0|w0+w1_h0|0_h0+h1|w0_h0+h1|w0+w1_h0+h1|0_h0+h1+h2|w0_h0+h1+h2|w0+w1_h0+h1+h2,scale=2400:-1" \
    "$OUT/sheet.png" && echo "sheet: $OUT/sheet.png"
fi
