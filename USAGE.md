# AION — Guía de uso

Super-agente de IA **local-first**: razona, recuerda, actúa, evoluciona y **vive**.
Todo el cómputo y los datos en tu Mac; la nube solo gestiona auth/licencias/sync (cifrado).

## Requisitos
- **Ollama** corriendo con los modelos:
  - `gemma4-reason` (chat/razonamiento/agente, abliterated Q6_K)
  - `nomic-embed-text` (memoria/embeddings)
  - `huihui_ai/gemma-4-abliterated:12b` (visión, opcional)
- **Rust** (`~/.cargo/env`) para compilar; la app de escritorio ya viene compilada.

## App de escritorio (recomendado) — un clic
Abre **`~/Desktop/AION.app`**. Arranca solo el backend (sidecars) y abre la UI premium.
Regístrate con tu email/contraseña (se guarda hasheada) y chatea.

## CLI — `aion-core` (núcleo)
```bash
cd ~/Desktop/Proyecto-AI-Local && source ~/.cargo/env
cargo run --bin aion-core -- <subcomando>
```
| Subcomando | Qué hace |
|------------|----------|
| `chat "<prompt>"`   | chat con razonamiento (streaming) |
| `rag "<consulta>"`  | RAG sobre documentos |
| `agent "<tarea>"`   | agente ReAct con herramientas (calc, skill WASM, memoria, web) |
| `vision <img> [p]`  | describe/razona sobre una imagen |
| `remember "<texto>"`| guarda en memoria de largo plazo |
| `recall "<consulta>"`| recupera de memoria |
| `history`           | conversaciones guardadas |
| `sleep`             | consolidación darwiniana de memoria (fusión/poda) |
| `skill <n>`         | ejecuta una skill WASM en sandbox |
| `evolve`            | demo del bucle de auto-mejora gated |
| `self-evolve`       | el LLM escribe una skill que pasa por las puertas |
| `cognition`         | demo de curiosidad/auto-modelo/metacognición |
| `live [ciclos]`     | **bucle de vida autónomo** (curiosidad→ejecuta→sueña→reflexiona) |
| `sync`              | demo de sincronización E2E entre dispositivos |
| `audit`             | registro de acciones del agente y la auto-evolución |
| `serve [addr]`      | puente HTTP (API) para la UI |

## Servicios (si usas la UI web en vez del .app)
```bash
cargo run --bin aion-core -- serve 127.0.0.1:8765   # núcleo + agente
cargo run -p aion-control-plane                     # auth/licencias :8787
cd apps/web && pnpm dev                             # UI en http://localhost:3000
```

## Conectar Claude Code (memoria compartida vía MCP)
AION expone su memoria a Claude Code por MCP en `http://127.0.0.1:8765/mcp` (loopback, token Bearer, auditado). En **cada máquina** la conexión se hace una vez:

1. **Instala la CLI de Claude Code** si no la tienes: `npm install -g @anthropic-ai/claude-code`. AION la detecta sola en las rutas típicas; si falta, la pantalla de conexión muestra el comando con botón de copiar.
2. Con AION corriendo, abre **Ajustes → Claude Code** (o `http://localhost:3000/claude-code`) y pulsa **Conectar**. AION escribe solo el bloque `mcpServers.aion` en `~/.claude.json` (permisos `0600`, sin exponer el token) y genera un Bearer nuevo.
3. Reinicia Claude Code; ya verás las tools `aion_*` (memoria, biblioteca, grafo, proyectos, brief, remember).

**Máquina nueva / reinstalación:** la URL es local y el token se regenera por máquina, así que la conexión **no se transporta**. Pero si restauras la config de AION (`claude_code.json` con `enabled: true`), AION **re-registra el endpoint solo al arrancar** reutilizando ese token — no hace falta volver a pulsar "Conectar". La **memoria** sí es portable aparte vía backup `.aion` (export/import); el registro MCP es independiente.

## Que AION "viva" en segundo plano (opt-in)
```bash
cp infra/launchd/it.prontoclick.aion.live.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/it.prontoclick.aion.live.plist
```

## Configuración
Copia `.env.example` a `.env` y rellena tus claves (Stripe, Postgres…) cuando las tengas.
Para Postgres: `docker compose -f infra/docker-compose.yml up -d` + migraciones en `infra/migrations/`.

## Privacidad
Tus chats, memoria y skills viven **solo en tu dispositivo**. La nube (cuando se use) solo
ve metadatos y **blobs cifrados E2E**. Modelo local sin censura, sin coste de inferencia.
