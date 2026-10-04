#!/usr/bin/env bash
# Cuts the shots of tools/trailer/record.sh into the trailer: a caption on each shot,
# crossfades between the shots, and a title card at the end.
# Needs ffmpeg and ImageMagick (magick). Writes out/trailer/timtech-trailer.mp4 and a small
# animated preview (out/trailer/timtech-preview.webp) for the README.
set -euo pipefail
SHOTS=${SHOTS:-out/trailer/shots}
WORK=out/trailer/work
OUT=out/trailer
FONT=assets/fonts/TitilliumWeb-Bold.ttf
FONT_SEMI=assets/fonts/TitilliumWeb-SemiBold.ttf
ACCENT='#ff8a2a'
FADE=0.5
mkdir -p "$WORK"

# Shot, start time in the shot, length, caption, caption place (x,y of the text base line;
# default: the lower left). The last shot is the title card.
LIST=(
  "01_world|0|4.5|Every cell in the world moves"
  "02_pour|0|4|Sand falls. Water flows."
  "03_fire|0.2|3.8|Oil burns. Fire spreads."
  "05_lava|0|4|Lava boils the lake"
  "04_boom|0|3.5|Blast holes in the ground"
  "06_robot|0|4.5|Dig, fly and fill your tanks"
  "07_kiln|0|3.5|Machines burn real fire"
  "08_smelter|0|3.5|Smelt ore and cast metal"
  "09_automation|0|4|Build automatic production lines"
  "10_research|0.4|2.6|Research new machines|132,936"
  "11_cave|0|4.5|Explore the caves below"
  "12_geode|0|3.5|Find crystal, gold and lava deep down"
  "13_title|0|6|"
)

# A caption: white text with a soft shadow and an orange bar left of it.
caption() {
  local text=$1 out=$2 x=$3 y=$4
  magick -size 1920x1080 xc:none \
    \( -size 1920x1080 xc:none -font "$FONT" -pointsize 68 -fill black -annotate "+$x+$y" "$text" \
       -blur 0x10 -channel A -evaluate multiply 0.9 +channel \) -composite \
    -fill "$ACCENT" -draw "rectangle $((x - 32)),$((y - 56)) $((x - 24)),$((y + 8))" \
    -font "$FONT" -pointsize 68 -fill white -annotate "+$x+$y" "$text" "$out"
}

# The title card: the name and one line under it, with a warm glow.
title() {
  local out=$1
  magick -size 1920x1080 xc:none \
    \( -size 1920x1080 xc:none -gravity center -font "$FONT" -pointsize 200 -fill "$ACCENT" -annotate +0-40 "TIMTECH" \
       -blur 0x24 -channel A -evaluate multiply 0.8 +channel \) -composite \
    \( -size 1920x1080 xc:none -gravity center -font "$FONT" -pointsize 200 -fill black -annotate +4-34 "TIMTECH" \
       -blur 0x6 -channel A -evaluate multiply 0.6 +channel \) -composite \
    -gravity center -font "$FONT" -pointsize 200 -fill white -annotate +0-40 "TIMTECH" \
    -font "$FONT_SEMI" -pointsize 56 -fill '#ffe2c4' -annotate +0+100 "A factory game in a world where every cell moves" "$out"
}

# Each shot: cut, color, vignette, and the caption with a fade in and out.
inputs=()
lengths=()
i=0
for row in "${LIST[@]}"; do
  IFS='|' read -r name start len text place <<<"$row"
  IFS=',' read -r cx cy <<<"${place:-132,968}"
  seg="$WORK/$(printf '%02d' $i).mp4"
  png="$WORK/$(printf '%02d' $i).png"
  if [ -z "$text" ]; then
    title "$png"
    # The title: the scene goes darker, then the name comes in.
    grade="eq=contrast=1.08:saturation=1.2:brightness=-0.04,gblur=sigma=2,vignette=angle=PI/4"
    cap_in=0.9
  else
    caption "$text" "$png" "$cx" "$cy"
    grade="eq=contrast=1.06:saturation=1.15,vignette=angle=PI/6"
    cap_in=0.35
  fi
  cap_out=$(echo "$len - 0.75" | bc)
  ffmpeg -v error -y -ss "$start" -t "$len" -i "$SHOTS/$name.mp4" -loop 1 -t "$len" -framerate 60 -i "$png" \
    -filter_complex "[0:v]setpts=PTS-STARTPTS,$grade[v];[1:v]format=rgba,fade=t=in:st=$cap_in:d=0.45:alpha=1,fade=t=out:st=$cap_out:d=0.4:alpha=1[c];[v][c]overlay=format=auto,format=yuv420p" \
    -r 60 -c:v libx264 -preset medium -crf 12 "$seg"
  inputs+=(-i "$seg")
  lengths+=("$len")
  i=$((i + 1))
done

# Crossfade the shots one after another.
graph=""
prev="[0:v]"
offset=0
for ((k = 1; k < ${#lengths[@]}; k++)); do
  offset=$(echo "$offset + ${lengths[$((k - 1))]} - $FADE" | bc)
  graph+="$prev[$k:v]xfade=transition=fade:duration=$FADE:offset=$offset[x$k];"
  prev="[x$k]"
done
total=$(echo "$offset + ${lengths[$((${#lengths[@]} - 1))]}" | bc)
graph+="${prev}fade=t=in:st=0:d=0.6,fade=t=out:st=$(echo "$total - 1" | bc):d=1,format=yuv420p[out]"
ffmpeg -v error -y "${inputs[@]}" -filter_complex "$graph" -map "[out]" -r 60 \
  -c:v libx264 -preset slow -crf 23 -movflags +faststart "$OUT/timtech-trailer.mp4"

# The README preview: smaller, 30 frames per second, and it loops (needs img2webp).
rm -rf "$WORK/preview" && mkdir -p "$WORK/preview"
ffmpeg -v error -y -i "$OUT/timtech-trailer.mp4" -vf "fps=30,scale=854:-2:flags=lanczos" "$WORK/preview/%04d.png"
img2webp -loop 0 -lossy -q 50 -m 4 -d 33 "$WORK"/preview/*.png -o "$OUT/timtech-preview.webp" >/dev/null
ls -lh "$OUT/timtech-trailer.mp4" "$OUT/timtech-preview.webp"
echo "length ${total} s"
