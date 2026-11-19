#!/bin/sh
# Renders the website's media with the app itself (headless, so every frame is deterministic):
# the promo film with its sound, a silent loop per feature, the settings window's pages and
# the social preview. Everything lands in docs/media.
#   scripts/site-media.sh
set -eu
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || . "$HOME/.cargo/env"
command -v ffmpeg >/dev/null 2>&1 || { echo "site-media.sh needs ffmpeg (brew install ffmpeg)" >&2; exit 1; }
[ "${SKIP_BUILD:-}" = 1 ] || cargo build --release --quiet
bin=target/release/pointer
media=docs/media
work=out/site
mkdir -p "$media" "$work"

render() { # scene seconds name: rendered at 2x, so the cropped close-ups stay sharp
    [ "${SKIP_RENDER:-}" = 1 ] && [ -f "$work/$3.mp4" ] && return 0
    POINTER_AUDIO="$work/$3.wav" "$bin" --scene "$1" --video "$work/$3.mp4" --duration "$2" --scale 2 --fps 60 2>/dev/null
}
render demo 17 demo
render styles 17 styles

# The promo: the tools tour, then every pointer, with a short cross-fade.
ffmpeg -v error -y -i "$work/demo.mp4" -i "$work/demo.wav" -i "$work/styles.mp4" -i "$work/styles.wav" \
    -filter_complex "[0:v]scale=1920:1080:flags=lanczos,format=yuv420p,setsar=1[a];[2:v]scale=1920:1080:flags=lanczos,format=yuv420p,setsar=1[b];[a][b]xfade=transition=fade:duration=0.5:offset=16.5[v];[1:a][3:a]acrossfade=d=0.5[s]" \
    -map "[v]" -map "[s]" -c:v libx264 -preset slow -crf 21 -profile:v high -pix_fmt yuv420p -r 60 \
    -c:a aac -b:a 160k -movflags +faststart "$media/promo.mp4"
ffmpeg -v error -y -ss 0.9 -i "$media/promo.mp4" -frames:v 1 -q:v 3 "$media/promo-poster.jpg"

# A silent close-up loop: source, start, seconds, name, the centre and width of the region to
# show (in points of the 1920×1080 page), and when in the clip to take its poster frame.
clip() {
    set -- "$@" $(awk -v cx="$5" -v cy="$6" -v w="$7" 'BEGIN {
        h = w * 9 / 16; x = cx - w / 2; y = cy - h / 2
        if (x < 0) x = 0; if (y < 0) y = 0; if (x + w > 1920) x = 1920 - w; if (y + h > 1080) y = 1080 - h
        printf "%d %d %d %d", w * 2, h * 2, x * 2, y * 2 }')
    ffmpeg -v error -y -ss "$2" -t "$3" -i "$work/$1.mp4" -an \
        -vf "crop=$9:${10}:${11}:${12},scale=1280:720:flags=lanczos,format=yuv420p" \
        -c:v libx264 -preset slow -crf 24 -profile:v high -r 60 -movflags +faststart "$media/$4.mp4"
    ffmpeg -v error -y -ss "$8" -i "$media/$4.mp4" -frames:v 1 -q:v 4 "$media/$4.jpg"
}
# (Crops start in the gaps between the page's title, subtitle, field, chips and tiles.)
clip demo 1.7 2.5 tap 880 570 960 0.75
clip demo 4.2 2.4 crack 760 620 1120 2.2
clip demo 6.7 2.6 spotlight 980 589 1240 1.2
clip demo 9.4 1.8 laser 960 470 1440 1.25
clip demo 11.2 1.9 pen 1120 721 1000 1.5
clip demo 13.2 3.0 focus 960 660 1280 2.6
i=0
for style in glove saber wand mallet chicken; do
    clip styles "$(awk "BEGIN { printf \"%.2f\", $i * 3.4 + 0.6 }")" 2.8 "style-$style" 1000 615 1120 1.75
    i=$((i + 1))
done

# The settings window, light and dark.
for page in general pointer shortcuts update-available; do
    for theme in light dark; do
        "$bin" --settings --scene "$page" --theme "$theme" --snap "$work/settings-$page-$theme.png" --scale 2 2>/dev/null
        ffmpeg -v error -y -i "$work/settings-$page-$theme.png" -q:v 2 "$media/settings-$page-$theme.jpg"
    done
done

# The social card links show, and the pointer thumbnails.
"$bin" --scene art-card --size 1200x630 --scale 1 --at 2.5 --snap "$work/card.png" 2>/dev/null
ffmpeg -v error -y -i "$work/card.png" -q:v 2 "$media/og.jpg"
for style in glove saber wand mallet chicken; do cp "assets/thumbs/$style.png" "$media/thumb-$style.png"; done

# The icon at web sizes.
sips -z 512 512 assets/app-icon-1024.png --out "$media/icon-512.png" >/dev/null
sips -z 180 180 assets/app-icon-1024.png --out "$media/apple-touch-icon.png" >/dev/null
sips -z 64 64 assets/app-icon-1024.png --out "$media/favicon-64.png" >/dev/null
echo "site-media: done ($media)"
