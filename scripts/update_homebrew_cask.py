#!/usr/bin/env python3
"""Write an optional Homebrew Cask from verified macOS release artifacts."""
from __future__ import annotations

import argparse
from pathlib import Path
import re

from package_release import digest


def checksum(assets: Path, version: str, arch: str) -> str:
    name = f"MantaSH-{version}-macos-{arch}.dmg"
    package = assets / name
    sidecar = assets / f"{name}.sha256"
    if not package.is_file() or not sidecar.is_file():
        raise ValueError(f"Missing release asset or checksum for {name}")
    expected = f"{digest(package)}  {name}\n"
    if sidecar.read_text(encoding="ascii") != expected:
        raise ValueError(f"Invalid release checksum for {name}")
    return expected.split()[0]


def render(version: str, arm: str, intel: str) -> str:
    return f'''cask "mantash" do
  version "{version}"

  on_arm do
    sha256 "{arm}"
    url "https://github.com/realmx/MantaSH/releases/download/v#{{version}}/MantaSH-#{{version}}-macos-arm64.dmg"
  end

  on_intel do
    sha256 "{intel}"
    url "https://github.com/realmx/MantaSH/releases/download/v#{{version}}/MantaSH-#{{version}}-macos-x64.dmg"
  end

  name "MantaSH"
  desc "Native terminal workspace for local and SSH sessions"
  homepage "https://github.com/realmx/MantaSH"
  app "MantaSH.app"

  caveats <<~EOS
    MantaSH uses ad-hoc signing and is not Apple notarized. macOS may block its
    first launch; use the system Open Anyway control after verifying the download.
  EOS
end
'''


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--assets", type=Path, required=True)
    parser.add_argument("--tap", type=Path, required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.version):
        parser.error("--version must be X.Y.Z")
    arm = checksum(args.assets, args.version, "arm64")
    intel = checksum(args.assets, args.version, "x64")
    path = args.tap / "Casks/mantash.rb"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render(args.version, arm, intel), encoding="utf-8")
    print(f"Updated {path} for MantaSH v{args.version}")


if __name__ == "__main__":
    main()
