# Notas de implementação

## Fase A — auditoria (concluída)

Interface real: uma página estática embutida; sem framework. Referência de testes antes
das mudanças: 28 testes do núcleo e 20 verificações do Android passando.

## Fase B — fundações (esta etapa)

Mudanças, todas em `core/src/ui.html` (nada no motor de áudio, nos drivers nem no `ui.rs`):

- Tokens semânticos centralizados; temas escuro e claro; seletor de tema no cabeçalho.
- Cabeçalho com o símbolo e o nome AOIP-Bridge; ícone do navegador.
- Estados com ícone + texto (selo do topo, quatro indicadores, fluxos recebidos, avisos).
  O aviso de falhas de áudio passou de "erro" para "atenção".
- Anel de foco visível para teclado; "reduzir movimento" respeitado.
- Correção: ao rolar a matriz, a linha com o nome dos fluxos ficava escondida atrás da
  linha dos números.
- Arte da marca em `assets/brand/`.

Decisões tomadas sem resposta explícita (fáceis de reverter):

1. AOIP-Bridge só no visual; nomes técnicos dos drivers mantidos.
2. Painel continua em português; lema em inglês.
3. Fontes do sistema (Inter/JetBrains Mono só se já instaladas); nada embutido.
4. Brief guardado em `docs/design/brief.md`.
5. Android fica para uma etapa própria.

## Verificação

- `cargo test -p aoip-core`: 28 passaram.
- `cargo build --release -p aoip-bridge -p aoip-virtual`: sem erros.
- Painel de demonstração conferido no navegador nos dois temas (1280x900), sem erros no
  console.
- **Não conferido:** os painéis dentro do driver ASIO no Windows e do auxiliar no Mac com
  áudio real (usam o mesmo arquivo, mas não foram reabertos); navegação só por teclado;
  capturas de tela ainda não guardadas no repositório.

## Fase C — telas (esta etapa)

Só `core/src/ui.html`. Matriz: teclado, nomes acessíveis, destaque de origem e destino,
divisórias a cada 8 linhas, anúncio do resultado, pintura incremental. Configuração:
validação junto ao campo (mesmas regras do `ui.rs`) e estado "Salvando…". Correção:
mensagem de erro do IP ficava presa quando o programa reescrevia o campo.

Verificação: 28 testes do núcleo; no painel de demonstração, pelo teclado, Tab entra na
matriz num ponto só, setas movem e destacam origem/destino, Enter ligou e Del desligou um
ponto (o estado voltou do programa e foi anunciado), IP inválido mostrou erro e o erro
sumiu ao corrigir; temas escuro e claro.

## Fase D — acabamento (esta etapa)

Só `core/src/ui.html`. Diálogos próprios no lugar de `confirm()`, com confirmação também
para "1 para 1" (substitui o roteamento atual); estados de carregando, reiniciando, sem
resposta (indicadores viram "Sem informação") e encerrado; aviso no lugar das matrizes
quando a placa está parada; marca de pico no medidor; layout para janela estreita.
Correção: em janela estreita a matriz alargava a página inteira.

Verificação no painel de demonstração: diálogo de Limpar abre com foco em Cancelar e `Esc`
cancela sem mudar nada (33 pontos continuaram ligados); 390 px sem rolagem lateral da
página; ao desligar o programa, em ~2 s o selo vira "sem resposta", os indicadores viram
"Sem informação" e os blocos ficam esmaecidos; ao religar, tudo volta sozinho; temas escuro
e claro. 28 testes do núcleo passando; programa e auxiliar do Mac compilam.

Não verificado: estado "reiniciando" com o programa real (a demonstração não reinicia),
painéis dentro do driver ASIO e do auxiliar do Mac, leitor de tela de verdade.

## Fase E — verificação e entrega (esta etapa)

- Pacotes de teste **0.4** gerados a partir do ramo: `dist/AoIP-Bridge-Mac-0.4-teste.zip`
  e `dist/AoIP-Bridge-Windows-0.4-teste.zip`.
- **Mac, instalação real:** o aplicativo instalado em `~/Applications` foi trocado pelo do
  pacote 0.4 (o anterior ficou como `AoIP Bridge.app.old`) e o auxiliar foi religado pelo
  launchd. O driver da placa virtual não mudou nesta identidade visual, então não foi
  preciso reinstalar com senha. Conferido no painel real (127.0.0.1:8788): estado parado,
  estado funcionando com um cliente CoreAudio de teste, temas escuro e claro.
- **Programa AoIP Bridge real** (aberto numa porta de teste, 8790, com a configuração do
  usuário): mostrou corretamente o erro de placa não encontrada no visual novo.
- Capturas reais em `docs/design/screenshots/` (nenhuma é maquete):
  `mac-placa-virtual-parada-escuro.jpg`, `mac-placa-virtual-funcionando-escuro.jpg`,
  `mac-placa-virtual-funcionando-claro.jpg`, `programa-erro-de-placa-claro.jpg`.
- Correções na compilação, sem relação com o visual mas necessárias para gerar o pacote:
  - o componente ASIO (`vendor/asio-sys/build.rs`) só declarava a biblioteca C++ na
    primeira compilação; a partir da segunda o `aoip-bridge.exe` não ligava. Agora declara
    sempre;
  - `build-windows-from-mac.sh` apagava a pasta `dist/` inteira (levando junto os pacotes
    do Mac e do Android); agora só refaz a parte do Windows.
- Motor de áudio, drivers e comandos do painel: sem mudanças (conferir com
  `git diff main -- engine asio-driver/src mac-driver/src mac-driver/driver.c core/src/ui.rs
  core/src/state.rs`, que sai vazio; fora disso só o número de versão nos scripts de pacote).

### Observações para depois

- A placa virtual do Mac parada (estado normal quando nenhum programa a usa) aparece como
  **erro** em vermelho, porque o auxiliar escreve o aviso no campo de erro. Ficaria melhor
  como informação; exige mudar `mac-driver/src/main.rs` (fora do escopo visual).
- O Mac está gerando o relógio quando o notebook não está na rede; o texto do indicador
  ("este computador esta gerando o relogio") vem do motor, sem acentos.

### Não verificado

- O painel novo dentro do driver ASIO no Windows (precisa instalar o pacote 0.4 lá).
- Leitor de tela de verdade e uso prolongado.

## Próximas etapas

Depois: estados vazios/erro, diálogos próprios. Android e ícones dos programas.
