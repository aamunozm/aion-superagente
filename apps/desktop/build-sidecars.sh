#!/usr/bin/env bash
# Compila el núcleo + control-plane y los coloca como sidecars de Tauri
# (con sufijo del target triple, como exige `externalBin`).
set -euo pipefail
cd "$(dirname "$0")/../.."   # raíz del repo
source "$HOME/.cargo/env" 2>/dev/null || true

TRIPLE=$(rustc -vV | sed -n 's/host: //p')
echo "==> compilando binarios release para $TRIPLE"
cargo build --release -p aion-core --bin aion-core
cargo build --release -p aion-control-plane --bin aion-control-plane

DEST="apps/desktop/src-tauri/binaries"
mkdir -p "$DEST"
cp target/release/aion-core "$DEST/aion-core-$TRIPLE"
cp target/release/aion-control-plane "$DEST/aion-control-plane-$TRIPLE"
echo "==> sidecars listos en $DEST"
echo "    ahora: cd apps/desktop && cargo tauri build"
