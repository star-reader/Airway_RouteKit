#!/bin/sh
set -eu
if [ "$#" -ne 3 ]; then
    echo 'Usage: install.sh TARGET LIBS_DIR PREFIX' >&2
    exit 2
fi
relative=$(sh scripts/library-path.sh "$1")
libs=$2
prefix=$3
[ -n "$prefix" ] || { echo 'An installation prefix is required' >&2; exit 2; }
for source in "$libs/$relative" "$libs/routekit.h"; do
    [ -s "$source" ] || { echo "Missing installation input: $source" >&2; exit 1; }
done
mkdir -p "$prefix/lib" "$prefix/include"
destination="$prefix/lib/$(basename "$relative")"
temporary="$destination.tmp.$$"
trap 'rm -f "$temporary"' EXIT
trap 'exit 1' HUP INT TERM
cp "$libs/$relative" "$temporary"
mv -f "$temporary" "$destination"
cp "$libs/routekit.h" "$prefix/include/"
printf 'Installed shared library and header under %s\n' "$prefix"
