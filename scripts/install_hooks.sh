#!/bin/sh
# Enable the repository hooks in .githooks for this clone (run once).
set -eu
root=$(git rev-parse --show-toplevel)
chmod +x "$root/.githooks/pre-commit"
git -C "$root" config core.hooksPath .githooks
echo "core.hooksPath=.githooks; the pre-commit hook now updates CHANGELOG.md."
