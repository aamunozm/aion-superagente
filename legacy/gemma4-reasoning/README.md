# Gemma 4 12B · Razonamiento · Sin censura · MacBook Pro M2 Max

Setup óptimo **calidad/velocidad** con **thinking mode** y **visión**, sin restricciones.

- **Modelo:** `huihui-ai/Huihui-gemma-4-12B-it-abliterated` (abliterated, thinking mode preservado)
- **Cuantización:** **Q6_K imatrix** (~9.8 GB) — punto dulce calidad/velocidad en 32 GB
- **Runtime:** Ollama (API OpenAI en `:11434/v1`)

## Uso rápido

```bash
chmod +x install.sh serve.sh
./install.sh          # instala, descarga pesos y crea el modelo 'gemma4-reason'
ollama run gemma4-reason
```

## Razonamiento (thinking mode)

El SYSTEM prompt ya pide razonar en bloques `<think>...</think>`. Además puedes
activar el modo nativo de Ollama:

```bash
ollama run gemma4-reason --think        # fuerza razonamiento
# dentro del chat:  /set think true   |   /set think false
```

## API local (OpenAI-compatible)

```bash
./serve.sh
python3 client.py     # pip install openai
```
Endpoint: `http://localhost:11434/v1` · modelo: `gemma4-reason`

## Ajustes

- **Más calidad:** cambia el quant a `Q8_0` en `install.sh` y `Modelfile` (~12.7 GB).
- **Más velocidad:** baja a `Q4_K_M` (~7.4 GB) o usa menos `num_ctx`.
- **Contexto largo:** sube `num_ctx` a 65536 en el Modelfile (vigila RAM: `ollama ps`).
- **Visión:** descarga `mmproj-f16.gguf` (lo hace install.sh) para procesar imágenes.

## Alternativa máxima calidad (sin thinking garantizado)

`OBLITERATUS/Gemma-4-12B-OBLITERATED` Q8_0 — cero rechazos con paridad de benchmark.
Cámbialo en el `FROM` del Modelfile si priorizas exactitud sobre razonamiento explícito.
