#!/usr/bin/env sh
# `npm run icon` — assets/icon.svg → the two raster forms every platform needs:
#
#   assets/icon.png          the clapp icon the Clapp Protocol fixes (square PNG,
#                            512–1024, ≤1 MiB; docs/icons.md) — the library tile, the Dock,
#                            and the bytes the running app sets as its own icon.
#   src-tauri/icons/icon.ico the Windows resource compiled INTO the exe. tauri-build reads
#                            it from the first .ico in tauri.conf.json's bundle.icon and
#                            FAILS THE BUILD without one — which is exactly how the first
#                            Windows release run died.
#
# The SVG is the source of truth and both of these are derived, so never hand-edit them:
# re-render. librsvg (`brew install librsvg`, `apt install librsvg2-bin`) draws the PNG;
# Pillow (`pip3 install pillow`) packs the .ico.
set -eu
ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

command -v rsvg-convert >/dev/null 2>&1 || {
  echo "rsvg-convert not found — brew install librsvg (or apt install librsvg2-bin)" >&2
  exit 1
}
rsvg-convert -w 1024 -h 1024 "$ROOT/assets/icon.svg" -o "$ROOT/assets/icon.png"
echo "assets/icon.png — $(du -h "$ROOT/assets/icon.png" | awk '{print $1}')"

mkdir -p "$ROOT/src-tauri/icons"
if python3 -c 'import PIL' 2>/dev/null; then
  python3 "$ROOT/scripts/make-ico.py" "$ROOT"
else
  echo "note: Pillow not installed (pip3 install pillow) — src-tauri/icons/icon.ico left as is" >&2
fi
