@echo off
chcp 65001 >nul
net session >nul 2>&1
if errorlevel 1 (
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
set "CLSID={BA273706-2FFA-4FAD-A237-B2BE6CA92823}"
taskkill /IM aoip-bridge.exe /F >nul 2>&1
reg delete "HKLM\SOFTWARE\ASIO\AoIP Bridge ASIO" /f /reg:64 >nul 2>&1
netsh advfirewall firewall delete rule name="AoIP Bridge - audio em rede" >nul 2>&1
reg delete "HKLM\SOFTWARE\Classes\CLSID\%CLSID%" /f /reg:64 >nul 2>&1
del "%PUBLIC%\Desktop\AoIP Bridge.lnk" >nul 2>&1
del "%ProgramData%\Microsoft\Windows\Start Menu\Programs\AoIP Bridge.lnk" >nul 2>&1
echo AoIP Bridge removido (feche o REAPER antes, se estiver aberto).
echo As configuracoes em %USERPROFILE% (.aoip-bridge.conf e .aoip-asio.conf) foram mantidas.
cd /d "%TEMP%"
start "" cmd /c "timeout /t 1 >nul & rmdir /s /q "%ProgramFiles%\AoIP Bridge""
pause
