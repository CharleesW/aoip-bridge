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
taskkill /IM aoip-bridge.exe /F >nul 2>&1
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

rem Remove a instalacao antiga (versao 0.2, na pasta do usuario).
if exist "%LOCALAPPDATA%\AoIP Bridge" rmdir /s /q "%LOCALAPPDATA%\AoIP Bridge"
powershell -NoProfile -Command "$s=New-Object -ComObject WScript.Shell; foreach($d in @([Environment]::GetFolderPath('CommonDesktopDirectory'),[Environment]::GetFolderPath('CommonPrograms'))){ $l=$s.CreateShortcut((Join-Path $d 'AoIP Bridge.lnk')); $l.TargetPath=(Join-Path $env:ProgramFiles 'AoIP Bridge\aoip-bridge.exe'); $l.WorkingDirectory=(Join-Path $env:ProgramFiles 'AoIP Bridge'); $l.Save() }"
del "%USERPROFILE%\Desktop\AoIP Bridge.lnk" >nul 2>&1

echo.
echo INSTALACAO OK: %DEST%
echo.
echo O driver "AoIP Bridge ASIO" ja pode ser escolhido no REAPER:
echo   Options ^> Preferences ^> Audio ^> Device ^> Audio system: ASIO ^> ASIO Driver: AoIP Bridge ASIO
echo O botao "ASIO Configuration" abre o painel do driver (http://127.0.0.1:8788).
echo Na primeira vez o Firewall do Windows vai perguntar sobre o REAPER: clique em Permitir.
echo.
pause
