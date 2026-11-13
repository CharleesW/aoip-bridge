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

## Próximas etapas

Fase C (telas): matriz com teclado e nomes acessíveis, Situação e Configuração refinadas.
Fase D: estados vazios/erro, diálogos próprios. Android e ícones dos programas.
