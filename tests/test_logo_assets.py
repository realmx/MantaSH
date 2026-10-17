"""Check reproducible brand assets and every shipped macOS icon representation."""
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
                                     ((16, 16), (27, 27), (42, 42), (255, 255)))

    def test_macos_generation_matches_shipped_icns_and_keeps_other_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "assets"
            destination.mkdir()
            other = destination / "mantash.ico"
            other.write_bytes(b"unrelated platform asset")
            with patch.object(generate_logo, "ROOT", root), \
                    patch.object(sys, "argv", ["generate_logo.py", "--macos-only"]):
                generate_logo.main()
            self.assertEqual(other.read_bytes(), b"unrelated platform asset")
            self.assertEqual({path.name for path in destination.iterdir()},
                             {"mantash.ico", "mantash.icns"})
            with Image.open(destination / "mantash.icns") as generated, \
                    Image.open(ROOT / "assets/mantash.icns") as shipped:
                sizes = generated.info["sizes"]
                self.assertEqual(sizes, shipped.info["sizes"])
            for width, height, scale in sizes:
                with self.subTest(size=(width, height, scale)), \
                        Image.open(destination / "mantash.icns") as generated, \
                        Image.open(ROOT / "assets/mantash.icns") as shipped:
                    generated.size = shipped.size = (width, height)
                    generated.load(scale=scale)
                    shipped.load(scale=scale)
                    self.assertEqual(generated.convert("RGBA").tobytes(),
                                     shipped.convert("RGBA").tobytes())

    def test_full_generation_keeps_consumed_assets_and_embedded_icon_sizes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "assets"
            destination.mkdir()
            with patch.object(generate_logo, "ROOT", root), \
                    patch.object(sys, "argv", ["generate_logo.py"]):
                generate_logo.main()
            self.assertEqual({path.name for path in destination.iterdir()},
                             {"mantash-monochrome.svg", "mantash-128.png",
                              "mantash.ico", "mantash.icns"})
            with Image.open(destination / "mantash.ico") as icon:
                self.assertEqual(icon.info["sizes"], {(n, n) for n in (16, 24, 32, 48, 64, 128, 256)})
            with Image.open(destination / "mantash.icns") as icon:
                self.assertEqual(icon.size, (1024, 1024))
            self.assertEqual((destination / "mantash-monochrome.svg").read_bytes(),
                             (ROOT / "assets/mantash-monochrome.svg").read_bytes())
            for name in ("mantash-128.png", "mantash.ico"):
                with Image.open(destination / name) as generated, \
                        Image.open(ROOT / "assets" / name) as shipped:
                    self.assertEqual(generated.size, shipped.size, name)
                    self.assertEqual(generated.convert("RGBA").tobytes(),
                                     shipped.convert("RGBA").tobytes(), name)


if __name__ == "__main__":
    unittest.main()
