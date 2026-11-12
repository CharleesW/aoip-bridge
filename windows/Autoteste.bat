@echo off
chcp 65001 >nul
echo == AoIP Bridge: AUTOTESTE ==
echo Um tom de teste entra nos canais 1 e 2 e o programa envia e recebe de si mesmo.
echo Nada sai para a rede. O som NAO vai para as saidas da placa, a menos que voce
echo ligue pontos na matriz "Receber da rede".
echo.
echo 1. No painel que vai abrir, escolha a placa (ex.: Ui24R) e clique em Salvar e iniciar.
echo 2. Em "Receber da rede", escolha o fluxo "Bridge : 1" na primeira posicao.
echo 3. Deixe rodar por 5 minutos e feche esta janela.
echo 4. Envie o arquivo  %USERPROFILE%\aoip-bridge-autoteste.log
echo.
taskkill /IM aoip-bridge.exe /F >nul 2>&1
set AOIP_SELFTEST=1
"%~dp0aoip-bridge.exe"
echo.
echo O programa segue rodando em segundo plano se voce salvou a configuracao.
echo Para encerrar, use o botao Encerrar no painel.
pause
