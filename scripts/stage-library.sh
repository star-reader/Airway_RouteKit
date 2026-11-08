#!/bin/sh
set -eu
if [ "$#" -ne 3 ]; then
    echo 'Usage: stage-library.sh TARGET RELEASE_DIR LIBS_DIR' >&2
    exit 2
fi
target=$1
release=$2
libs=$3
relative=$(sh scripts/library-path.sh "$target")
name=$(basename "$relative")
source="$release/$name"
[ -s "$source" ] || { echo "Missing library: $source" >&2; exit 1; }
destination="$libs/$relative"
mkdir -p "$(dirname "$destination")"
temporary="$destination.tmp.$$"
trap 'rm -f "$temporary"' EXIT
trap 'exit 1' HUP INT TERM
cp "$source" "$temporary"
case "$target" in
    *-apple-darwin)
        # A copied library must not retain a build-machine absolute install name.
        install_name_tool -id @rpath/libroutekit.dylib "$temporary"
        codesign --force --sign - --identifier libroutekit "$temporary"
        ;;
esac
mv -f "$temporary" "$destination"
printf 'Staged %s\n' "$destination"
