#!/bin/bash
# Build a committed Git ref as a standalone app for this Mac, without publishing.
set -euo pipefail

if [[ $# -gt 1 || "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    echo "Usage: bash tools/build-macos-app.sh [git-ref] (default: HEAD)"
    exit 0
fi
if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "This script builds a local macOS app; run it on a Mac." >&2
    exit 1
fi

repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
commit="$(git rev-parse --verify "${1:-HEAD}^{commit}")"
work="$(mktemp -d "${TMPDIR:-/tmp}/localpaste-macos.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir "$work/source"
git archive --format=tar "$commit" | tar -xf - -C "$work/source"
source_dir="$(cd "$work/source" && pwd -P)"
# Read only the selected source, even when the checkout's manifest is unfinished.
# Keep this cwd so Cargo discovers the checkout's cache configuration as before.
target_dir="$(cargo metadata --locked --no-deps --format-version 1 \
    --manifest-path "$source_dir/Cargo.toml" |
    /usr/bin/plutil -extract target_directory raw -o - -)"
if [[ "$target_dir" == "$source_dir/target" ]]; then
    # Cargo's default follows the archived workspace; reuse a persistent cache.
    target_dir="$repo/target"
fi
cd "$source_dir"

package_id="$(cargo pkgid -p localpaste_gui)"
version="${package_id##*[@#]}"
bundle_version="${version%%[-+]*}"
host="$(rustc -vV | awk '/^host: / { print $2 }')"
cargo build -p localpaste_gui --bin localpaste-gui --release --locked \
    --target "$host" --target-dir "$target_dir"

app="$work/LocalPaste.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$target_dir/$host/release/localpaste-gui" "$app/Contents/MacOS/localpaste-gui"
cp LICENSE "$app/Contents/Resources/LICENSE"

iconset="$work/LocalPaste.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
    /usr/bin/sips -z "$size" "$size" assets/icons/desktop_icon.png \
        --out "$iconset/icon_${size}x${size}.png" >/dev/null
    doubled=$((size * 2))
    /usr/bin/sips -z "$doubled" "$doubled" assets/icons/desktop_icon.png \
        --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
/usr/bin/iconutil -c icns "$iconset" -o "$app/Contents/Resources/LocalPaste.icns"

config="packaging/macos/packager.json"
name="$(/usr/bin/plutil -extract productName raw -o - "$config")"
identifier="$(/usr/bin/plutil -extract identifier raw -o - "$config")"
minimum_os="$(/usr/bin/plutil -extract macos.minimumSystemVersion raw -o - "$config")"
plist="$app/Contents/Info.plist"
/usr/bin/plutil -create xml1 "$plist"
/usr/bin/plutil -insert CFBundleExecutable -string localpaste-gui "$plist"
/usr/bin/plutil -insert CFBundleIdentifier -string "$identifier" "$plist"
/usr/bin/plutil -insert CFBundleName -string "$name" "$plist"
/usr/bin/plutil -insert CFBundleDisplayName -string "$name" "$plist"
/usr/bin/plutil -insert CFBundlePackageType -string APPL "$plist"
/usr/bin/plutil -insert CFBundleShortVersionString -string "$bundle_version" "$plist"
/usr/bin/plutil -insert CFBundleVersion -string "$bundle_version" "$plist"
/usr/bin/plutil -insert CFBundleIconFile -string LocalPaste.icns "$plist"
/usr/bin/plutil -insert LSMinimumSystemVersion -string "$minimum_os" "$plist"
/usr/bin/plutil -insert NSHighResolutionCapable -bool true "$plist"

# An ad-hoc signature needs no Apple account or signing certificate.
/usr/bin/codesign --force --sign - "$app"
/usr/bin/codesign --verify --deep --strict "$app"
mkdir -p "$repo/dist/macos"
# A fresh directory keeps reruns from replacing an installed or existing bundle.
output="$(mktemp -d "$repo/dist/macos/${version}.XXXXXX")"
mv "$app" "$output/LocalPaste.app"
printf '\nBuilt %s\nDrag LocalPaste.app into Applications, then double-click it to open.\n' \
    "$output/LocalPaste.app"
