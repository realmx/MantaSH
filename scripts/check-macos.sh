#!/bin/sh
# Run the authorized debug checks from any working directory. No release packaging.
set -eu
cd "$(dirname "$0")/.."
cargo fmt --all --check
cargo test --locked --no-default-features
cargo check --locked
cargo build --locked
if [ "${1:-}" = "--run" ]; then
    exec ./target/debug/mantash
fi
