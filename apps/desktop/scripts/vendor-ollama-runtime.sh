#!/usr/bin/env bash
# Vendoriza el runtime Ollama PORTÁTIL y COMPLETO dentro del bundle de AION para que la app sea
# 100% autocontenida (el usuario NO instala Ollama).
#
# Copia el binario `ollama` + su runner `llama-server` + TODAS las dylib (ggml/llama/mtmd) + los
# runners Metal (mlx_metal_v*). Gracias a que `llama-server` usa @rpath=@loader_path, todo carga
# por ruta relativa: la carpeta es portátil y funciona sin /Applications/Ollama.app.
#
# OJO (auditoría 2026-06-25): el bug que esto evita es empaquetar SOLO el binario `ollama` sin su
# runner `llama-server` → Ollama 0.30.x devuelve 500 en TODA inferencia local (chat local Y
# embeddings/RAG). Por eso aquí se EXIGE `llama-server` y se verifica al final.
#
# Fuente (en orden): 1) $OLLAMA_APP_RESOURCES  2) /Applications/Ollama.app/Contents/Resources
#                    3) descarga del tgz oficial fijado (reproducible en cualquier Mac, sin Ollama).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"        # apps/desktop
DEST="$HERE/src-tauri/ollama-runtime"
# Versión FIJADA del runtime (debe casar con el formato de modelos ya descargados). Cambiar con
# cuidado: una versión distinta de `ollama` necesita su `llama-server` de la MISMA versión.
OLLAMA_VERSION="${OLLAMA_VERSION:-v0.30.6}"
TGZ_URL="https://github.com/ollama/ollama/releases/download/${OLLAMA_VERSION}/ollama-darwin.tgz"

SRC="${OLLAMA_APP_RESOURCES:-/Applications/Ollama.app/Contents/Resources}"
TMP=""
if [[ ! -x "$SRC/ollama" || ! -x "$SRC/llama-server" ]]; then
  echo "No hay un Ollama.app completo en: $SRC"
  echo "→ Descargando runtime oficial portátil: $TGZ_URL"
  TMP="$(mktemp -d)"
  curl -fsSL -o "$TMP/ollama.tgz" "$TGZ_URL"
  tar -xzf "$TMP/ollama.tgz" -C "$TMP"
  SRC="$TMP"
fi

if [[ ! -x "$SRC/ollama" || ! -x "$SRC/llama-server" ]]; then
  echo "ERROR: no encuentro 'ollama' + 'llama-server' en: $SRC" >&2
  exit 1
fi

echo "Vendorizando runtime Ollama desde: $SRC"
rm -rf "$DEST"; mkdir -p "$DEST"

# Binarios (ollama + runner + quantizador si está).
cp "$SRC/ollama" "$SRC/llama-server" "$DEST/"
[[ -x "$SRC/llama-quantize" ]] && cp "$SRC/llama-quantize" "$DEST/" || true

# Librerías nativas (Metal/CPU): dylib (macOS) y so (variantes CPU x86).
cp "$SRC"/*.dylib "$DEST/" 2>/dev/null || true
cp "$SRC"/*.so "$DEST/" 2>/dev/null || true

# Runners Metal (estructura de directorios) si existen.
for d in mlx_metal_v3 mlx_metal_v4; do
  [[ -d "$SRC/$d" ]] && cp -R "$SRC/$d" "$DEST/" || true
done

# Sin cuarentena (si vino de una descarga) para que ejecute sin bloqueo de Gatekeeper.
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true
[[ -n "$TMP" ]] && rm -rf "$TMP" || true

# Verificación: arquitectura universal + presencia del RUNNER (la causa del bug).
ARCHS="$(lipo -archs "$DEST/ollama" 2>/dev/null || echo '?')"
COUNT="$(find "$DEST" -type f | wc -l | xargs)"
SIZE="$(du -sh "$DEST" | cut -f1)"
echo "OK → $DEST"
echo "   archivos: $COUNT · tamaño: $SIZE · arch ollama: $ARCHS"
[[ -x "$DEST/llama-server" ]] \
  || { echo "   ✗ FALTA el runner llama-server — el runtime NO serviría modelos" >&2; exit 1; }
echo "   ✓ runner llama-server presente"
[[ "$ARCHS" == *arm64* && "$ARCHS" == *x86_64* ]] \
  && echo "   ✓ universal (Mac Silicon + Intel)" \
  || echo "   ⚠ NO universal — Intel podría no funcionar con este binario"
