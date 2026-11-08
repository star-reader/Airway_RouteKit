#!/bin/sh
set -eu
if [ "$#" -ne 4 ]; then
    echo 'Usage: package.sh LIBS_DIR DIST_DIR VERSION all|TARGET' >&2
    exit 2
fi
libs=$1
dist=$2
version=$3
mode=$4
case "$version" in ''|*[!A-Za-z0-9._-]*) echo 'Invalid package version' >&2; exit 2 ;; esac
name="routekit-$version"
if [ "$mode" = all ]; then
    files='macos/libroutekit.dylib linux/amd64/libroutekit.so linux/arm64/libroutekit.so'
else
    files=$(sh scripts/library-path.sh "$mode")
    name="$name-$mode"
fi
# Validate before making or replacing an archive. Never include navigation data.
for path in $files routekit.h; do
    [ -s "$libs/$path" ] || { echo "Missing package input: $libs/$path" >&2; exit 1; }
done
cmp routekit.h "$libs/routekit.h"
mkdir -p "$dist"
staging=$(mktemp -d "$dist/.routekit.XXXXXX")
archive="$dist/$name.tar.gz"
partial="$archive.tmp.$$"
trap 'rm -rf "$staging"; rm -f "$partial"' EXIT
trap 'exit 1' HUP INT TERM
root="$staging/$name"
mkdir -p "$root/libs"
cp "$libs/routekit.h" LICENSE README.md "$root/"
for path in $files; do
    mkdir -p "$root/libs/$(dirname "$path")"
    cp "$libs/$path" "$root/libs/$path"
done
COPYFILE_DISABLE=1 tar -czf "$partial" -C "$staging" "$name"
mv -f "$partial" "$archive"
printf 'Packaged %s\n' "$archive"
