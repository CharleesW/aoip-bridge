@echo off
chcp 65001 >nul
rem Precisa de administrador para registrar o driver ASIO: pede a elevacao sozinho.
net session >nul 2>&1
if errorlevel 1 (
  echo Pedindo permissao de administrador...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
setlocal
set "DEST=%ProgramFiles%\AoIP Bridge"
set "CLSID={BA273706-2FFA-4FAD-A237-B2BE6CA92823}"
echo == AoIP Bridge: instalacao ==
rem Um programa so: se a mesa (placa de verdade) estava ligada, volta ligada no fim.
set "MESA="
tasklist /FI "IMAGENAME eq aoip-bridge.exe" 2>nul | find /I "aoip-bridge.exe" >nul && set "MESA=1"
echo %* | find "--abrir-bridge" >nul && set "MESA=1"
taskkill /IM aoip-winvirtual.exe /F >nul 2>&1
taskkill /IM aoip-bridge.exe /F >nul 2>&1
taskkill /IM aoip-winvirtual.exe /F >nul 2>&1
if not exist "%DEST%" mkdir "%DEST%"
copy /Y "%~dp0aoip_asio.dll" "%DEST%\" >nul
if errorlevel 1 (
  echo.
  echo INSTALACAO FALHOU: nao consegui gravar o driver em "%DEST%".
  echo Feche o REAPER e qualquer programa de audio e rode de novo.
  pause
  exit /b 1
)
copy /Y "%~dp0aoip-bridge.exe" "%DEST%\" >nul
copy /Y "%~dp0aoip-winvirtual.exe" "%DEST%\" >nul
copy /Y "%~dp0Autoteste.bat" "%DEST%\" >nul
copy /Y "%~dp0Desinstalar.bat" "%DEST%\" >nul
copy /Y "%~dp0LEIA-ME.txt" "%DEST%\" >nul

rem Registro do driver ASIO (programas de 64 bits).
reg add "HKLM\SOFTWARE\Classes\CLSID\%CLSID%" /ve /d "AoIP Bridge ASIO" /f /reg:64 >nul
reg add "HKLM\SOFTWARE\Classes\CLSID\%CLSID%\InprocServer32" /ve /d "%DEST%\aoip_asio.dll" /f /reg:64 >nul
reg add "HKLM\SOFTWARE\Classes\CLSID\%CLSID%\InprocServer32" /v ThreadingModel /d "Apartment" /f /reg:64 >nul
reg add "HKLM\SOFTWARE\ASIO\AoIP Bridge ASIO" /v CLSID /d "%CLSID%" /f /reg:64 >nul
reg add "HKLM\SOFTWARE\ASIO\AoIP Bridge ASIO" /v Description /d "AoIP Bridge ASIO" /f /reg:64 >nul
if errorlevel 1 (
  echo.
  echo INSTALACAO FALHOU: nao consegui registrar o driver ASIO.
  pause
  exit /b 1
)

rem Firewall: libera a chegada do audio em rede (relogio PTP 319/320, fluxos 5004, anuncios 9875)
rem em qualquer tipo de rede. Sem isso o Windows bloqueia em silencio quando a rede e "Publica"
rem (ex.: a rede da igreja), mesmo que o REAPER tenha sido liberado em casa.
netsh advfirewall firewall delete rule name="AoIP Bridge - audio em rede" >nul 2>&1
netsh advfirewall firewall add rule name="AoIP Bridge - audio em rede" dir=in action=allow protocol=UDP localport=319,320,5004,8793,9875 profile=any >nul
if errorlevel 1 echo AVISO: nao consegui liberar o Firewall. Se o audio nao chegar, libere as portas UDP 319, 320, 5004, 8793 e 9875.
rem Controller: comandos de roteamento vindos de outros paineis da rede (TCP 8791 e 8792).
netsh advfirewall firewall delete rule name="AoIP Bridge - Controller" >nul 2>&1
netsh advfirewall firewall add rule name="AoIP Bridge - Controller" dir=in action=allow protocol=TCP localport=8791,8792 profile=any remoteip=localsubnet >nul

rem Remove a instalacao antiga (versao 0.2, na pasta do usuario).
if exist "%LOCALAPPDATA%\AoIP Bridge" rmdir /s /q "%LOCALAPPDATA%\AoIP Bridge"
powershell -NoProfile -Command "$s=New-Object -ComObject WScript.Shell; foreach($d in @([Environment]::GetFolderPath('CommonDesktopDirectory'),[Environment]::GetFolderPath('CommonPrograms'))){ $l=$s.CreateShortcut((Join-Path $d 'AoIP Bridge.lnk')); $l.TargetPath=(Join-Path $env:ProgramFiles 'AoIP Bridge\aoip-winvirtual.exe'); $l.WorkingDirectory=(Join-Path $env:ProgramFiles 'AoIP Bridge'); $l.Save() }"
del "%USERPROFILE%\Desktop\AoIP Bridge.lnk" >nul 2>&1
rem Atalho antigo da placa virtual: agora e um programa so.
del "%ProgramData%\Microsoft\Windows\Start Menu\Programs\AoIP Virtual.lnk" >nul 2>&1
rem Usado pelo instalador para abrir o programa ja com a mesa (o explorer nao aceita argumentos).
powershell -NoProfile -Command "$s=New-Object -ComObject WScript.Shell; $l=$s.CreateShortcut((Join-Path $env:ProgramFiles 'AoIP Bridge\Abrir com a mesa.lnk')); $l.TargetPath=(Join-Path $env:ProgramFiles 'AoIP Bridge\aoip-winvirtual.exe'); $l.Arguments='--mesa'; $l.Save()"
reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v "AoIP Virtual" /d "\"%DEST%\aoip-winvirtual.exe\" --tray" /f >nul

echo.
rem Abre o programa como usuario comum (nao como administrador).
if defined MESA (
  start "" explorer.exe "%DEST%\Abrir com a mesa.lnk"
) else (
  start "" explorer.exe "%DEST%\aoip-winvirtual.exe"
)

echo INSTALACAO OK: %DEST%
echo.
echo O AoIP Bridge ja esta ligado: icone perto do relogio. Duplo clique no icone abre a janela.
echo O driver "AoIP Bridge ASIO" ja pode ser escolhido no REAPER:
echo   Options ^> Preferences ^> Audio ^> Device ^> Audio system: ASIO ^> ASIO Driver: AoIP Bridge ASIO
echo O botao "ASIO Configuration" do REAPER tambem abre a janela.
echo O Firewall do Windows ja foi liberado para o audio em rede e para o Controller.
echo.
rem Atualizacao automatica (--auto): fecha sozinho depois de alguns segundos.
echo %* | find "--auto" >nul && (timeout /t 8 >nul & exit /b 0)
pause
