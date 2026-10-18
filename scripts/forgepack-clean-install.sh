#!/usr/bin/env bash
# Linux/macOS counterpart of scripts/forgepack-clean-install.ps1: copy a built
# fgx into a fresh directory and prove the packaged forgepack workflow from
# there.
#
# Usage: scripts/forgepack-clean-install.sh [path/to/fgx]
set -euo pipefail

fgx_path="${1:-target/release/fgx}"
resolved="$(realpath "$fgx_path")"
# mktemp gives a fresh, unpredictable directory, so nothing else on the host
# can pre-create or reuse it.
install_root="$(mktemp -d "${TMPDIR:-/tmp}/forgentex-forgepack-clean-install.XXXXXX")"
trap 'rm -rf "$install_root"' EXIT

installed="$install_root/$(basename "$resolved")"
cp "$resolved" "$installed"
if ! "$installed" forgepack evidence --strict --json; then
    echo "installed fgx failed forgepack workflow evidence" >&2
    exit 1
fi
