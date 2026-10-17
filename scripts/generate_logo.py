#!/usr/bin/env python3
"""Generate the MantaSH folded-M mark and platform icons from one geometry.

Four flat facets form the approved Folded Summit identity. The color icons keep
its optical size and midnight-blue field; monochrome UI marks use the same
outline with tighter clear space. Requires Pillow, without fonts or networking.
"""
import argparse
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
INK = "#101B2A"
# Preserve the approved composition: a 55.8%-wide mark, optically centered.
# The original design spans x=214..786 and y=322..681 on a 1000-unit field.
MARK_SCALE = 64 * 0.558 / 572


def on_grid(points):
    """Resolve every facet and the continuous outline onto one 64-unit grid."""
    return tuple((32 + (x - 500) * MARK_SCALE, 32 + (y - 501.5) * MARK_SCALE)
                 for x, y in points)


FACETS = (
    ("#B6F1DE", on_grid(((214, 680), (214, 322), (328, 322), (328, 567)))),
    ("#58BEBA", on_grid(((328, 322), (500, 500), (500, 681), (328, 505)))),
    ("#D6F6F0", on_grid(((500, 500), (672, 322), (672, 505), (500, 681)))),
    ("#7F91DA", on_grid(((672, 322), (786, 322), (786, 680), (672, 567)))),
)
# A continuous monochrome contour avoids antialiasing seams between facets.
OUTLINE = on_grid(((214, 322), (328, 322), (500, 500), (672, 322),
                   (786, 322), (786, 680), (672, 567), (672, 505),
                   (500, 681), (328, 505), (328, 567), (214, 680)))


def svg_document():
    """Generate the single monochrome SVG consumed by the native toolbar."""
    points = " ".join(f"{x:.6f},{y:.6f}" for x, y in OUTLINE)
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="8 8 48 48" role="img" aria-label="MantaSH">'
        f'<polygon points="{points}" fill="#000000"/></svg>\n'
    )


def render_mark(pixels, tile=True):
    """Supersample the shared facets, keeping crisp folds at small icon sizes."""
    scale = 32
    image = Image.new("RGBA", (64 * scale, 64 * scale))
    draw = ImageDraw.Draw(image)
    if tile:
        draw.rounded_rectangle((2 * scale, 2 * scale, 62 * scale - 1, 62 * scale - 1),
                               radius=14 * scale, fill=INK)
    for color, points in FACETS:
        draw.polygon([(x * scale, y * scale) for x, y in points], fill=color)
    return image.resize((pixels, pixels), Image.Resampling.LANCZOS)


def render_macos_icon(pixels):
    """Fill the macOS canvas so system masks cannot expose a light backing plate."""
    background = Image.new("RGBA", (pixels, pixels), INK)
    return Image.alpha_composite(background, render_mark(pixels, tile=False))


def write_macos_icons(destination):
    """Write the platform ICNS with all of its embedded size representations."""
    render_macos_icon(1024).save(destination / "mantash.icns")


def main():
    """Write reproducible SVG/PNG/ICO/ICNS application assets."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--macos-only", action="store_true", help="Only rebuild macOS ICNS")
    args = parser.parse_args()
    destination = ROOT / "assets"
    if args.macos_only:
        write_macos_icons(destination)
        return
    (destination / "mantash-monochrome.svg").write_text(svg_document(), encoding="utf-8")
    # Only this standalone size is consumed by documentation. ICO/ICNS carry their own sizes.
    render_mark(128).save(destination / "mantash-128.png")
    bitmap = render_mark(1024)
    bitmap.save(destination / "mantash.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    write_macos_icons(destination)


if __name__ == "__main__":
    main()
