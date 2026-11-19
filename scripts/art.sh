#!/bin/sh
# Renders the app icon and the settings window's pointer thumbnails with the app itself
# (headless), then cuts the icon to the macOS shape and builds AppIcon.icns.
#   scripts/art.sh
set -eu
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || . "$HOME/.cargo/env"
cargo build --release --quiet
bin=target/release/pointer
out=out/art
mkdir -p "$out" assets/thumbs

for style in glove saber wand mallet chicken; do
    POINTER_TRANSPARENT=1 "$bin" --scene "art-$style" --size 256x256 --scale 1 --at 2.5 --snap "assets/thumbs/$style.png" 2>/dev/null
done

"$bin" --scene art-icon --size 1024x1024 --scale 1 --at 2.5 --snap "$out/icon-raw.png" 2>/dev/null
# The icon's shape: an 824-point superellipse centred in the 1024 canvas (Apple's grid),
# antialiased, over a soft drop shadow.
shape="lum='255*clip(0.5+82*(1-pow(abs(X-511.5)/412,5)-pow(abs(Y-511.5)/412,5)),0,1)'"
ffmpeg -v error -y -i "$out/icon-raw.png" \
    -f lavfi -i "nullsrc=s=1024x1024,format=gray,geq=$shape" \
    -f lavfi -i "color=black:s=1024x1024,format=rgba" \
    -f lavfi -i "color=black@0:s=1024x1024,format=rgba" \
    -filter_complex "[1]split[m1][m2];[0]format=rgba[icon];[icon][m1]alphamerge[cut];[2][m2]alphamerge,gblur=sigma=14,colorchannelmixer=aa=0.38[shadow];[3][shadow]overlay=0:12:format=auto[s];[s][cut]overlay=0:0:format=auto,format=rgba" \
    -frames:v 1 assets/app-icon-1024.png

set_dir="$out/AppIcon.iconset"
rm -rf "$set_dir" && mkdir -p "$set_dir"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/app-icon-1024.png --out "$set_dir/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" assets/app-icon-1024.png --out "$set_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o assets/AppIcon.icns
sips -z 256 256 assets/app-icon-1024.png --out assets/app-icon-256.png >/dev/null
echo "art: assets/app-icon-1024.png, assets/AppIcon.icns, assets/app-icon-256.png, assets/thumbs/*.png"
