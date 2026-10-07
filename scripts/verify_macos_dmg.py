#!/usr/bin/env python3
"""Verify a macOS disk image, retrying only temporary hdiutil resource errors."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys
import time

RETRY_DELAYS = (2, 4, 8, 16)
TRANSIENT_ERRORS = ("resource temporarily unavailable", "resource busy")


def verify(image: Path) -> None:
    """Require successful image verification; permanent errors and exhausted retries fail."""
    command = ["/usr/bin/hdiutil", "verify", str(image.resolve())]
    # Match the system error text consistently on runners with different locales.
    environment = {**os.environ, "LC_ALL": "C"}
    attempts = len(RETRY_DELAYS) + 1
    for attempt in range(attempts):
        result = subprocess.run(command, capture_output=True, text=True,
                                encoding="utf-8", errors="replace", env=environment)
        print(result.stdout, end="", flush=True)
        print(result.stderr, end="", file=sys.stderr, flush=True)
        if result.returncode == 0:
            return
        diagnostic = (result.stdout + result.stderr).casefold()
        transient = any(message in diagnostic for message in TRANSIENT_ERRORS)
        if not transient or attempt == attempts - 1:
            result.check_returncode()
        delay = RETRY_DELAYS[attempt]
        print(f"hdiutil verify: temporary resource error on attempt {attempt + 1}/{attempts}; "
              f"retrying in {delay}s", file=sys.stderr, flush=True)
        time.sleep(delay)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path, help="Disk image to verify before mounting or publishing")
    args = parser.parse_args(argv)
    if not args.image.is_file():
        parser.error(f"Disk image does not exist: {args.image}")
    try:
        verify(args.image)
    except subprocess.CalledProcessError as error:
        return error.returncode if error.returncode > 0 else 1
    except OSError as error:
        print(f"Unable to run hdiutil verify: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
