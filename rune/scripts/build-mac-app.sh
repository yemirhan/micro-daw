#!/usr/bin/env bash
# Builds the Rust app's macOS bundle: `Micro DAW.app` (phase 10, docs/rune-port/phase-10-app-shell.md).
#
#   rune/scripts/build-mac-app.sh [out dir]        # default: rune/dist
#
# Release build of `mdaw-app` (respects CARGO_TARGET_DIR), the icon rendered from the app's own
# `app-icon` scenes (matted to a transparent PNG, then iconutil), Info.plist with the version of
# the workspace, everything else (fonts, templates, catalog, factory sounds) is compiled into the
# binary. Signing: ad-hoc by default; MAC_APP_SIGN="Developer ID Application: …" signs with a
# real identity and the hardened runtime (notarizing is a manual step, see the phase doc).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/dist}"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
NAME="Micro DAW"
BUNDLE_ID="com.yemirhan.micro-daw"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

echo "[mac-app] cargo build --release -p mdaw-app (version $VERSION)"
cargo build --release -p mdaw-app --manifest-path "$ROOT/Cargo.toml"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
# A private copy: the target folder may be shared with other checkouts.
cp "$TARGET/release/micro-daw" "$WORK/micro-daw"

APP="$OUT/$NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$WORK/micro-daw" "$APP/Contents/MacOS/micro-daw"

echo "[mac-app] icon"
"$WORK/micro-daw" --snap "$WORK/icon-black.png" --scene app-icon --size 1024x1024 --scale 1 >/dev/null 2>&1
"$WORK/micro-daw" --snap "$WORK/icon-white.png" --scene app-icon-white --size 1024x1024 --scale 1 >/dev/null 2>&1
python3 -I "$ROOT/scripts/icon-matte.py" "$WORK/icon-black.png" "$WORK/icon-white.png" "$WORK/icon.png"
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$WORK/icon.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" "$WORK/icon.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>micro-daw</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSHumanReadableCopyright</key><string>MIT License</string>
  <key>NSMicrophoneUsageDescription</key><string>Micro DAW uses audio devices to play and record your music.</string>
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist" >/dev/null

if [ -n "${MAC_APP_SIGN:-}" ]; then
  echo "[mac-app] codesign with \"$MAC_APP_SIGN\" (hardened runtime)"
  codesign --force --timestamp --options runtime \
    --entitlements "$ROOT/scripts/mac-entitlements.plist" --sign "$MAC_APP_SIGN" "$APP"
else
  echo "[mac-app] ad-hoc codesign (not notarizable; see the phase doc for Developer ID)"
  codesign --force --sign - "$APP"
fi
codesign --verify --strict "$APP"
echo "[mac-app] $APP"
