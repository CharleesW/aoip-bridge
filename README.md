# aoip-bridge

Ponte livre entre uma placa de áudio e uma rede **AES67**, com matriz de roteamento, driver
ASIO virtual para Windows e um aplicativo Android de escuta. Os fluxos enviados aparecem no
Dante Controller (em azul) e podem ser assinados por aparelhos Dante com o modo AES67 ligado.

Caso de uso que motivou o projeto: levar os 32 canais de uma mesa Soundcraft Ui24R para a rede
de áudio de uma igreja e ouvir/gravar em qualquer computador ou celular da rede.

Licença: **GPL-3.0-or-later**. Projeto independente, sem vínculo com a Audinate. "Dante" é
marca registrada da Audinate Pty Ltd e aparece aqui só para descrever a compatibilidade.

> **Estado: versão de teste.** Funciona nos testes descritos abaixo, mas ainda não foi usado
> em produção nem validado com aparelhos Dante de verdade. Leia "O que foi testado".

## O que tem aqui

| Pasta | O que é |
|---|---|
| `core/` | Núcleo sem dependências externas: matriz, RTP (L24), SDP, SAP, PTPv2 (seguidor e relógio próprio), fila com correção de relógio e conversão de taxa, configuração, painel web. 28 testes. |
| `engine/` | Rede e relógio em funcionamento (threads de envio, recepção, PTP e anúncios). |
| `bridge/` | Programa **AoIP Bridge** (macOS e Windows): liga uma placa de som (CoreAudio/ASIO) à rede. Painel em http://127.0.0.1:8787. |
| `asio-driver/` | **AoIP Bridge ASIO** (Windows): driver ASIO virtual 32x32. O programa de áudio (REAPER etc.) enxerga a rede como se fosse uma placa. Painel em http://127.0.0.1:8788. |
| `android/` | **AoIP Escuta** (Android 6+): escolhe um fluxo da rede e dois canais e toca no celular. Java puro, sem Gradle. |
| `windows/` | Instalador e desinstalador (`.bat`) e instruções do pacote Windows. |
| `tools/` | Ferramentas de medição usadas nos testes. |
| `vendor/asio-sys/` | Cópia do `asio-sys` 0.2.6 (do projeto cpal, Apache-2.0) com um ajuste para compilação cruzada. |

## Instalar

Os pacotes prontos ficam na página **Releases** do repositório.

- **Windows:** baixe o `.zip`, extraia e rode `Instalar.bat` (pede administrador). Ele instala
  o programa e registra o driver ASIO. Instruções em `LEIA-ME.txt`.
- **Android:** baixe o `.apk` no celular e toque nele (é preciso permitir "instalar apps
  desconhecidos"). O celular deve estar no Wi-Fi da mesma rede.
- **macOS:** rode `bash install-mac.command`. Ele instala o Rust se faltar, compila e cria
  `~/Applications/AoIP Bridge.app`.

## Compilar

    cargo test -p aoip-core                 # testes do núcleo
    cargo build --release -p aoip-bridge    # programa (macOS; no Windows exige LLVM)
    ./build-windows-from-mac.sh             # pacote Windows a partir do macOS (cargo-zigbuild + Zig)
    android/build-apk.sh                    # APK (JDK 17 + SDK Android em ~/.aoip-android)

## Usar

1. **Relógio.** Se houver um aparelho Dante com "AES67 Mode" ligado, ele fornece o relógio
   PTPv2. Sem nenhum, o próprio programa gera o relógio e cede a vez a qualquer aparelho real.
2. **Enviar.** No painel, as entradas da placa (ou as saídas do programa de áudio, no driver
   ASIO) viram fluxos `Nome : 1`, `Nome : 2`... de 8 canais.
3. **Receber.** Escolha um fluxo anunciado na rede e ligue os canais dele na matriz.
4. **Celular.** Abra o AoIP Escuta, escolha o fluxo e os dois canais e toque em Ouvir.

Use rede **cabeada** entre os computadores. Em Wi-Fi o áudio em rede sofre pausas; o
aplicativo Android compensa com uma folga maior (80 ms por padrão).

## O que foi testado

- Núcleo: 28 testes automáticos.
- Ida e volta entre um Mac e um notebook Windows por cabo: REAPER usando o driver ASIO,
  32 canais enviados, retorno com níveis exatos, cerca de 17 ms por sentido.
- Relógio próprio: um computador gera, o outro segue.
- Placa em 44,1 kHz com conversão para os 48 kHz da rede.
- Aplicativo Android 0.1 recebendo do notebook pelo Wi-Fi.

## O que ainda não foi testado ou não existe

- Aparelhos Dante de verdade (relógio PTP vindo deles, assinatura dos fluxos no Dante
  Controller) e a própria Ui24R.
- Aparecer como **dispositivo** no Dante Controller, com canais de recepção roteáveis por lá.
  Isso exige o protocolo Dante nativo; hoje os fluxos só aparecem como transmissores AES67.
- Driver virtual para macOS.
- Aplicativo Android 0.2 (tocar com a tela apagada) em celular.
- Uso prolongado (horas).

## Limitações conhecidas

- Os endereços multicast dos fluxos são derivados do último número do IP; dois computadores
  com IPs vizinhos podem cair no mesmo endereço. A recepção deste projeto filtra pela origem,
  mas aparelhos de terceiros podem se confundir.
- PTP por software, sem medição de atraso.
- Conversão de taxa e correção de relógio por interpolação linear.
- As filas usam `Mutex` dentro do callback de áudio.
- O driver ASIO aceita um programa por vez e só 48 kHz.
