#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────────────
# AION · Aprovisionamiento del stack de VOZ local en un Mac nuevo (Apple Silicon)
#
# El .app de AION embebe Ollama + Gemma (chat de texto se autoconfigura en el 1er
# arranque). Pero la VOZ optimizada (cerebro Qwen3-4B, Qwen3-TTS, Kokoro, voces
# Piper) vive en ~/Library/Application Support/AION/ y NO viaja en el bundle.
# Este script la instala, replicando EXACTAMENTE la instalación de referencia:
#
#   llm/venv      mlx-lm 0.31.3      → cerebro de voz (mlx_lm.server :11920)
#   tts/venv-mlx  mlx-audio 0.4.4    → Qwen3-TTS clonación/natural (:8768)
#   tts/venv      kokoro-onnx 0.5.0 + piper-tts 1.4.2 → Kokoro/Piper (:8766)
#
# Modelos (HuggingFace + GitHub releases):
#   mlx-community/Qwen3-4B-Instruct-2507-4bit            (~2.1 GB)
#   mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit   (~0.9 GB)
#   kokoro-v1.0.onnx + voices-v1.0.bin                   (~0.35 GB)
#   4 voces Piper español (Diego/Mateo hombre, Lucía/Daniela mujer) (~0.3 GB)
#
# Idempotente: re-ejecutar es seguro (salta lo ya presente). Descarga ~3.5 GB.
# Requisitos: Apple Silicon, macOS, conexión a internet, Homebrew.
# Uso:  bash scripts/provision-voice-mac.sh
# ──────────────────────────────────────────────────────────────────────────────
set -euo pipefail

AION_DIR="$HOME/Library/Application Support/AION"
TTS_DIR="$AION_DIR/tts"
PIPER_DIR="$TTS_DIR/piper-voices"
LLM_DIR="$AION_DIR/llm"
PY_VER="3.12"

# Versiones ANCLADAS a la instalación de referencia (evita drift que rompa MLX).
MLX_LM_VER="0.31.3"
MLX_AUDIO_VER="0.4.4"
KOKORO_VER="0.5.0"
PIPER_VER="1.4.2"

QWEN_BRAIN="mlx-community/Qwen3-4B-Instruct-2507-4bit"
QWEN_TTS="mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit"

KOKORO_BASE="https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0"
PIPER_BASE="https://huggingface.co/rhasspy/piper-voices/resolve/main"

bold() { printf "\033[1m%s\033[0m\n" "$1"; }
ok()   { printf "  \033[32m✓\033[0m %s\n" "$1"; }
info() { printf "  \033[36m·\033[0m %s\n" "$1"; }
warn() { printf "  \033[33m!\033[0m %s\n" "$1"; }
die()  { printf "  \033[31m✗ %s\033[0m\n" "$1" >&2; exit 1; }

# ── 0) Comprobaciones de entorno ──────────────────────────────────────────────
bold "AION · aprovisionamiento de voz local"
[ "$(uname -s)" = "Darwin" ] || die "Esto es solo para macOS."
[ "$(uname -m)" = "arm64" ]  || die "Se requiere Apple Silicon (arm64). MLX no corre en Intel."
ok "Apple Silicon detectado ($(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo Mac))"

if ! command -v uv >/dev/null 2>&1; then
  info "uv no está instalado; instalando con Homebrew…"
  command -v brew >/dev/null 2>&1 || die "Falta Homebrew. Instálalo desde https://brew.sh y reintenta."
  brew install uv
fi
ok "uv $(uv --version | awk '{print $2}')"

mkdir -p "$TTS_DIR" "$PIPER_DIR" "$LLM_DIR"

# ── helpers ───────────────────────────────────────────────────────────────────
# Crea un venv con uv si no existe y devuelve su python.
make_venv() { # $1=ruta_venv
  if [ ! -x "$1/bin/python" ]; then
    info "creando venv $(basename "$(dirname "$1")")/$(basename "$1") (Python $PY_VER)…"
    uv venv --python "$PY_VER" "$1" >/dev/null
  fi
}
# Descarga con reanudación; salta si el destino ya tiene tamaño > mínimo.
fetch() { # $1=url $2=destino $3=min_bytes
  if [ -f "$2" ] && [ "$(stat -f%z "$2" 2>/dev/null || echo 0)" -gt "${3:-1000}" ]; then
    ok "$(basename "$2") (ya presente)"; return 0
  fi
  info "descargando $(basename "$2")…"
  curl -fL --retry 3 -C - -o "$2" "$1" || die "fallo al descargar $1"
  ok "$(basename "$2")"
}

# ── 1) Cerebro de voz: llm/venv + mlx-lm ────────────────────────────────────────
bold "1/5 · Cerebro de voz (mlx-lm)"
make_venv "$LLM_DIR/venv"
info "instalando mlx-lm==$MLX_LM_VER…"
uv pip install --python "$LLM_DIR/venv/bin/python" --quiet "mlx-lm==$MLX_LM_VER"
ok "mlx-lm listo"

# ── 2) Voz natural/clonada: tts/venv-mlx + mlx-audio ────────────────────────────
bold "2/5 · Voz natural Qwen3-TTS (mlx-audio)"
make_venv "$TTS_DIR/venv-mlx"
info "instalando mlx-audio==$MLX_AUDIO_VER…"
uv pip install --python "$TTS_DIR/venv-mlx/bin/python" --quiet "mlx-audio==$MLX_AUDIO_VER"
ok "mlx-audio listo"

# ── 3) Kokoro + Piper: tts/venv ─────────────────────────────────────────────────
bold "3/5 · Kokoro + Piper (kokoro-onnx, piper-tts)"
make_venv "$TTS_DIR/venv"
info "instalando kokoro-onnx==$KOKORO_VER y piper-tts==$PIPER_VER…"
uv pip install --python "$TTS_DIR/venv/bin/python" --quiet \
  "kokoro-onnx==$KOKORO_VER" "piper-tts==$PIPER_VER" "soundfile>=0.13"
ok "kokoro-onnx + piper-tts listos"

# ── 4) Modelos de fichero: Kokoro + voces Piper ─────────────────────────────────
bold "4/5 · Modelos Kokoro + voces Piper (descarga)"
fetch "$KOKORO_BASE/kokoro-v1.0.onnx" "$TTS_DIR/kokoro-v1.0.onnx" 300000000
fetch "$KOKORO_BASE/voices-v1.0.bin"  "$TTS_DIR/voices-v1.0.bin"  20000000

# Voz: ruta_HF|fichero_local. Diego=davefx(hombre 107Hz), Mateo=ald(hombre 152Hz),
# Lucía=claude(mujer), Daniela=daniela(mujer). Ver memoria aion-voz-genero-piper.
PIPER_VOICES=(
  "es/es_ES/davefx/medium/es_ES-davefx-medium|es_ES-davefx-medium"
  "es/es_MX/ald/medium/es_MX-ald-medium|es_MX-ald-medium"
  "es/es_MX/claude/high/es_MX-claude-high|es_MX-claude-high"
  "es/es_AR/daniela/high/es_AR-daniela-high|es_AR-daniela-high"
)
for entry in "${PIPER_VOICES[@]}"; do
  path="${entry%%|*}"; name="${entry##*|}"
  fetch "$PIPER_BASE/$path.onnx"      "$PIPER_DIR/$name.onnx"      40000000
  fetch "$PIPER_BASE/$path.onnx.json" "$PIPER_DIR/$name.onnx.json" 1000
done

# ── 5) Modelos MLX (Qwen3-4B cerebro + Qwen3-TTS) al cache de HuggingFace ────────
bold "5/5 · Modelos MLX (Qwen3-4B + Qwen3-TTS) — precarga"
info "descargando $QWEN_BRAIN (~2.1 GB)…"
"$LLM_DIR/venv/bin/python" - "$QWEN_BRAIN" <<'PY'
import sys
from huggingface_hub import snapshot_download
snapshot_download(sys.argv[1])
print("  ✓ cerebro descargado")
PY
info "descargando $QWEN_TTS (~0.9 GB)…"
"$TTS_DIR/venv-mlx/bin/python" - "$QWEN_TTS" <<'PY'
import sys
from huggingface_hub import snapshot_download
snapshot_download(sys.argv[1])
print("  ✓ Qwen3-TTS descargado")
PY

bold "✅ Voz aprovisionada."
echo "   Reinicia AION (o ábrela) y el modo voz usará el stack local:"
echo "   cerebro Qwen3-4B (:11920) · Qwen3-TTS (:8768) · Kokoro/Piper (:8766)."
echo "   Voz por defecto: Mateo (hombre). En Ajustes → Voz puedes elegir Diego/Lucía/Daniela."
