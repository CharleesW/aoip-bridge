#!/bin/bash
# Compila o driver de audio virtual do macOS e monta AoIPBridge.driver em dist/.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
cargo build --release -p aoip-virtual -p aoip-bridge
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)   # [workspace.package]
OUT="dist/AoIP-Bridge-Mac/AoIPBridge.driver"
rm -rf "$OUT" && mkdir -p "$OUT/Contents/MacOS"
cp mac-driver/Info.plist "$OUT/Contents/Info.plist"
clang -O2 -Wall -Wno-unused-parameter -bundle -mmacosx-version-min=10.15 \
    -o "$OUT/Contents/MacOS/AoIPBridge" mac-driver/driver.c -framework CoreAudio -framework CoreFoundation
codesign --force --sign - "$OUT" >/dev/null 2>&1
clang -O2 -Wall -o target/release/hal_test mac-driver/hal_test.c -framework CoreAudio -framework CoreFoundation

# O programa: uma janela propria (janela.m). Dentro dele vao o auxiliar sempre ligado
# (aoip-virtual) e a mesa (um aplicativo a parte, por causa da permissao de microfone).
APP="dist/AoIP-Bridge-Mac/AoIP Bridge.app"
MESA="$APP/Contents/Helpers/AoIP Mesa.app"
rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$MESA/Contents/MacOS"
plist() { # nome, identificador, executavel, extras
cat <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$1</string>
  <key>CFBundleIdentifier</key><string>$2</string>
  <key>CFBundleExecutable</key><string>$3</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSLocalNetworkUsageDescription</key><string>O AoIP Bridge envia e recebe audio na rede local.</string>
  $4
</dict></plist>
PLIST
}
plist "AOIP-Bridge" org.aoipbridge.app aoip-janela \
  '<key>CFBundleIconFile</key><string>icone</string><key>NSHighResolutionCapable</key><true/><key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>' > "$APP/Contents/Info.plist"
plist "AoIP Mesa" org.aoipbridge.mesa aoip-bridge \
  '<key>LSUIElement</key><true/><key>NSMicrophoneUsageDescription</key><string>O AoIP Bridge precisa ler as entradas da placa de audio para envia-las pela rede.</string>' > "$MESA/Contents/Info.plist"
clang -O2 -Wall -fobjc-arc -mmacosx-version-min=10.15 -o "$APP/Contents/MacOS/aoip-janela" mac-driver/app/janela.m -framework Cocoa -framework WebKit
cp mac-driver/app/icone.icns "$APP/Contents/Resources/"
cp target/release/aoip-virtual "$APP/Contents/MacOS/"
cp target/release/aoip-bridge "$MESA/Contents/MacOS/"
codesign --force --sign - "$MESA"
codesign --force --sign - "$APP/Contents/MacOS/aoip-virtual"
codesign --force --sign - "$APP"
cp mac/Instalar.command mac/Desinstalar.command mac/LEIA-ME.txt dist/AoIP-Bridge-Mac/
(cd dist && rm -f "AoIP-Bridge-Mac-$VERSION-teste.zip" && zip -q -r -y "AoIP-Bridge-Mac-$VERSION-teste.zip" AoIP-Bridge-Mac)
echo "pronto: dist/AoIP-Bridge-Mac-$VERSION-teste.zip"
