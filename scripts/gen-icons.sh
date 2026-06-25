#!/usr/bin/env bash
# Regenerate platform icon assets from the master emblem art.
#
# Source of truth : build/branding/logo_master_bore.png  (square emblem, transparent bg)
# Generated outputs (committed):
#   build/icons/window_icon.png  - transparent emblem, 256 (winit runtime icon, Win/X11)
#   build/icons/icon.ico         - transparent emblem (Windows .exe resource)
#   build/icons/icon.icns        - emblem on a dark gunmetal plate (macOS app bundle)
#
# Why the macOS icon differs: macOS 26 (Tahoe) renders app icons in a unified
# rounded-rect and drops icons WITH transparency onto a default white plate with
# extra margin. So macOS gets an opaque, full-bleed plate (our own background +
# Tahoe-style safe-area margin baked in); Windows/Linux keep the clean emblem.
#
# We derive icons from the EMBLEM, not the wordmark: text-in-icon is illegible at
# 16x16/32x32. Re-run this whenever the master art changes.
#
# Requires: sips + iconutil (macOS, built-in) and ImageMagick (`brew install imagemagick`).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/build/branding/logo_master_bore.png"
OUT="$ROOT/build/icons"
mkdir -p "$OUT"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# resize <src> <px> <dest> — square resample into a PNG.
resize() { sips -s format png -z "$2" "$2" "$1" --out "$3" >/dev/null; }

echo ">> window_icon.png (256, winit runtime icon for Windows/X11)"
resize "$SRC" 256 "$OUT/window_icon.png"

echo ">> icon.ico (Windows exe resource, multi-resolution)"
ico_pngs=()
for s in 16 32 48 64 128 256; do resize "$SRC" "$s" "$tmp/ico_$s.png"; ico_pngs+=("$tmp/ico_$s.png"); done
magick "${ico_pngs[@]}" "$OUT/icon.ico"

echo ">> macOS icon master (emblem on dark gunmetal plate, Tahoe safe-area margin)"
# Plate: 832x832 rounded-rect (radius 185) inside a 1024 canvas => ~9% margin.
# Emblem: ~84% of the plate, centered. Background fully opaque so Tahoe doesn't
# add its default white plate.
magick "$SRC" -trim +repage "$tmp/emblem.png"
magick -size 832x832 xc:black -fill white -draw 'roundrectangle 0,0,831,831,185,185' "$tmp/mask.png"
magick -size 832x832 'gradient:#4a4e54-#16181b' "$tmp/mask.png" \
  -alpha off -compose CopyOpacity -composite "$tmp/plate.png"
magick "$tmp/plate.png" \( "$tmp/emblem.png" -resize 690x690 \) \
  -gravity center -compose over -composite "$tmp/plate_emblem.png"
magick -size 1024x1024 xc:none "$tmp/plate_emblem.png" \
  -gravity center -composite "$tmp/icon_macos_1024.png"

echo ">> icon.icns (macOS app bundle)"
iconset="$tmp/icon.iconset"; mkdir -p "$iconset"
mac_src="$tmp/icon_macos_1024.png"
resize "$mac_src" 16   "$iconset/icon_16x16.png"
resize "$mac_src" 32   "$iconset/icon_16x16@2x.png"
resize "$mac_src" 32   "$iconset/icon_32x32.png"
resize "$mac_src" 64   "$iconset/icon_32x32@2x.png"
resize "$mac_src" 128  "$iconset/icon_128x128.png"
resize "$mac_src" 256  "$iconset/icon_128x128@2x.png"
resize "$mac_src" 256  "$iconset/icon_256x256.png"
resize "$mac_src" 512  "$iconset/icon_256x256@2x.png"
resize "$mac_src" 512  "$iconset/icon_512x512.png"
resize "$mac_src" 1024 "$iconset/icon_512x512@2x.png"
iconutil -c icns "$iconset" -o "$OUT/icon.icns"

echo ">> Done. Generated in $OUT:"
ls -1 "$OUT"
