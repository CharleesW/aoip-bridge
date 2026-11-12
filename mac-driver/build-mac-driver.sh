#!/bin/bash
# Compila o driver de audio virtual do macOS e monta AoIPBridge.driver em dist/.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
cargo build --release -p aoip-virtual -p aoip-bridge
VERSION="0.3"
OUT="dist/AoIP-Bridge-Mac/AoIPBridge.driver"
rm -rf "$OUT" && mkdir -p "$OUT/Contents/MacOS"
cp mac-driver/Info.plist "$OUT/Contents/Info.plist"
clang -O2 -Wall -Wno-unused-parameter -bundle -mmacosx-version-min=10.15 \
    -o "$OUT/Contents/MacOS/AoIPBridge" mac-driver/driver.c -framework CoreAudio -framework CoreFoundation
codesign --force --sign - "$OUT" >/dev/null 2>&1
clang -O2 -Wall -o target/release/hal_test mac-driver/hal_test.c -framework CoreAudio -framework CoreFoundation

APP="dist/AoIP-Bridge-Mac/AoIP Bridge.app"
rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS"
cp target/release/aoip-bridge "$APP/Contents/MacOS/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>AoIP Bridge</string>
  <key>CFBundleIdentifier</key><string>org.aoipbridge.app</string>
  <key>CFBundleExecutable</key><string>aoip-bridge</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>LSUIElement</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>O AoIP Bridge precisa ler as entradas da placa de audio para envia-las pela rede.</string>
  <key>NSLocalNetworkUsageDescription</key><string>O AoIP Bridge envia e recebe audio na rede local.</string>
</dict></plist>
PLIST
cp target/release/aoip-virtual "$APP/Contents/MacOS/"
codesign --force --sign - "$APP/Contents/MacOS/aoip-virtual"
codesign --force --sign - "$APP"
cp mac/Instalar.command mac/Desinstalar.command mac/LEIA-ME.txt dist/AoIP-Bridge-Mac/
(cd dist && rm -f "AoIP-Bridge-Mac-$VERSION-teste.zip" && zip -q -r -y "AoIP-Bridge-Mac-$VERSION-teste.zip" AoIP-Bridge-Mac)
echo "pronto: dist/AoIP-Bridge-Mac-$VERSION-teste.zip"
