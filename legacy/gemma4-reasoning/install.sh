#!/usr/bin/env bash
# =====================================================================
# Instalador: Gemma 4 12B abliterated Q6_K (imatrix) + razonamiento
# MacBook Pro M2 Max (32 GB)
# =====================================================================
set -euo pipefail

cd "$(dirname "$0")"

echo "==> 1/4  Comprobando dependencias..."
command -v ollama >/dev/null 2>&1 || { echo "Instalando Ollama..."; brew install ollama; }
command -v huggingface-cli >/dev/null 2>&1 || pip install -U "huggingface_hub[cli]"

echo "==> 2/4  Arrancando servicio Ollama (si no corre)..."
pgrep -x ollama >/dev/null 2>&1 || (ollama serve >/dev/null 2>&1 &) && sleep 2

REPO="mradermacher/Huihui-gemma-4-12B-it-abliterated-i1-GGUF"
GGUF="Huihui-gemma-4-12B-it-abliterated.i1-Q6_K.gguf"
MMPROJ="mmproj-f16.gguf"   # proyector de visión

echo "==> 3/4  Descargando pesos Q6_K (~9.8 GB) + proyector de visión..."
huggingface-cli download "$REPO" "$GGUF"   --local-dir . --local-dir-use-symlinks False || \
  echo "   (Si el nombre del archivo cambió, míralo en https://huggingface.co/$REPO/tree/main)"
# El mmproj suele estar en el repo de quants estáticos:
huggingface-cli download "mradermacher/Huihui-gemma-4-12B-it-abliterated-GGUF" "$MMPROJ" --local-dir . --local-dir-use-symlinks False || \
  echo "   (mmproj opcional: solo si vas a usar imágenes)"

echo "==> 4/4  Creando el modelo en Ollama..."
ollama create gemma4-reason -f Modelfile

echo ""
echo "✅ Listo. Úsalo con:   ollama run gemma4-reason"
echo "   API local:          ./serve.sh"
