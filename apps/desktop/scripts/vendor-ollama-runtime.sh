#!/usr/bin/env bash
# Vendoriza el runtime Ollama PORTÁTIL dentro del bundle de AION para que la app
# sea 100% autocontenida (el usuario NO instala Ollama).
#
# Copia el binario `ollama` + su runner `llama-server` + todas las dylib/so +
# los runners Metal (mlx_metal_v*) a `src-tauri/ollama-runtime/`. Gracias a que
# `llama-server` usa @rpath=@loader_path, todo carga por ruta relativa: la carpeta
# es portátil y funciona sin /Applications/Ollama.app (verificado desde /tmp).
#
# El binario `ollama` es universal (arm64+x86_64) → cubre Mac Silicon + Intel.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"        # apps/desktop
DEST="$HERE/src-tauri/ollama-runtime"
SRC="${OLLAMA_APP_RESOURCES:-/Applications/Ollama.app/Contents/Resources}"

if [[ ! -x "$SRC/ollama" || ! -x "$SRC/llama-server" ]]; then
  echo "ERROR: no encuentro el runtime de Ollama en: $SRC" >&2
  echo "Instala Ollama.app o exporta OLLAMA_APP_RESOURCES apuntando a su carpeta Resources." >&2
  exit 1
fi

echo "Vendorizando runtime Ollama desde: $SRC"
rm -rf "$DEST"; mkdir -p "$DEST"

# Binarios (ollama + runner + quantizador).
cp "$SRC/ollama" "$SRC/llama-server" "$DEST/"
[[ -x "$SRC/llama-quantize" ]] && cp "$SRC/llama-quantize" "$DEST/" || true

# Librerías nativas (Metal/CPU): dylib (macOS) y so (variantes CPU x86).
cp "$SRC"/*.dylib "$DEST/" 2>/dev/null || true
cp "$SRC"/*.so "$DEST/" 2>/dev/null || true

# Runners Metal (estructura de directorios) si existen.
for d in mlx_metal_v3 mlx_metal_v4; do
  [[ -d "$SRC/$d" ]] && cp -R "$SRC/$d" "$DEST/" || true
done

# Verificación de arquitectura (debe ser universal para cubrir Silicon+Intel).
ARCHS="$(lipo -archs "$DEST/ollama" 2>/dev/null || echo '?')"
COUNT="$(find "$DEST" -type f | wc -l | xargs)"
SIZE="$(du -sh "$DEST" | cut -f1)"
echo "OK → $DEST"
echo "   archivos: $COUNT · tamaño: $SIZE · arch ollama: $ARCHS"
[[ "$ARCHS" == *arm64* && "$ARCHS" == *x86_64* ]] \
  && echo "   ✓ universal (Mac Silicon + Intel)" \
  || echo "   ⚠ NO universal — Intel podría no funcionar con este binario"
