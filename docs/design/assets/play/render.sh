#!/usr/bin/env bash
# Renders the Google Play store graphics from the canonical brand files.
# Nothing here is drawn by hand: both images are a canonical SVG placed on a
# brand surface colour from tokens.json. Re-run after any change to the SVGs.
#
#   docs/design/assets/play/render.sh      (needs ImageMagick 7 with librsvg)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
assets="$here/.."

# 512 x 512, 32-bit PNG. Exactly what the Android launcher shows: the
# adaptive icon's visible 72-unit square, Surface #F7F9FC background
# (ic_launcher_background since 2026-10-03), and pliwee-mark.svg placed with
# the translate and scale ic_launcher_foreground.xml carries — read from that
# file, so the two cannot drift. Play applies its own corner mask.
fg="$here/../../../../android/app/src/main/res/drawable/ic_launcher_foreground.xml"
# The first <group> is the placement; later ones are radial paints.
tx=$(grep -oP '<group android:translateX="\K[0-9.]+' "$fg" | head -1)
ty=$(grep -oP '<group android:translateX="[0-9.]+" android:translateY="\K[0-9.]+' "$fg" | head -1)
sc=$(grep -oP 'android:scaleX="\K[0-9.]+(?=" android:scaleY)' "$fg" | head -1)
[[ -n "$tx" && -n "$ty" && -n "$sc" ]] || { echo "cannot read the launcher placement from $fg" >&2; exit 1; }
bg=$(grep -oP '<color name="ic_launcher_background">\K#[0-9A-Fa-f]{6}' "$(dirname "$fg")/../values/colors.xml")
[[ -n "$bg" ]] || { echo "cannot read ic_launcher_background" >&2; exit 1; }
icon=$(mktemp --suffix=.svg); trap 'rm -f "$icon"' EXIT
{ printf '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="18 18 72 72">'
  printf '<rect x="0" y="0" width="108" height="108" fill="%s"/><g transform="translate(%s %s) scale(%s)">' "$bg" "$tx" "$ty" "$sc"
  perl -0pe 's/^.*?<svg[^>]*>//s; s/<\/svg>\s*$//s; s/<title>.*?<\/title>//s' "$assets/pliwee-mark.svg"
  printf '</g></svg>'; } > "$icon"
magick -background none RSVG:"$icon" -resize 512x512 PNG32:"$here/play-icon-512.png"

# 1024 x 500, 24-bit PNG without alpha. The frozen Pliwee lockup (mark,
# wordmark, tagline; omnibridge-logo-lockup.svg until 2026-10-03) is drawn
# for light surfaces, so it sits on light background #F7F9FC.
magick -size 1024x500 xc:'#F7F9FC' \
    \( -background none -density 192 RSVG:"$assets/pliwee-lockup.svg" -resize 760x \) \
    -gravity center -composite -alpha off PNG24:"$here/play-feature-graphic-1024x500.png"

magick identify "$here"/play-*.png
