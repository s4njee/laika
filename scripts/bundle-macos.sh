#!/usr/bin/env bash
# Package target/release/laika as Laika.app plus a .zip and .dmg in dist/.
# Usage: scripts/bundle-macos.sh [version]   (default: Cargo package version)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="${1:-$(grep -m1 '^version' crates/laika-app/Cargo.toml | cut -d'"' -f2)}"
version="${version#v}"
arch="$(uname -m)"
bin="target/release/laika"
[ -x "$bin" ] || { echo "missing $bin — run: cargo build --release -p laika-app" >&2; exit 1; }

dist="$root/dist"
app="$dist/Laika.app"
rm -rf "$dist"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/fonts" "$app/Contents/Resources/samples"

cp "$bin" "$app/Contents/MacOS/laika"
cp assets/icon/Laika.icns "$app/Contents/Resources/Laika.icns"
cp assets/fonts/*.ttf "$app/Contents/Resources/fonts/"
# First-run sample photos (V32 "Try with sample photos").
cp fixtures/raw/*.NEF "$app/Contents/Resources/samples/"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Laika</string>
  <key>CFBundleDisplayName</key><string>Laika</string>
  <key>CFBundleIdentifier</key><string>dev.laika.Laika</string>
  <key>CFBundleExecutable</key><string>laika</string>
  <key>CFBundleIconFile</key><string>Laika</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPhotoLibraryUsageDescription</key><string>Laika can import photos from and add exports to your Photos library.</string>
  <key>NSAppleEventsUsageDescription</key><string>Laika talks to Photos to add and sync photos you choose.</string>
  <key>NSRemovableVolumesUsageDescription</key><string>Laika imports photos from memory cards.</string>
</dict>
</plist>
PLIST

# Ad-hoc signature (no Developer ID): required for arm64 binaries to launch.
codesign --force --deep --sign - "$app"
codesign --verify --deep "$app"

name="Laika-${version}-macos-${arch}"
ditto -c -k --keepParent "$app" "$dist/$name.zip"
hdiutil create -volname "Laika ${version}" -srcfolder "$app" -ov -format UDZO "$dist/$name.dmg" >/dev/null
( cd "$dist" && shasum -a 256 "$name.zip" "$name.dmg" > "$name.sha256" )
ls -la "$dist"
