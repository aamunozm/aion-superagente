#!/usr/bin/env bash
# Bootstrap de modelos en PRIMER ARRANQUE de AION.
#
# Comprueba que el Ollama embebido tenga los modelos necesarios y, si faltan, los
# descarga/crea usando el MISMO binario ollama embebido (que habla con el servidor
# ya levantado en $OLLAMA_HOST). Pensado para máquinas nuevas sin nada instalado.
#
# Uso:  first-run-models.sh <ollama_bin> <modelfile> <host:port>
# Idempotente: si los modelos ya existen, no hace nada.
set -uo pipefail

OLLAMA_BIN="${1:?falta ruta al binario ollama}"
MODELFILE="${2:?falta ruta al Modelfile}"
HOST="${3:-127.0.0.1:11919}"
export OLLAMA_HOST="$HOST"

CHAT_MODEL="gemma4-reason"
EMBED_MODEL="nomic-embed-text"

notify() { osascript -e "display notification \"$1\" with title \"AION\" sound name \"Glass\"" 2>/dev/null || true; }
log() { echo "[AION bootstrap] $*"; }

# 1) Esperar a que el servidor Ollama embebido responda (máx ~60s).
for i in $(seq 1 60); do
  if "$OLLAMA_BIN" list >/dev/null 2>&1; then break; fi
  sleep 1
done

HAVE="$("$OLLAMA_BIN" list 2>/dev/null || true)"
NEED_CHAT=1; NEED_EMBED=1
echo "$HAVE" | grep -q "$CHAT_MODEL"  && NEED_CHAT=0
echo "$HAVE" | grep -q "$EMBED_MODEL" && NEED_EMBED=0

if [[ "$NEED_CHAT" == 0 && "$NEED_EMBED" == 0 ]]; then
  log "modelos ya presentes — nada que hacer."
  exit 0
fi

notify "Preparando la IA por primera vez. Descargando el modelo (~9 GB). Te avisaré al terminar."
log "faltan modelos (chat=$NEED_CHAT embed=$NEED_EMBED). Descargando…"

# 2) Embeddings (pequeño, ~275 MB).
if [[ "$NEED_EMBED" == 1 ]]; then
  log "pull $EMBED_MODEL"
  "$OLLAMA_BIN" pull "$EMBED_MODEL" || { log "fallo al descargar $EMBED_MODEL"; notify "Error descargando embeddings."; }
fi

# 3) Modelo de razonamiento (grande): create descarga el GGUF de HF automáticamente.
if [[ "$NEED_CHAT" == 1 ]]; then
  log "create $CHAT_MODEL desde $MODELFILE"
  if "$OLLAMA_BIN" create "$CHAT_MODEL" -f "$MODELFILE"; then
    log "modelo $CHAT_MODEL listo."
  else
    log "fallo al crear $CHAT_MODEL"; notify "Error preparando el modelo de IA."; exit 1
  fi
fi

notify "¡Listo! AION ya está preparado para conversar."
log "bootstrap completado."
