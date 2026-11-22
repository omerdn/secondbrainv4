#!/usr/bin/env python3
"""Generates the Routune brand mark PNGs (T0.7.6) without third-party packages.

The mark: a white loop arrow (routines) around a check mark (tasks) on the primary indigo.
Run from the repository root:  python3 apps/client/assets/brand/generate.py
then regenerate the desktop icons:  pnpm --filter @secondbrain/desktop exec tauri icon apps/client/assets/icon.png
"""

import math
import struct
import zlib
from pathlib import Path

ASSETS = Path(__file__).resolve().parent.parent
INDIGO = (0x4F, 0x46, 0xE5)  # packages/ui palette.indigo[600] (primary)
WHITE = (0xFF, 0xFF, 0xFF)

# Mark geometry in unit coordinates (0..1), centred at (0.5, 0.5).
RING_R = 0.27
RING_W = 0.075
GAP_START = math.radians(-75)  # gap in the ring, upper right (y grows downwards)
GAP_END = math.radians(-20)
CHECK = [(0.385, 0.51), (0.465, 0.59), (0.625, 0.42)]
CHECK_W = 0.075


def _seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def _in_triangle(px, py, a, b, c):
    def sign(p1, p2, p3):
        return (p1[0] - p3[0]) * (p2[1] - p3[1]) - (p2[0] - p3[0]) * (p1[1] - p3[1])

    d1, d2, d3 = sign((px, py), a, b), sign((px, py), b, c), sign((px, py), c, a)
    neg = d1 < 0 or d2 < 0 or d3 < 0
    pos = d1 > 0 or d2 > 0 or d3 > 0
    return not (neg and pos)


# Arrowhead at the end of the arc (where the ring resumes after the gap, pointing clockwise
# into the gap start, i.e. the loop "comes round").
_tip_angle = GAP_START + math.radians(4)
_base_angle = GAP_START - math.radians(30)
_bx, _by = 0.5 + RING_R * math.cos(_base_angle), 0.5 + RING_R * math.sin(_base_angle)
_nx, _ny = math.cos(_base_angle), math.sin(_base_angle)
_head = 0.085
ARROW = (
    (_bx + _nx * _head, _by + _ny * _head),
    (_bx - _nx * _head, _by - _ny * _head),
    (0.5 + RING_R * math.cos(_tip_angle), 0.5 + RING_R * math.sin(_tip_angle)),
)


def _in_mark(x, y):
    """True when the unit point (x, y) is on the mark."""
    r = math.hypot(x - 0.5, y - 0.5)
    if abs(r - RING_R) <= RING_W / 2:
        a = math.atan2(y - 0.5, x - 0.5)
        if not (GAP_START - math.radians(30) <= a <= GAP_END):
            return True
    if _in_triangle(x, y, *ARROW):
        return True
    for (ax, ay), (bx, by) in zip(CHECK, CHECK[1:]):
        if _seg_dist(x, y, ax, ay, bx, by) <= CHECK_W / 2:
            return True
    return False


def render(size, background, mark_color, scale=1.0, samples=3):
    """RGBA rows. `background` None = transparent. `scale` shrinks the mark around the centre."""
    rows = []
    step = 1.0 / samples
    for py in range(size):
        row = bytearray()
        for px in range(size):
            hits = 0
            for sy in range(samples):
                for sx in range(samples):
                    ux = ((px + (sx + 0.5) * step) / size - 0.5) / scale + 0.5
                    uy = ((py + (sy + 0.5) * step) / size - 0.5) / scale + 0.5
                    hits += _in_mark(ux, uy)
            cover = hits / (samples * samples)
            if background is None:
                row += bytes((*mark_color, round(255 * cover)))
            else:
                row += bytes(
                    round(b + (m - b) * cover) for b, m in zip(background, mark_color)
                ) + b"\xff"
        rows.append(bytes(row))
    return rows


def solid(size, color):
    return [bytes((*color, 255)) * size for _ in range(size)]


def write_png(path, rows):
    size = len(rows)
    raw = b"".join(b"\x00" + row for row in rows)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)
    print(f"wrote {path.relative_to(ASSETS.parent.parent.parent)} ({size}px)")


if __name__ == "__main__":
    # iOS/app icon: opaque, the OS rounds the corners.
    write_png(ASSETS / "icon.png", render(1024, INDIGO, WHITE))
    # Android adaptive icon: foreground inside the 66% safe zone, solid background layer.
    write_png(ASSETS / "android-icon-foreground.png", render(512, None, WHITE, scale=0.72))
    write_png(ASSETS / "android-icon-background.png", solid(512, INDIGO))
    write_png(ASSETS / "android-icon-monochrome.png", render(512, None, WHITE, scale=0.72))
    # Splash: the mark in indigo on the (white/dark) splash background.
    write_png(ASSETS / "splash-icon.png", render(512, None, INDIGO))
    write_png(ASSETS / "favicon.png", render(64, INDIGO, WHITE, samples=4))
