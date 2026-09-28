#!/usr/bin/env python3
"""Generate the MantaSH lens mark, platform icons and brand board from one geometry.

The mark is two overlapping circles — one filled, one a hollow ring — sharing a
lens where they meet: the local and the remote, the terminal and the file list,
one connection made visible. Requires Pillow. Fonts and their OFL licenses are
distributed in assets/fonts; no installed font, old project resource, network
request or application packaging is required.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

from release_version import source_version
ROOT = Path(__file__).resolve().parents[1]
INK = "#142A45"
BRAND = "#1F7FA6"
ACCENT = "#58C9E8"
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


def brand_board(destination):
    """Present the mark, wordmark, monochrome uses and actual pixel sizes on one calm canvas."""
    board = Image.new("RGB", (1600, 1060), "#F5F6F9")
    draw = ImageDraw.Draw(board)
    bold_path = ROOT / "assets/fonts/InstrumentSans-Bold.ttf"
    regular_path = ROOT / "assets/fonts/WorkSans-Regular.ttf"
    bold = ImageFont.truetype(str(bold_path), 104)
    small = ImageFont.truetype(str(regular_path), 18)
    tiny = ImageFont.truetype(str(regular_path), 14)
    regular = ImageFont.truetype(str(regular_path), 25)
    draw.text((76, 54), "MantaSH  /  Visual identity", font=small, fill=INK)
    draw.text((1397, 54), "01 / 2026", font=small, fill="#677286")
    board.paste(render_mark(338), (74, 182), render_mark(338))
    # The wordmark uses the formal MantaSH capitalization; the shell suffix carries the accent color.
    draw.text((475, 253), "Manta", font=bold, fill=INK)
    draw.text((475 + draw.textlength("Manta", font=bold), 253), "SH", font=bold, fill=BRAND)
    draw.text((481, 398), "Glide between shells.", font=regular, fill="#677286")
    draw.line((76, 586, 1524, 586), fill="#DDE2EB", width=1)
    draw.text((76, 619), "MONOCHROME", font=small, fill="#677286")
    draw.text((613, 619), "SMALL SIZES", font=small, fill="#677286")
    draw.text((1118, 619), "DARK SURFACE", font=small, fill="#677286")
    black = render_mark(128, INK, INK, False)
    board.paste(black, (68, 696), black)
    draw.rounded_rectangle((226, 686, 394, 854), radius=23, fill=INK)
    white = render_mark(136, WHITE, WHITE, False)
    board.paste(white, (242, 702), white)
    offset = 621
    for pixels in (16, 32, 64, 128):
        icon = render_mark(pixels)
        board.paste(icon, (offset, 824 - pixels), icon)
        draw.text((offset + (pixels - draw.textlength(str(pixels), font=tiny)) / 2, 853), str(pixels), font=tiny, fill="#677286")
        offset += pixels + 32
    draw.rounded_rectangle((1104, 682, 1524, 924), radius=24, fill=INK)
    mark = render_mark(54, ACCENT, ACCENT, False)
    board.paste(mark, (1136, 722), mark)
    word_bold = ImageFont.truetype(str(bold_path), 36)
    draw.text((1210, 731), "Manta", font=word_bold, fill=WHITE)
    draw.text((1210 + draw.textlength("Manta", font=word_bold), 731), "SH", font=word_bold, fill=ACCENT)
    for x, fill in [(1151, BRAND), (1254, ACCENT), (1357, WHITE)]:
        draw.rounded_rectangle((x, 830, x + 71, 876), radius=8, fill=fill)
    draw.text((76, 985), "Two circles, one shared lens.", font=small, fill="#677286")
    draw.text((1337, 985), f"MantaSH {source_version()}", font=small, fill=INK)
    board.save(destination / "logo-preview.png")
    board.save(destination / "logo-preview.pdf", resolution=144)


def main():
    """Write reproducible SVG/PNG/ICO/ICNS assets and the visual review board."""
    destination = ROOT / "assets"
    for name, disc, ring, tile in [
        ("mantash.svg", BRAND, WHITE, True),
        ("mantash-monochrome.svg", "#000000", "#000000", False),
        ("mantash-black.svg", "#000000", "#000000", False),
        ("mantash-white.svg", WHITE, WHITE, False),
        ("mantash-mark.svg", BRAND, BRAND, False),
    ]:
        (destination / name).write_text(svg_document(disc, ring, tile))
    for pixels in (16, 32, 64, 128, 256, 512, 1024):
        render_mark(pixels).save(destination / f"mantash-{pixels}.png")
    bitmap = render_mark(1024)
    bitmap.save(destination / "mantash.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    bitmap.save(destination / "mantash.icns")
    brand_board(destination)


if __name__ == "__main__":
    main()
