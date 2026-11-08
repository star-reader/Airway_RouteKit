#!/bin/sh
set -eu
tool=$1
if ! command -v zig >/dev/null 2>&1; then
    echo "Zig is required for Linux cross-builds." >&2
    exit 1
fi
if ! version=$("$tool" --version 2>/dev/null); then
    echo "cargo-zigbuild is missing. Run make setup-cross first." >&2
    exit 1
fi
if ! printf '%s\n' "$version" | awk '$1 == "cargo-zigbuild" { split($2, v, "."); ok = v[1] > 0 || (v[1] == 0 && v[2] >= 23) } END { exit !ok }'; then
    echo "cargo-zigbuild >= 0.23.0 is required with current Rust linkers; found: $version" >&2
    echo "Run make setup-cross for a project-local installation." >&2
    exit 1
fi
