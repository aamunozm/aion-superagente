# AION

> Super-agente de IA **local-first**, multiplataforma, multiusuario, autónomo y auto-evolutivo.

AION ejecuta toda su cognición y datos **en el dispositivo** (privacidad total, coste de
inferencia cero); la nube solo gestiona auth, billing, licencias y sincronización cifrada E2E.

## Capacidades objetivo
🧠 Razonamiento · 🎯 Curiosidad y auto-objetivos · 🧬 Memoria darwiniana · 🛠️ Skills auto-generadas
(WASM) · 🌐 Browser agéntico · 🔧 Auto-mejora gated · 🪞 Auto-modelo y metacognición.

## Arquitectura (resumen)
- **Núcleo:** Rust (monolito modular, un binario `aion-core` con crates por dominio).
- **UI única:** Next.js/React/Tailwind → Tauri (desktop) + Capacitor (móvil).
- **LLM:** trait `LlmEngine` → Ollama (F1) → mistral.rs (F2) → MLX/Candle móvil (F6).
- **Memoria:** LanceDB · **Skills:** Extism (WASM) · **Sync:** Automerge (CRDT, E2E).
- **Control-plane:** Axum + Postgres + Redis + Stripe.

Detalle completo: ver `docs/` (RESEARCH, PRD, ADRs) y el plan maestro.

## Estructura
```
crates/     núcleo Rust (kernel inmutable + dominios)
apps/       aion-core (binario), desktop (Tauri), mobile (Capacitor), control-plane (Axum)
packages/   web (UI), design-system (tokens), ipc-bindings
docs/       investigación, ADRs, PRD, design tokens
infra/      docker-compose, migraciones, despliegue, observabilidad
legacy/     prototipo gemma4-reasoning (referencia)
```

## Desarrollo
```bash
# Rust
cargo build --workspace && cargo test --workspace
cargo run --bin aion-core            # smoke test del núcleo

# JS
pnpm install && pnpm build
```

## Estado

| Fase | Descripción | Estado |
|------|-------------|--------|
| **F0** | Cimientos: monorepo, kernel inmutable, telemetría, design tokens, CI | ✅ |
| **F1** | MVP: LLM local (chat+RAG), UI Next.js, control-plane (auth+licencias) | ✅ núcleo |
| **F2** | Cerebro: orquestador ReAct + herramientas + memoria persistente | ✅ |
| **F3** | Skills WASM (sandbox deny-all) + como herramientas del agente | ✅ |
| **F4** | Memoria darwiniana: ciclo de "sueño" (decay/fusión/poda) | ✅ núcleo |
| **F5** | Auto-mejora gated: sandbox→tests→canary→rollback + circuit breaker | ✅ núcleo |
| **F6** | Móvil (Capacitor) + sync E2E | ⏳ |

Pendiente de productización: Stripe real · Postgres · mistral.rs embebido · build
firmado del .app · browser agéntico · que el LLM *genere* las candidatas de evolución.

### Subcomandos de `aion-core` (todos verificados en vivo, LLM local)
```
chat <prompt>      chat con razonamiento (streaming)
rag <query>        RAG sobre documentos
agent <task>       agente ReAct con herramientas (calculadora, sum_to, memory_search)
skill <n>          ejecuta una skill WASM en sandbox
remember <texto>   guarda en memoria persistente
recall <query>     recupera de memoria persistente
sleep              consolidación darwiniana (fusión/poda con snapshot)
evolve             demo del bucle de auto-mejora gated
serve [addr]       puente HTTP local (/api/chat, /api/agent) para la UI
```

## Licencia
MIT OR Apache-2.0
