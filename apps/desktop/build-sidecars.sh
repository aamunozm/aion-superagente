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

# Helper de reconocimiento facial (Swift/Apple Vision). Info.plist embebido para el permiso de
# cámara (NSCameraUsageDescription). Si no hay swiftc, se omite (la app sigue, sin face-probe).
FP="apps/desktop/face_probe"
if command -v swiftc >/dev/null 2>&1; then
  echo "==> compilando face-probe (Swift/Vision)"
  swiftc -O "$FP/face_probe.swift" -o "$DEST/face-probe-$TRIPLE" \
    -framework AVFoundation -framework Vision -framework CoreImage \
    -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker "$FP/Info.plist"
else
  echo "==> AVISO: swiftc no disponible; face-probe NO se compila (reconocimiento facial inactivo)"
fi

echo "==> sidecars listos en $DEST"
echo "    ahora: cd apps/desktop && cargo tauri build"
