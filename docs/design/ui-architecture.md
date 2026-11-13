# Arquitetura da interface

## Painel (os três produtos de computador)

- Um arquivo: `core/src/ui.html` (HTML + CSS + JavaScript puros, sem dependências nem
  etapa de montagem), embutido no executável por `include_str!` em `core/src/ui.rs`.
- Servido só em 127.0.0.1 por um servidor HTTP mínimo (`ui::serve`):
  - programa AoIP Bridge: porta 8787;
  - driver ASIO do Windows e auxiliar da placa virtual do Mac: porta 8788.
- Estado: `GET /api/state` a cada 500 ms (JSON montado em `state_json`).
- Comandos (`POST`, com o cabeçalho `X-AoIP`): `/api/point`, `/api/preset`, `/api/rx`,
  `/api/setup`, `/api/quit`.
- A identidade visual **não mudou nenhum desses contratos** nem o `ui.rs`.

## Tema

- Script no `<head>` define `data-theme` em `<html>` antes do primeiro desenho.
- Preferência: "Sistema" (segue o sistema operacional e acompanha mudanças), "Escuro" ou
  "Claro", guardada em `localStorage` (`aoip-theme`).
- Limite conhecido: a preferência é por navegador e por porta (8787 e 8788 guardam
  separadamente). Guardar no arquivo de configuração do programa exigiria mudar `state.rs`
  e `ui.rs`; ficou fora desta etapa.

## Android

Tela única montada em código (`android/src/.../MainActivity.java`), com cores fixas.
Ainda não recebeu a identidade nova.

## Ver o painel sem placa nem rede

`cargo run -p aoip-core --example demo` e abrir http://127.0.0.1:8787 (dados de exemplo,
só para desenvolvimento).
