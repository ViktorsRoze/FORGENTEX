#!/usr/bin/env bash
# Linux/macOS counterpart of scripts/fuzz.ps1.
# Stops at the first failing command.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo run --bin fgx -- fuzz --iterations 32 --json
cargo run --bin fgx -- diff-test --json
