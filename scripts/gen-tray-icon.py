#!/usr/bin/env python3
"""Render the menu-bar hat (`core/rust-lib/assets/tray-hat.png`).

Geometry is COMPUTED, not hand-typed: the shape has to stay balanced when a
proportion is tuned, and hand-written path data is a silent source of error.

Measured facts this is built on (not guessed):
  * `NSStatusBar.system.thickness` = 22.0 pt on this machine → the icon square
    is 22 pt, so 44 x 44 px is exactly @2x. The previous asset already had the
    right SIZE; only the drawing was wrong.
  * The previous glyph filled 21 x 15 pt of that 22 pt square — 1 px of air on
    each side — and its crown-to-brim ratio was 22 px : 2 px. That is why it
    read as a bucket rather than a hat.

Design rules encoded here:
  * 18 pt glyph width inside the 22 pt square → 2 pt of breathing room a side.
  * Crown : brim ≈ 2 : 1, the proportion that says "hat" at a glance.
  * Nothing thinner than 3 px @2x (1.5 pt), so no part closes up at @1x.
  * No interior holes. The old teardrop cut-out mushed into a notch.
  * Alpha only: the RGB is solid white and the SHAPE lives in the alpha
    channel, which is what a macOS template image is.
"""
import math

from PIL import Image, ImageDraw

S = 8                      # supersample factor — downsampled for clean edges
PX = 44                    # 22 pt @2x
N = PX * S

CX = 22.0                  # centre of the square
GLYPH_HALF = 18.0          # OUTER half-width → 36 px = 18 pt, 2 pt air a side
HALF_W = GLYPH_HALF - 1.7  # …the brim's centre-line stops short of the round cap
BRIM_Y = 29.0              # centre-line of the brim
BRIM_RISE = 1.9            # how far the tips sweep UP from the centre
BRIM_MID = 7.6             # brim thickness at the centre
BRIM_TIP = 3.4             # …and at the tips (never below the 3 px floor)
CROWN_TOP = 12.6
CROWN_BASE_Y = 29.0
CROWN_HALF_TOP = 7.8
CROWN_HALF_BASE = 9.2
BAND_FLARE = 1.1           # the crown widens just above the brim = the band
BAND_Y = 24.6
# ⚠️ Measured on the real menu bar: at 1.5 px the dip is 0.75 px once macOS
# renders the 44 px asset into a 22 px slot on a NON-Retina display — it
# vanished completely and the hat read as a round bowler. A detail has to be
# ~2 px at @1x to survive, i.e. ~4 px here.
PINCH = 2.5                # dip in the crown's top — what makes it a fedora
DIP_SPAN = 0.84            # …wide and shallow: a NARROW deep dip leaves two horns
CORNER_R = 2.8             # rounded crown corners and brim tips: no sharp spikes


def brim_polygon(steps=160):
    """Stroke a shallow upswept arc with a width that tapers to the tips."""
    top, bot = [], []
    for i in range(steps + 1):
        t = i / steps                      # 0..1 across the brim
        x = CX - HALF_W + t * 2 * HALF_W
        u = abs(x - CX) / HALF_W           # 0 centre … 1 tip
        y = BRIM_Y - BRIM_RISE * u * u     # tips ride up
        w = BRIM_TIP + (BRIM_MID - BRIM_TIP) * (1 - u ** 1.6)
        top.append((x, y - w / 2))
        bot.append((x, y + w / 2))
    return top + bot[::-1]


def crown_polygon(steps=80):
    """Rounded trapezoid, pinched on top, flared at the band."""
    pts = []
    # left side, bottom → top
    for i in range(steps + 1):
        t = i / steps
        y = CROWN_BASE_Y - t * (CROWN_BASE_Y - CROWN_TOP)
        half = CROWN_HALF_BASE + (CROWN_HALF_TOP - CROWN_HALF_BASE) * t
        if y > BAND_Y:                     # flare below the band line
            half += BAND_FLARE * (y - BAND_Y) / (CROWN_BASE_Y - BAND_Y)
        pts.append((CX - half, y))
    # Top edge, left → right. ⚠️ The dip is confined to the MIDDLE: a parabola
    # across the whole top turned the corners into two sharp horns — it read as
    # a crown, not a hat (seen only by rendering it).
    for i in range(steps + 1):
        t = i / steps
        x = CX - CROWN_HALF_TOP + t * 2 * CROWN_HALF_TOP
        u = abs(x - CX) / (CROWN_HALF_TOP * DIP_SPAN)
        dip = PINCH * 0.5 * (1 + math.cos(math.pi * min(u, 1.0)))
        pts.append((x, CROWN_TOP + dip))
    # right side, top → bottom
    for i in range(steps + 1):
        t = i / steps
        y = CROWN_TOP + t * (CROWN_BASE_Y - CROWN_TOP)
        half = CROWN_HALF_TOP + (CROWN_HALF_BASE - CROWN_HALF_TOP) * t
        if y > BAND_Y:
            half += BAND_FLARE * (y - BAND_Y) / (CROWN_BASE_Y - BAND_Y)
        pts.append((CX + half, y))
    return pts


def main() -> None:
    mask = Image.new("L", (N, N), 0)
    d = ImageDraw.Draw(mask)
    for poly in (crown_polygon(), brim_polygon()):
        d.polygon([(x * S, y * S) for x, y in poly], fill=255)
    # Round every extremity. Sharp spikes alias badly and look fragile at 22 pt.
    for cx, cy, r in (
        (CX - HALF_W, BRIM_Y - BRIM_RISE, BRIM_TIP / 2),
        (CX + HALF_W, BRIM_Y - BRIM_RISE, BRIM_TIP / 2),
        (CX - CROWN_HALF_TOP + CORNER_R, CROWN_TOP + CORNER_R, CORNER_R),
        (CX + CROWN_HALF_TOP - CORNER_R, CROWN_TOP + CORNER_R, CORNER_R),
    ):
        d.ellipse(
            [(cx - r) * S, (cy - r) * S, (cx + r) * S, (cy + r) * S], fill=255
        )
    # Round the crown's top corners by drawing over them with circles is not
    # needed — the pinch curve already meets the sides tangentially.
    # ⚠️ BOX, not LANCZOS: this is supersampled COVERAGE, and Lanczos rings —
    # it feathered ~3 px of haze past the shape on each side. A box filter is
    # the exact area average, which is what antialiasing a silhouette means.
    alpha = mask.resize((PX, PX), Image.BOX)

    # Centre the glyph optically: a hat's mass sits low, so balancing the
    # BOUNDING BOX would leave it looking bottom-heavy in the bar.
    # ⚠️ Shift by CROP+PASTE, never an affine transform: resampling a finished
    # alpha re-blurs every edge and measurably widened the glyph.
    bb = alpha.getbbox()
    dx = round((PX - (bb[2] - bb[0])) / 2 - bb[0])
    dy = round((PX - (bb[3] - bb[1])) / 2 - bb[1])
    shifted = Image.new("L", (PX, PX), 0)
    shifted.paste(alpha, (dx, dy))
    alpha = shifted

    # Template image: white pixels, the shape carried by alpha alone.
    out = Image.merge("RGBA", (
        Image.new("L", (PX, PX), 255),
        Image.new("L", (PX, PX), 255),
        Image.new("L", (PX, PX), 255),
        alpha,
    ))
    out.save("core/rust-lib/assets/tray-hat.png")
    px = alpha.load()
    solid = max(
        sum(1 for x in range(PX) if px[x, y] > 128) for y in range(PX)
    )
    bb = alpha.getbbox()
    print(f"{PX}x{PX} px (22 pt @2x) — solide Breite {solid} px = {solid/2:.1f} pt, "
          f"inkl. Kantenglättung {bb[2]-bb[0]} px; Höhe {bb[3]-bb[1]} px")


if __name__ == "__main__":
    main()
