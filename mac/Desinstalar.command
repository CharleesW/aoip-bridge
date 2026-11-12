#!/bin/bash
echo "== AoIP Bridge: remover do Mac =="
echo "O Mac vai pedir a sua senha de administrador."
sudo rm -rf "/Library/Audio/Plug-Ins/HAL/AoIPBridge.driver" || exit 1
launchctl bootout "gui/$(id -u)/org.aoipbridge.virtual" 2>/dev/null
rm -f "$HOME/Library/LaunchAgents/org.aoipbridge.virtual.plist"
pkill -x aoip-virtual 2>/dev/null
pkill -x aoip-bridge 2>/dev/null
rm -rf "$HOME/Applications/AoIP Bridge.app"
sudo killall coreaudiod 2>/dev/null || sudo launchctl kickstart -k system/com.apple.audio.coreaudiod
echo "Removido. As configuracoes ficaram em ~/.aoip-coreaudio.conf e ~/.aoip-bridge.conf (pode apagar)."
