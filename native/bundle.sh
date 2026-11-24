#!/bin/sh
# Builds "Testnet Monitor.app" (macOS) next to the cargo target directory and signs it ad hoc.
#
#   ./bundle.sh            # release build, then the .app
#   ./bundle.sh --open     # ... and open it
#
# Ad-hoc signing lets it run on the Mac that built it. Handing it to other Macs needs a
# Developer ID signature and notarization, which this script doesn't do.
set -eu
cd "$(dirname "$0")"

[ "$(uname)" = Darwin ] || { echo "bundle.sh only builds the macOS app" >&2; exit 1; }

cargo build --release
target=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app="$target/release/Testnet Monitor.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$target/release/chia-monitor-native" "$app/Contents/MacOS/"
sed "s/@VERSION@/$version/g" macos/Info.plist.in > "$app/Contents/Info.plist"

# AppIcon.icns from the 1024 px master
iconset=$(mktemp -d)/AppIcon.iconset
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
  sips -z $s $s macos/icon-1024.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) macos/icon-1024.png --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$iconset")"

codesign --force --deep --sign - "$app"
echo "Built: $app"
[ "${1:-}" = "--open" ] && open "$app"
exit 0
