#!/bin/sh
# The launch film: renders each shot of src/promo.rs with its sound, films the settings
# window over its still, cuts everything together with short cross-fades, and lays the
# voiceover on top phrase by phrase (assets/promo/voiceover.cues says where each goes).
# The app's own sound effects sit at 20% so the voice reads clearly over them.
#
#   scripts/promo.sh                  the film with its voiceover
#   VOICE=none scripts/promo.sh       ...without it
#   MUSIC=bed.mp3 scripts/promo.sh    ...with a music bed under the voice
#
# Writes out/promo/pointer-launch.mp4 (1920×1080, 60 fps, for uploading) and the website's
# lighter copy, docs/media/launch.mp4, with its poster. SKIP_RENDER=1 reuses rendered shots.
set -eu
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || . "$HOME/.cargo/env"
command -v ffmpeg >/dev/null 2>&1 || { echo "promo.sh needs ffmpeg (brew install ffmpeg)" >&2; exit 1; }
[ "${SKIP_BUILD:-}" = 1 ] || cargo build --release --quiet
bin=target/release/pointer
out=out/promo
work=$out/shots
mkdir -p "$work" docs/media
fade=0.4                                     # cross-fade between shots, seconds
sfx=0.2                                      # the app's sound effects, under the voice
music=0.18                                   # a music bed, under the voice
voice=${VOICE:-assets/promo/voiceover.mp3}
cues=${VOICE_CUES:-assets/promo/voiceover.cues}
voice_gain=${VOICE_GAIN:-3.4dB}              # the take is about -19 LUFS; this lifts it to about -16
settings_len=6.4

# The cut, in order: name, start in the film, length. The settings window's shot follows
# the pointers.
"$bin" --promo-shots | awk -v s="$settings_len" '{ print } $1 == "pointers" { print "settings", s }' |
    awk -v f="$fade" '{ printf "%s %.3f %s\n", $1, t, $2; t += $2 - f }' > "$work/cut.txt"

render() { # name length
    [ "${SKIP_RENDER:-}" = 1 ] && [ -f "$work/$1.mp4" ] && return 0
    if [ "$1" = settings ]; then
        "$bin" --scene promo-wall --size 1920x1080 --scale 2 --at 0.5 --snap "$work/wall.png" 2>/dev/null
        cat > "$work/settings.json" <<'JSON'
[
  {"t": 1.2, "type": "tap", "key": "rec-toggle"},
  {"t": 2.3, "physical": "ctrl+alt+k"},
  {"t": 3.7, "type": "tap", "key": "rec-next-style"},
  {"t": 4.8, "physical": "ctrl+shift+n"},
  {"t": 6.4, "type": "mark", "name": "end"}
]
JSON
        "$bin" --settings --scene shortcuts --desktop "$work/wall.png" --size 1920x1080 --scale 2 --pointer \
            --script "$work/settings.json" --video "$work/settings-raw.mp4" --duration "$2" --fps 60 2>/dev/null
        # A slow push in toward the recorders, easing out.
        ffmpeg -nostdin -v error -y -i "$work/settings-raw.mp4" \
            -vf "zoompan=z='1+0.32*(1-pow(1-on/($2*60),2))':x='(iw-iw/zoom)*0.6':y='(ih-ih/zoom)*0.4':d=1:s=3840x2160:fps=60" \
            -c:v libx264 -preset fast -crf 16 -pix_fmt yuv420p "$work/settings.mp4"
        ffmpeg -nostdin -v error -y -f lavfi -i anullsrc=r=48000:cl=stereo -t "$2" "$work/settings.wav"
    else
        POINTER_AUDIO="$work/$1.wav" "$bin" --scene "promo-$1" --video "$work/$1.mp4" --duration "$2" \
            --size 1920x1080 --scale 2 --fps 60 2>/dev/null
    fi
}
while read -r name start length; do render "$name" "$length"; done < "$work/cut.txt"

# Cross-fade the pictures and the effects into one track each.
inputs="" graph="" i=0 vlast="" alast="" total=0
while read -r name start length; do
    inputs="$inputs -i $work/$name.mp4 -i $work/$name.wav"
    v=$((i * 2)) a=$((i * 2 + 1))
    graph="$graph[$v:v]scale=1920:1080:flags=lanczos,fps=60,format=yuv420p,setsar=1,setpts=PTS-STARTPTS[v$i];"
    graph="$graph[$a:a]aresample=48000,aformat=channel_layouts=stereo,asetpts=PTS-STARTPTS[a$i];"
    if [ "$i" -eq 0 ]; then
        vlast="v0" alast="a0"
    else
        graph="$graph[$vlast][v$i]xfade=transition=fade:duration=$fade:offset=$start[x$i];[$alast][a$i]acrossfade=d=$fade[y$i];"
        vlast="x$i" alast="y$i"
    fi
    total=$(awk -v s="$start" -v l="$length" 'BEGIN { printf "%.3f", s + l }')
    i=$((i + 1))
done < "$work/cut.txt"
graph="$graph[$alast]volume=$sfx[fx]"
extra="" layers="" n=$((i * 2)) k=0

# The voiceover: each phrase trimmed from the take, faded at its edges, and placed.
if [ "$voice" != none ]; then
    grep -v '^#' "$cues" | awk 'NF >= 4' > "$work/cues.txt"
    count=$(wc -l < "$work/cues.txt" | tr -d ' ')
    extra="$extra -i $voice"
    split=""
    j=0
    while [ "$j" -lt "$count" ]; do split="$split[s$j]"; j=$((j + 1)); done
    graph="$graph;[$n:a]aresample=48000,aformat=channel_layouts=stereo,volume=$voice_gain,asplit=$count$split"
    j=0 last_end=0
    while read -r shot at from to words; do
        start=$(awk -v s="$shot" '$1 == s { print $2 }' "$work/cut.txt")
        [ -n "$start" ] || { echo "promo: no shot named $shot in the cut" >&2; exit 1; }
        place=$(awk -v s="$start" -v a="$at" 'BEGIN { printf "%.3f", s + a }')
        len=$(awk -v f="$from" -v t="$to" 'BEGIN { printf "%.3f", t - f }')
        awk -v p="$place" -v e="$last_end" -v w="$words" 'BEGIN { if (p < e) printf "promo: \"%s\" starts %.2fs before the line before it ends\n", w, e - p }' >&2
        last_end=$(awk -v p="$place" -v l="$len" 'BEGIN { printf "%.3f", p + l }')
        ms=$(awk -v p="$place" 'BEGIN { printf "%d", p * 1000 }')
        tail=$(awk -v l="$len" 'BEGIN { printf "%.3f", l - 0.05 }')
        graph="$graph;[s$j]atrim=start=$from:end=$to,asetpts=PTS-STARTPTS,afade=t=in:d=0.02,afade=t=out:st=$tail:d=0.05,adelay=$ms|$ms[vo$j]"
        layers="$layers[vo$j]"
        j=$((j + 1))
    done < "$work/cues.txt"
    n=$((n + 1)) k=$((k + count))
fi
if [ -n "${MUSIC:-}" ]; then
    extra="$extra -stream_loop -1 -i $MUSIC"
    graph="$graph;[$n:a]aresample=48000,aformat=channel_layouts=stereo,atrim=0:$total,afade=t=in:d=1,afade=t=out:st=$(awk -v t="$total" 'BEGIN { print t - 2.5 }'):d=2.5,volume=$music[bed]"
    layers="$layers[bed]" k=$((k + 1))
fi
if [ "$k" -gt 0 ]; then
    graph="$graph;[fx]${layers}amix=inputs=$((k + 1)):normalize=0:duration=first,alimiter=limit=0.89:level=disabled[mix]"
else
    graph="$graph;[fx]anull[mix]"
fi

# shellcheck disable=SC2086
ffmpeg -nostdin -v error -y $inputs $extra -filter_complex "$graph" -map "[$vlast]" -map "[mix]" -t "$total" \
    -c:v libx264 -preset slow -crf 18 -profile:v high -pix_fmt yuv420p -r 60 \
    -c:a aac -b:a 192k -movflags +faststart "$out/pointer-launch.mp4"

# The website's copy: lighter, and a poster from the opening title.
ffmpeg -nostdin -v error -y -i "$out/pointer-launch.mp4" -c:v libx264 -preset slow -crf 24 -profile:v high \
    -pix_fmt yuv420p -c:a aac -b:a 160k -movflags +faststart docs/media/launch.mp4
ffmpeg -nostdin -v error -y -ss 1.0 -i "$out/pointer-launch.mp4" -frames:v 1 -q:v 3 docs/media/launch-poster.jpg
echo "promo: $out/pointer-launch.mp4 and docs/media/launch.mp4 (${total}s)"
