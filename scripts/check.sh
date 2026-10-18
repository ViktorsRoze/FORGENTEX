#!/usr/bin/env bash
# Linux/macOS counterpart of scripts/check.ps1: the full local gate.
# Stops at the first failing command.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy -- -D warnings
cargo test -- --test-threads=1
scripts/fuzz.sh
scripts/demo.sh
