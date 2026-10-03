#!/usr/bin/env python3
"""Derive the in-app brand mark so that Compose draws it as Android does.

    python3 docs/reports/android/android-inapp-mark-compose-v1/derive_logo_mark.py [--check]

Reads, never writes:
    docs/design/assets/pliwee-mark.svg        (colour master)

Writes (or, with --check, compares against):
    android/app/src/main/res/drawable/logo_pliwee_mark.xml

Why this exists. `logo_pliwee_mark.xml` was derived by
docs/reports/branding/pliwee-wave-6/derive_android_icons.py with the faces'
radial paints nested inside one <group> per face, under one <group> clipped to
the silhouette. The platform VectorDrawable renderer intersects those clips
and draws the mark correctly — the launcher icon is that renderer. The app
draws this file through Compose's `painterResource`, whose XML parser
(androidx.compose.ui 1.7.6, `parseCurrentVectorNode`) keeps ONE counter of
open <clip-path>s for the whole document and, at the first `</group>` of any
kind, closes every clip opened so far. The first radial group's end therefore
drops both the face clip and the silhouette clip, and every later radial is
painted unclipped over the master's whole viewBox: the streak and haze seen
in the app bar and on Settings.

The fix is structural, not geometric. Every layer of the master's paint order
gets a <group> of its own carrying the clips that layer needs, declared
first, so that whatever closes first — the radial's transform group, or the
wrapper itself — closes exactly the clips of that layer and nothing else:

    <path silhouette  fill=paint-inner/>
    <group> <clip-path silhouette/> <path face fill=linear/> </group>
    <group> <clip-path silhouette/> <clip-path face/>
            <group T·R·S> <path unit-radial/> </group> </group>

Under VectorDrawable's rules this is the same drawing: two <clip-path>s in one
group intersect, as the nested ones did. Every pathData, gradient stop and
radial transform is produced by the Wave 6 functions themselves, imported
below and not modified, so BrandingResourcesTest's equality checks against
the master hold unchanged. The launcher layers are not touched: they are
drawn by the platform renderer, and are still exactly the Wave 6 output.
"""

import importlib.util
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WAVE6 = ROOT / "docs/reports/branding/pliwee-wave-6/derive_android_icons.py"
TARGET = ROOT / "android/app/src/main/res/drawable/logo_pliwee_mark.xml"

sys.dont_write_bytecode = True  # leave the Wave 6 evidence directory untouched
spec = importlib.util.spec_from_file_location("wave6", WAVE6)
w6 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(w6)

HEADER_LOGO = """<?xml version="1.0" encoding="utf-8"?>
<!--
  GENERATED FROM THE PLIWEE MASTER - DO NOT HAND-EDIT.
  Regenerate with docs/reports/android/android-inapp-mark-compose-v1/derive_logo_mark.py.

  The in-app brand mark, at the master's own 276x255 viewport. Every
  pathData and every gradient comes out of docs/design/assets/pliwee-mark.svg,
  by the same Wave 6 conversion as ic_launcher_foreground.xml, and is
  asserted against it.

  Unlike the launcher layer, this file is drawn by Compose, whose vector
  parser closes every open <clip-path> at the first </group>. So each layer
  sits in a <group> of its own that declares all the clips it needs, and
  nothing follows the radial's transform group inside it.
  BrandingResourcesTest asserts that shape.
-->
"""


def layer_body(svg, indent):
    paths = {pid: w6.path_d(svg, pid) for pid in ["silhouette"] + w6.FACES}
    grads = w6.gradients(svg)
    vb = w6.re.search(r'viewBox="0 0 ([\d.]+) ([\d.]+)"', svg).groups()
    viewbox = (float(vb[0]), float(vb[1]))
    base, uses = w6.paint_order(svg)

    clip = lambda pid, ind: f'{ind}<clip-path android:pathData="{paths[pid]}" />\n'
    inner = indent + "    "
    out = [w6.linear_path(paths["silhouette"], grads[base], indent)]
    for face, gid in uses:
        g = grads[gid]
        out.append(f"{indent}<group>\n" + clip("silhouette", inner))
        if g["kind"] == "linearGradient":
            out.append(w6.linear_path(paths[face], g, inner))
        else:
            out.append(clip(face, inner))
            out.append(w6.radial_group(g, viewbox, inner))
        out.append(f"{indent}</group>\n")
    return "".join(out), viewbox


def build():
    svg = w6.read("pliwee-mark.svg")
    body, (vw, vh) = layer_body(svg, "    ")
    return (HEADER_LOGO
            + '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
            + '    xmlns:aapt="http://schemas.android.com/aapt"\n'
            + f'    android:width="{w6.fmt(vw)}dp" android:height="{w6.fmt(vh)}dp"\n'
            + f'    android:viewportWidth="{w6.fmt(vw)}" android:viewportHeight="{w6.fmt(vh)}">\n'
            + body + "</vector>\n")


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        same = TARGET.is_file() and TARGET.read_text() == text
        print(f"{'SAME ' if same else 'DIFF '} {TARGET.relative_to(ROOT)}")
        sys.exit(0 if same else 1)
    TARGET.write_text(text)
    print(f"wrote {TARGET.relative_to(ROOT)} ({len(text.encode())} bytes)")


if __name__ == "__main__":
    main()
