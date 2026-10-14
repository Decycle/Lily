#!/bin/sh
# Packs a finished Lily setup (the folder holding assets/ and data/) into one uncompressed
# lily-pack.tar for setup by URL on iPad (or any build with --setup-url). Background movies are
# left out: the iPad cannot play them. The pack is YOUR game data: keep it to your own devices.
#
#   tools/make-pack.sh <game-dir> [out.tar]
set -eu
dir=${1:?usage: tools/make-pack.sh <game-dir with assets/ and data/> [out.tar]}
out=${2:-lily-pack.tar}
for d in assets data; do
    [ -d "$dir/$d" ] || { echo "error: $dir/$d not found (run Lily's setup first)" >&2; exit 1; }
done
[ -f "$dir/data/songs.json" ] || { echo "error: $dir/data/songs.json missing (setup incomplete?)" >&2; exit 1; }
# No AppleDouble (._*) files on macOS; Lily skips them anyway.
export COPYFILE_DISABLE=1
mac_flags=""
tar --version 2>/dev/null | grep -q bsdtar && mac_flags="--no-mac-metadata --no-xattrs"
# shellcheck disable=SC2086
tar $mac_flags --exclude 'assets/Movie' --exclude '.DS_Store' -cf "$out" -C "$dir" assets data
size=$(wc -c < "$out" | tr -d ' ')
echo "wrote $out ($((size / 1000000)) MB)"
echo "share it with: tools/serve-pack.sh $out"
