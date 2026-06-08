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
**F0 — Cimientos** ✅ en curso (scaffolding, kernel, telemetría, design tokens, CI).
Roadmap F0→F6 en el plan maestro.

## Licencia
MIT OR Apache-2.0
