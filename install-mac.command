#!/bin/bash
# Compila e instala o AoIP Bridge em ~/Applications. Tudo fica registrado em install.log.
cd "$(dirname "$0")" || exit 1
exec > >(tee install.log) 2>&1
echo "== AoIP Bridge: instalacao ($(date)) =="
fail() { echo; echo "INSTALACAO FALHOU: $1"; exit 1; }

if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install
  fail "faltam as Command Line Tools da Apple. Conclua a janela de instalacao que abriu e rode este arquivo de novo."
fi
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
if ! command -v cargo >/dev/null 2>&1; then
  echo "-- instalando o Rust (rustup.rs)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal || fail "nao consegui instalar o Rust"
  source "$HOME/.cargo/env"
fi
echo "-- $(cargo --version)"
echo "-- testando o nucleo"
cargo test -p aoip-core 2>&1 | grep -E "test result|FAILED|error" 
echo "-- compilando (alguns minutos na primeira vez)"
cargo build --release -p aoip-bridge || fail "erro de compilacao (veja acima)"

APP="$HOME/Applications/AoIP Bridge.app"
pkill -x aoip-bridge 2>/dev/null
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp target/release/aoip-bridge "$APP/Contents/MacOS/" || fail "nao consegui copiar o programa"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>AoIP Bridge</string>
  <key>CFBundleIdentifier</key><string>org.aoipbridge.app</string>
  <key>CFBundleExecutable</key><string>aoip-bridge</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.2.0</string>
  <key>LSUIElement</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>O AoIP Bridge precisa ler as entradas da placa de audio para envia-las pela rede.</string>
  <key>NSLocalNetworkUsageDescription</key><string>O AoIP Bridge envia e recebe audio na rede local.</string>
</dict></plist>
PLIST
codesign --force -s - "$APP" || echo "aviso: nao consegui assinar o app"
open "$APP"
echo
echo "INSTALACAO OK: $APP"
echo "O painel abre no navegador em http://127.0.0.1:8787"
