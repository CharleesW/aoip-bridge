#!/bin/bash
# Instala o AoIP Bridge no Mac: a placa virtual "AoIP Bridge" (32x32) e o programa.
cd "$(dirname "$0")" || exit 1
echo "== AoIP Bridge: instalacao no Mac =="
echo "O Mac vai pedir a sua senha de administrador (ela nao aparece enquanto voce digita)."
echo
HAL="/Library/Audio/Plug-Ins/HAL"
sudo mkdir -p "$HAL" || { echo "INSTALACAO CANCELADA."; exit 1; }
sudo rm -rf "$HAL/AoIPBridge.driver"
sudo cp -R "AoIPBridge.driver" "$HAL/" || { echo "FALHOU: nao consegui copiar o driver."; exit 1; }
sudo chown -R root:wheel "$HAL/AoIPBridge.driver"
sudo xattr -dr com.apple.quarantine "$HAL/AoIPBridge.driver" 2>/dev/null
mkdir -p "$HOME/Applications"
# Um programa so: guarda se a mesa (placa de verdade) estava ligada, para religar no fim.
MESA=""; pgrep -x aoip-bridge >/dev/null && MESA=1
pkill -x aoip-janela 2>/dev/null
pkill -x aoip-bridge 2>/dev/null
rm -rf "$HOME/Applications/AoIP Bridge.app"
cp -R "AoIP Bridge.app" "$HOME/Applications/" && xattr -dr com.apple.quarantine "$HOME/Applications/AoIP Bridge.app" 2>/dev/null

# Programa auxiliar da placa virtual (rede e painel): o Mac o mantem aberto sozinho, em segundo
# plano. Ele fica parado, sem mandar nada para a rede, ate algum programa usar a placa.
AGENT="$HOME/Library/LaunchAgents/org.aoipbridge.virtual.plist"
mkdir -p "$HOME/Library/LaunchAgents"
launchctl bootout "gui/$(id -u)/org.aoipbridge.virtual" 2>/dev/null
pkill -x aoip-virtual 2>/dev/null
cat > "$AGENT" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>org.aoipbridge.virtual</string>
  <key>ProgramArguments</key><array><string>$HOME/Applications/AoIP Bridge.app/Contents/MacOS/aoip-virtual</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>1</integer>
  <key>AbandonProcessGroup</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict></plist>
PLIST
launchctl bootstrap "gui/$(id -u)" "$AGENT" || echo "aviso: nao consegui ligar o programa auxiliar; reinicie a sessao do Mac."

echo "Reiniciando o sistema de audio do Mac (o som para por um instante)..."
sudo killall coreaudiod 2>/dev/null || sudo launchctl kickstart -k system/com.apple.audio.coreaudiod
sleep 3
echo
if system_profiler SPAudioDataType 2>/dev/null | grep -q "AoIP Bridge"; then
  echo "INSTALACAO OK. A placa \"AoIP Bridge\" ja aparece nos programas de audio."
else
  echo "Instalado, mas a placa ainda nao apareceu. Reinicie o Mac e confira em Configuracao de Audio MIDI."
fi
[ -n "$MESA" ] && curl -s -o /dev/null -X POST -H "X-AoIP: 1" "http://127.0.0.1:8788/api/app/mesa?on=1" && echo "A mesa estava ligada: liga de novo sozinha."
open "$HOME/Applications/AoIP Bridge.app"
echo "Abra o programa quando quiser em: pasta pessoal > Aplicativos > AoIP Bridge."
