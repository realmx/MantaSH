#!/usr/bin/env python3
"""Check real native chart pixels against the QA state's monitor sample, not just outer card bounds."""
import argparse
from io import BytesIO
import json
from pathlib import Path

from PIL import Image, ImageCms


def near(pixel, color):
    """Allow small screenshot profile rounding while excluding text and hover backgrounds."""
    return all(abs(channel - expected) <= 4 for channel, expected in zip(pixel, color))


def usage_color(percent, night):
    """Expected continuous color at a measured percentage, independently checked against native pixels."""
    green, blue, red = ((96, 200, 148), (124, 165, 239), (240, 132, 132)) if night else ((50, 168, 117), (75, 130, 227), (228, 97, 97))
    if percent < 25:
        first, last, amount = green, blue, percent / 25
    elif percent > 85:
        first, last, amount = blue, red, (percent - 85) / 15
    else:
        return blue
    return tuple(round(a + (b - a) * amount) for a, b in zip(first, last))


def meter_pixels(image, bounds, scale, accent, track, ordinal=0, fill_colors=None):
    """Locate a solid meter band; ignore thin dividers and select RAM or Swap by vertical order."""
    left, top = bounds["x"], bounds["y"]
    right, bottom = left + bounds["width"], top + bounds["height"]
    assert 0 <= top < bottom <= image.height / scale + 1, bounds
    bands = []
    band = None
    colors = fill_colors or [accent]
    thickness = max(1, round(2 * scale))
    for y in range(round((top + 5) * scale), round((bottom - 5) * scale)):
        best = (0, 0, 0, y)
        length = filled = 0
        for x in range(round((left + 5) * scale), round((right - 5) * scale)):
            pixel = image.getpixel((x, y))
            is_fill = any(near(pixel, color) for color in colors)
            solid = (is_fill or near(pixel, track)) and all(any(near(image.getpixel((x, y + delta)), color) for color in colors) or near(image.getpixel((x, y + delta)), track)
                        for delta in [-thickness, thickness])
            if (is_fill or near(pixel, track)) and solid:
                length += 1
                filled += int(is_fill)
                if length > best[0]:
                    best = length, filled, x - length + 1, y
            else:
                length = filled = 0
        if best[0] >= 40 * scale:
            if band is None:
                band = best
            elif best[0] > band[0]:
                band = best
        elif band is not None:
            bands.append(band)
            band = None
    if band is not None:
        bands.append(band)
    assert len(bands) > ordinal, f"Missing meter band {ordinal}: {bounds}"
    length, filled, start, row = bands[ordinal]
    if filled:
        actual = image.getpixel((start + filled // 2, row))
        assert near(actual, accent), f"Wrong utilization color: {actual}, expected {accent}"
    return {"track_width": length / scale, "filled_percent": 100 * filled / length,
            "left_inset": start / scale - left, "right_inset": right - (start + length) / scale}


def check(image_path, state_path):
    """Require visible proportional CPU/memory/disk meters and a painted network curve."""
    state = json.loads(state_path.read_text())
    with Image.open(image_path) as source:
        profile = source.info.get("icc_profile")
        image = ImageCms.profileToProfile(source, ImageCms.ImageCmsProfile(BytesIO(profile)),
                                         ImageCms.createProfile("sRGB"), outputMode="RGB") if profile else source.convert("RGB")
    scale = image.width / state["width"]
    assert abs(image.height / scale - state["height"]) < 1, "Image and layout dimensions differ"
    night = state["preferences"]["theme"] == "night"
    accent = (162, 186, 255) if night else (59, 95, 192)
    track = (51, 60, 75) if night else (225, 229, 235)
    tab = state["tabs"][state["active_tab"]]
    sample = tab["panes"][tab["active_pane"]]["monitor"]
    memory = sample["memory"]
    disk = next((disk for disk in sample["disks"] if disk["mount"] == "/"), sample["disks"][0])
    expected = {"cpu": sample["cpu"][0]["percent"],
                "memory": 100 * (memory["total"] - memory["available"]) / memory["total"],
                "disk": 100 * disk["used"] / disk["total"]}
    meters = {}
    swap = memory["swap_total"] - memory["swap_free"]
    memory_colors = [usage_color(expected["memory"], night), usage_color(100 * swap / memory["swap_total"] if memory["swap_total"] else 0, night)]
    for key, value in expected.items():
        color = usage_color(value or 0, night)
        result = meter_pixels(image, state["overview"]["bounds"][key], scale, color, track, fill_colors=memory_colors if key == "memory" else None)
        assert result["track_width"] >= 40, f"{key}: missing or collapsed native track: {result}"
        assert abs(result["filled_percent"] - (value or 0)) < 1, f"{key}: fill does not match sample: {result}, expected {value}"
        meters[key] = {**result, "expected_color": color}
    if memory["swap_total"]:
        color = usage_color(100 * swap / memory["swap_total"], night)
        result = meter_pixels(image, state["overview"]["bounds"]["memory"], scale, color, track, ordinal=1, fill_colors=memory_colors)
        assert abs(result["filled_percent"] - 100 * swap / memory["swap_total"]) < 1, result
        meters["swap"] = {**result, "expected_color": color}
    assert all(abs(meter["left_inset"] - meter["right_inset"]) < 1 for meter in meters.values()), meters
    network = state["overview"]["network_plot"]
    # Use the actual prepaint plot bounds, excluding speed labels and cumulative traffic text.
    crop = image.crop((round((network["x"] + network["width"] * .1) * scale),
                       round(network["y"] * scale),
                       round((network["x"] + network["width"] * .9) * scale),
                       round((network["y"] + network["height"]) * scale)))
    curve_pixels = sum(near(pixel, accent) for pixel in crop.get_flattened_data())
    assert curve_pixels > 20 * scale, "Network plot has no painted receive curve"
    sent = (139, 210, 187) if night else (39, 116, 103)
    sent_curve_pixels = sum(near(pixel, sent) for pixel in crop.get_flattened_data())
    assert sent_curve_pixels > 10 * scale, "Network plot has no painted upload curve"
    return {"image": str(image_path), "state": str(state_path), "meters": meters,
            "network_curve_pixels": curve_pixels, "sent_curve_pixels": sent_curve_pixels, "passed": True}


def main():
    """Validate one captured WindowServer image with its matching isolated native state."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(check(args.image, args.state)))


if __name__ == "__main__":
    main()
