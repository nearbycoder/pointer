#!/bin/sh
# Renders a scripted run headlessly, with its sound effects, into one video.
#   scripts/demo.sh [out.mp4] [scale] [fps] [scene]
#   defaults: out/demo.mp4, 1 (1920×1080), 60, demo (the tools tour; "styles" shows every pointer)
set -e
cd "$(dirname "$0")/.."
OUT="${1:-out/demo.mp4}"
SCALE="${2:-1}"
FPS="${3:-60}"
SCENE="${4:-demo}"
command -v cargo >/dev/null || . "$HOME/.cargo/env"
command -v ffmpeg >/dev/null || { echo "demo.sh needs ffmpeg (brew install ffmpeg)"; exit 1; }
mkdir -p "$(dirname "$OUT")"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cargo build --release --quiet
POINTER_AUDIO="$TMP/sound.wav" target/release/pointer --scene "$SCENE" --video "$TMP/picture.mp4" \
  --duration 17 --scale "$SCALE" --fps "$FPS" 2>&1 | grep -E "wrote|wall" || true
ffmpeg -v error -y -i "$TMP/picture.mp4" -i "$TMP/sound.wav" -c:v copy -c:a aac -b:a 192k -shortest "$OUT"
echo "demo: $OUT"
