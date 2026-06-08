#!/usr/bin/env bash
# Expone Gemma 4 como API local compatible con OpenAI en :11434/v1
set -euo pipefail
pgrep -x ollama >/dev/null 2>&1 || (ollama serve >/dev/null 2>&1 &) && sleep 2
ollama run gemma4-reason >/dev/null 2>&1 &   # precarga en RAM
echo "✅ API OpenAI-compatible activa en: http://localhost:11434/v1"
echo "   Modelo: gemma4-reason"
echo "   Prueba: python3 client.py"
