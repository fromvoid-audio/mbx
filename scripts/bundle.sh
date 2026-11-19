#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export VST3_SDK_DIR="${VST3_SDK_DIR:-$HOME/lib/vst3sdk}"

cargo build --locked --release
version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml)"
mkdir -p dist
stage="$(mktemp -d "$PWD/dist/.mbx.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
bundle="$stage/MBX.vst3"
mkdir -p "$bundle/Contents/MacOS"
cp target/release/libmbx.dylib "$bundle/Contents/MacOS/MBX"
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>MBX</string>
    <key>CFBundleIdentifier</key><string>com.portalsurfer.mbx.vst3</string>
    <key>CFBundleName</key><string>MBX</string>
    <key>CFBundlePackageType</key><string>BNDL</string>
    <key>CFBundleShortVersionString</key><string>${version}</string>
    <key>CFBundleVersion</key><string>${version}</string>
</dict>
</plist>
PLIST
printf 'BNDL????' > "$bundle/Contents/PkgInfo"
plutil -lint "$bundle/Contents/Info.plist"
codesign --sign - --force "$bundle"
codesign --verify --strict "$bundle"
rm -rf dist/MBX.vst3
mv "$bundle" dist/MBX.vst3
printf '%s\n' "$PWD/dist/MBX.vst3"
