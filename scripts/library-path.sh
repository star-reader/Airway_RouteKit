#!/bin/sh
# Shared-library paths by target architecture.
set -eu
if [ "$#" -ne 1 ]; then
    echo 'Usage: library-path.sh TARGET' >&2
    exit 2
fi
case "$1" in
    *-apple-darwin) echo macos/libroutekit.dylib ;;
    x86_64-unknown-linux-gnu) echo linux/amd64/libroutekit.so ;;
    aarch64-unknown-linux-gnu) echo linux/arm64/libroutekit.so ;;
    x86_64-pc-windows-*) echo windows/amd64/routekit.dll ;;
    aarch64-pc-windows-*) echo windows/arm64/routekit.dll ;;
    *) echo "Unsupported distribution target: $1" >&2; exit 2 ;;
esac
