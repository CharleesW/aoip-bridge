# Sistema de design — AOIP-Bridge

Direção aprovada: **Minimal Premium**, temas escuro e claro. Tudo o que está aqui existe no
código (`core/src/ui.html`); o que ainda é proposta está marcado como tal.

## Onde ficam os tokens

No topo do `<style>` de `core/src/ui.html`, em `:root` (tema escuro, padrão) e
`:root[data-theme="light"]`. Os componentes só usam tokens semânticos; valor de cor fora
desse bloco é erro (exceções: o verde da marca dentro do símbolo e o ícone do navegador).

## Paleta base

| Token | Valor | Uso |
|---|---|---|
| ink-900 | `#171B21` | fundo do tema escuro, texto do tema claro |
| slate-800 | `#343C47` | estrutura (referência; as bordas usam variações) |
| signal-500 | `#26A88B` | marca, ação principal, rota ativa |
| silver-400 | `#A8B1BC` | texto secundário no escuro |
| paper-050 | `#F4F6F8` | fundo do tema claro |

## Tokens semânticos

| Token | Escuro | Claro | Uso |
|---|---|---|---|
| `--bg` | `#171B21` | `#F4F6F8` | fundo da página |
| `--surface` | `#1E232B` | `#FFFFFF` | blocos |
| `--field` | `#262C35` | `#FFFFFF` | campos e botões |
| `--hover` | `#2B323C` | `#EEF1F4` | passar o mouse |
| `--border` | `#2E3540` | `#D5DBE2` | divisórias |
| `--border-strong` | `#3A4350` | `#B9C2CC` | botões, grupos de 8 na matriz |
| `--border-input` | `#66727F` | `#7F8A97` | contorno de campos (≥ 3:1) |
| `--text` | `#E8ECF0` | `#171B21` | texto principal |
| `--text-2` | `#A8B1BC` | `#4A5564` | texto secundário |
| `--text-3` | `#8A95A3` | `#5F6B79` | texto discreto |
| `--action` / `--on-action` | `#26A88B` / `#0B1513` | `#1B7F69` / `#FFFFFF` | botão principal |
| `--focus` | `#7FD8C4` | `#1B7F69` | anel de foco do teclado |
| `--select` | `#E8ECF0` | `#171B21` | célula selecionada na matriz |
| `--ok` | `#4CC39A` | `#1B7455` | funcionando |
| `--info` | `#6AA5E8` | `#2B66AD` | em andamento (iniciando, reiniciando) |
| `--warn` | `#D9A441` | `#8A5A10` | atenção |
| `--err` | `#EC7F7F` | `#B03A3A` | erro |
| `--route` | `#26A88B` | `#1B7F69` | ponto ligado (0 dB) |
| `--route-att` | `#1B6F5D` | `#8CC9BA` | ponto ligado com atenuação |
| `--route-boost` | `#C98A2B` | `#C98A2B` | ponto ligado com reforço |

Os valores de estado sugeridos no brief (`#238B68`, `#3478C8`, `#B7791F`, `#C44747`) são a
base dos fundos translúcidos dos selos; para **texto** foram clareados (escuro) ou
escurecidos (claro) para passar no contraste.

### Contraste medido (WCAG, texto sobre `--bg` / sobre `--surface`)

| Token | Escuro | Claro |
|---|---|---|
| `--text` | 14,6 / 13,3 | 16,0 / 17,3 |
| `--text-2` | 8,0 / 7,3 | 7,0 / 7,6 |
| `--text-3` | 5,7 / 5,2 | 5,0 / 5,4 |
| `--ok` | 7,9 / 7,2 | 5,3 / 5,7 |
| `--info` | 6,7 / 6,1 | 5,4 / 5,8 |
| `--warn` | 7,7 / 7,0 | 5,5 / 5,9 |
| `--err` | 6,5 / 5,9 | 5,5 / 6,0 |
| botão principal (texto/fundo) | 6,2 | 4,9 |

Todos acima de 4,5:1 (AA para texto normal).

## Tipografia

- Interface: `Inter` se estiver instalada, senão a fonte do sistema. **Nenhuma fonte é
  embutida** (o painel funciona sem internet e o programa fica leve).
- Valores técnicos (ganho, números da matriz): `JetBrains Mono` se instalada, senão a
  monoespaçada do sistema.
- Numerais tabulares em toda a página.
- Tamanhos: título de bloco 16 px; corpo 14 px; apoio 12–13 px; cabeçalho da matriz 11 px;
  número dentro da célula 9 px (só o ganho em dB, informação redundante com a cor e o
  controle de ganho).

## Espaçamento e forma

Escala de 4 px (`--s1` a `--s8`: 4, 8, 12, 16, 20, 24, 32). Raios `--r-sm` 4, `--r-md` 6,
`--r-lg` 8. Sem sombras nem brilho; a separação é por borda fina e diferença de superfície.

## Ícones

Conjunto próprio, mínimo, desenhado em 16 px com traço de 1,75 (constante `ICON` no
script): funcionando, erro, atenção, em andamento, neutro. Nenhuma biblioteca de ícones foi
adicionada. Estado é sempre **ícone + texto**, nunca só cor.

## Componentes existentes

Selo de estado (`.badge`), indicador de situação (`.step`), aviso (`.alert`, erro ou
atenção), botões (principal, padrão, discreto), campos, barra de ações, matriz, medidor,
cartão de fluxo (`.slot`), controle de ganho.

## Movimento

Nenhuma animação contínua. Só o retorno de clique dos botões, desligado quando o sistema
pede "reduzir movimento".

## Matriz de roteamento

- Linhas = origens, colunas = destinos; cabeçalhos fixos ao rolar; divisórias mais fortes
  a cada 8 canais nos dois sentidos.
- Ponto ligado = cor de rota + número do ganho quando diferente de 0 dB; ponto selecionado
  (para ajuste de ganho) = contorno.
- Ao passar o mouse ou focar um ponto, a linha de origem e a coluna de destino ficam
  destacadas no cabeçalho.
- Teclado: um único ponto recebe o Tab (os outros pelas setas). Setas andam, `PgUp`/`PgDn`
  pulam 8 linhas, `Home`/`End` vão ao começo/fim da linha (com `Ctrl`, da matriz),
  `Enter`/`Espaço` liga e desliga, `Del` desliga.
- Cada ponto tem nome acessível ("Entrada 3 para Ui24R : 1, canal 2: ligado, 0 dB") e o
  resultado de cada mudança é anunciado numa região viva (inclusive a recusa).
- Toda mudança chama o comando real (`/api/point`); a matriz só muda de cor quando o estado
  volta do programa. A pintura só altera as células que mudaram.

## Formulário

Mensagem embaixo de cada campo que tem regra (`.msg`; em erro, `.msg.err` e
`aria-invalid`). As regras repetem as do programa (`ui.rs`): IP vazio ou IPv4 válido, placa
obrigatória. O botão Salvar mostra "Salvando…" e fica desativado enquanto espera.

## Ainda não feito (propostas)

- Medidor com marca não dependente de cor para saturação.
- Diálogos próprios no lugar de `confirm()` do navegador.
- Mesma identidade no aplicativo Android e nos ícones dos programas.
