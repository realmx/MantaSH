"""Regression checks for macOS system-icon backing, including every shipped ICNS representation."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import generate_logo


class MacIconTests(unittest.TestCase):
    def test_every_shipped_icns_size_has_opaque_brand_edges(self):
        path = ROOT / "assets/mantash.icns"
        with Image.open(path) as icon:
            sizes = icon.info["sizes"]
        self.assertTrue(sizes)
        for width, height, scale in sizes:
            with self.subTest(size=(width, height, scale)), Image.open(path) as icon:
                icon.size = (width, height)
                icon.load(scale=scale)
                rgba = icon.convert("RGBA")
                self.assertEqual(rgba.getchannel("A").getextrema(), (255, 255))
                w, h = rgba.size
                for edge in [(0, 0, w, 1), (0, h - 1, w, h), (0, 0, 1, h), (w - 1, 0, w, h)]:
                    self.assertEqual(rgba.crop(edge).getextrema(),
                                     ((20, 20), (42, 42), (69, 69), (255, 255)))

    def test_macos_generation_matches_checked_in_png_and_keeps_other_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary)
            other = destination / "mantash.ico"
            other.write_bytes(b"unrelated platform asset")
            generate_logo.write_macos_icons(destination)
            self.assertEqual(other.read_bytes(), b"unrelated platform asset")
            with Image.open(destination / "mantash-macos-1024.png") as generated, \
                    Image.open(ROOT / "assets/mantash-macos-1024.png") as shipped, \
                    Image.open(destination / "mantash.icns") as icns:
                self.assertEqual(generated.tobytes(), shipped.convert("RGBA").tobytes())
                self.assertEqual(generated.tobytes(), icns.convert("RGBA").tobytes())

    def test_full_generation_keeps_consumed_assets_and_embedded_icon_sizes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "assets"
            destination.mkdir()
            with patch.object(generate_logo, "ROOT", root), \
                    patch.object(sys, "argv", ["generate_logo.py"]):
                generate_logo.main()
            self.assertEqual({path.name for path in destination.glob("*.png")},
                             {"mantash-128.png", "mantash-macos-1024.png"})
            with Image.open(destination / "mantash.ico") as icon:
                self.assertEqual(icon.info["sizes"], {(n, n) for n in (16, 24, 32, 48, 64, 128, 256)})
            with Image.open(destination / "mantash.icns") as icon:
                self.assertEqual(icon.size, (1024, 1024))
            for name in ("mantash.svg", "mantash-monochrome.svg", "mantash-black.svg",
                         "mantash-white.svg", "mantash-mark.svg"):
                self.assertTrue((destination / name).is_file(), name)


if __name__ == "__main__":
    unittest.main()
