#!/usr/bin/env python3
"""Generate the MantaSH lens mark and platform icons from one geometry.

The mark is two overlapping circles — one filled, one a hollow ring — sharing a
lens where they meet: the local and the remote, the terminal and the file list,
one connection made visible. Requires Pillow; no font files, network requests
or application packaging are required.
"""
import argparse
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
INK = "#142A45"
BRAND = "#1F7FA6"
WHITE = "#FFFFFF"
# The two circles of the lens mark, on the shared 64-unit grid. The filled
# disc sits up-left of the hollow ring; their overlap is the shared lens.
DISC = (24.0, 26.0, 15.0)
RING = (40.0, 38.0, 15.0)
RING_WIDTH = 4.4


def svg_document(disc, ring, tile=False):
    """Keep every vector variant on the same grid, with no font or external-file dependency."""
    background = f'<rect x="2" y="2" width="60" height="60" rx="14" fill="{INK}"/>' if tile else ""
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="MantaSH">'
        f'{background}'
        f'<circle cx="{DISC[0]}" cy="{DISC[1]}" r="{DISC[2]}" fill="{disc}"/>'
        f'<circle cx="{RING[0]}" cy="{RING[1]}" r="{RING[2]}" fill="none" stroke="{ring}" stroke-width="{RING_WIDTH}"/></svg>\n'
    )


def render_mark(pixels, disc=BRAND, ring=WHITE, tile=True):
    """Supersample the lens mark, preserving the ring's weight at 16px."""
    scale = 32
    image = Image.new("RGBA", (64 * scale, 64 * scale))
    draw = ImageDraw.Draw(image)
    if tile:
        draw.rounded_rectangle((2 * scale, 2 * scale, 62 * scale - 1, 62 * scale - 1), radius=14 * scale, fill=INK)
    draw.ellipse(
        (
            (DISC[0] - DISC[2]) * scale,
            (DISC[1] - DISC[2]) * scale,
            (DISC[0] + DISC[2]) * scale,
            (DISC[1] + DISC[2]) * scale,
        ),
        fill=disc,
    )
    # PIL strokes grow inward from the bounding box; offset by half the width
    # so the ring's centerline sits on the same radius the SVG strokes.
    half = RING_WIDTH / 2
    draw.ellipse(
        (
            (RING[0] - RING[2] - half) * scale,
            (RING[1] - RING[2] - half) * scale,
            (RING[0] + RING[2] + half) * scale,
            (RING[1] + RING[2] + half) * scale,
        ),
        outline=ring,
        width=int(RING_WIDTH * scale),
    )
    return image.resize((pixels, pixels), Image.Resampling.LANCZOS)


def render_macos_icon(pixels):
    """Fill the macOS icon canvas; let the system apply its own rounded mask.

    A pre-rounded tile with transparent padding can expose the system's light
    backing plate in process lists and other small native icon presentations.
    Keep the mark geometry shared with the other platforms, without that padding.
    """
    background = Image.new("RGBA", (pixels, pixels), INK)
    return Image.alpha_composite(background, render_mark(pixels, tile=False))


def write_macos_icons(destination):
    """Keep a reviewable full-canvas PNG beside the shipped ICNS."""
    bitmap = render_macos_icon(1024)
    bitmap.save(destination / "mantash-macos-1024.png")
    bitmap.save(destination / "mantash.icns")


def main():
    """Write reproducible SVG/PNG/ICO/ICNS application assets."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--macos-only", action="store_true", help="Only rebuild macOS PNG and ICNS")
    args = parser.parse_args()
    destination = ROOT / "assets"
    if args.macos_only:
        write_macos_icons(destination)
        return
    for name, disc, ring, tile in [
        ("mantash.svg", BRAND, WHITE, True),
        ("mantash-monochrome.svg", "#000000", "#000000", False),
        ("mantash-black.svg", "#000000", "#000000", False),
        ("mantash-white.svg", WHITE, WHITE, False),
        ("mantash-mark.svg", BRAND, BRAND, False),
    ]:
        (destination / name).write_text(svg_document(disc, ring, tile))
    # Only this standalone size is consumed by documentation. ICO/ICNS carry their own sizes.
    render_mark(128).save(destination / "mantash-128.png")
    bitmap = render_mark(1024)
    bitmap.save(destination / "mantash.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    write_macos_icons(destination)


if __name__ == "__main__":
    main()
