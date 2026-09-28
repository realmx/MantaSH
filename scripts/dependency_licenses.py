#!/usr/bin/env python3
"""Generate a reproducible license inventory from locked Cargo metadata supplied as a file."""
import csv
import json
from pathlib import Path
import sys


def main():
    """Keep package versions, license expressions and public source locations together."""
    metadata = json.loads(Path(sys.argv[1]).read_text())
    root = Path(__file__).resolve().parents[1]
    destination = root / "docs/dependency-licenses.csv"
    packages = sorted(metadata["packages"], key=lambda package: (package["name"], package["version"]))
    with destination.open("w", newline="", encoding="utf-8") as output:
        writer = csv.writer(output)
        writer.writerow(["package", "version", "license", "license_file", "repository", "source"])
        for package in packages:
            license_file = package.get("license_file")
            writer.writerow([package["name"], package["version"], package.get("license") or "See license_file", Path(license_file).name if license_file else "", package.get("repository") or "", package.get("source") or "MantaSH workspace"])
    print(f"Recorded {len(packages)} locked packages in {destination.relative_to(root)}")


if __name__ == "__main__":
    main()
