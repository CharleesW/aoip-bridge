#!/bin/bash
# Gera o .exe de Windows (com ASIO) a partir do macOS e monta dist/AoIP-Bridge-Windows-*.zip.
# Requisitos: rustup target add x86_64-pc-windows-gnu; cargo install cargo-zigbuild;
# Zig (pasta informada em ZIG_DIR) e as Command Line Tools da Apple (libclang).
set -euo pipefail
cd "$(dirname "$0")"
source "$HOME/.cargo/env"
ZIG_DIR="${ZIG_DIR:-$HOME/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/tools/zig-x86_64-macos-0.17.0}"
CLT=/Library/Developer/CommandLineTools
RES=$(ls -d "$CLT"/usr/lib/clang/*/include | tail -1)   # cabecalhos internos do clang (versao mais nova)
export PATH="$ZIG_DIR:$PATH"
export LIBCLANG_PATH="$CLT/usr/lib"
# bindgen usa o libclang da Apple: precisa dos cabecalhos dele, do libc++ da Apple e do mingw do Zig.
ARGS="--target=x86_64-pc-windows-gnu -nostdinc -isystem $CLT/SDKs/MacOSX.sdk/usr/include/c++/v1 -isystem $RES -isystem $ZIG_DIR/lib/libc/include/any-windows-any"
export BINDGEN_EXTRA_CLANG_ARGS="$ARGS" BINDGEN_EXTRA_CLANG_ARGS_x86_64_pc_windows_gnu="$ARGS"
cargo zigbuild --release -p aoip-bridge -p aoip-asio --target x86_64-pc-windows-gnu
rm -rf dist && mkdir -p dist/AoIP-Bridge-Windows
cp target/x86_64-pc-windows-gnu/release/aoip-bridge.exe target/x86_64-pc-windows-gnu/release/aoip_asio.dll windows/*.bat windows/LEIA-ME.txt dist/AoIP-Bridge-Windows/
(cd dist && zip -q -r AoIP-Bridge-Windows-0.3-teste.zip AoIP-Bridge-Windows)
echo "pronto: dist/AoIP-Bridge-Windows-0.3-teste.zip"
