#!/bin/bash
# Menu parity numbers (doc 14 §1).
# usage: tools/research/menu_compare.sh <ours 1920x1200 png> <real 675x1200 png> <out prefix>
# Aligns the menu circle (ours: the 1200 px play square at x = 360, scaled to 675; real: the
# 675 px square at y = 228 of the game window), writes <prefix>-pair.png (real | ours | |diff|)
# and prints the mean absolute difference (luma of the diff, 0-255) over the square and per
# cell of a 3 x 3 grid (TL TC TR / ML MC MR / BL BC BR). Needs FFmpeg (WACCA_FFMPEG or PATH).
FF="${WACCA_FFMPEG:-ffmpeg}"
ours="$1"; real="$2"; out="$3"
"$FF" -v error -y -i "$real" -i "$ours" -filter_complex \
 "[0]crop=675:675:0:228,format=rgb24,split[r0][r1];[1]crop=1200:1200:360:0,scale=675:675:flags=area,format=rgb24,split[o0][o1];\
  [r1][o1]blend=all_mode=difference[d];[r0][o0][d]hstack=3" -frames:v 1 "$out-pair.png"
mean() {
  "$FF" -hide_banner -i "$out-pair.png" -vf "crop=$1,format=gray,signalstats,metadata=print:key=lavfi.signalstats.YAVG" \
    -f null - 2>&1 | grep -o "YAVG=[0-9.]*" | cut -d= -f2
}
names=(TL TC TR ML MC MR BL BC BR)
line="all=$(printf %.1f "$(mean 675:675:1350:0)")"
for i in 0 1 2 3 4 5 6 7 8; do
  x=$(( (i % 3) * 225 )); y=$(( (i / 3) * 225 ))
  line="$line ${names[$i]}=$(printf %.1f "$(mean 225:225:$((1350 + x)):$y)")"
done
echo "$(basename "$out") $line"
