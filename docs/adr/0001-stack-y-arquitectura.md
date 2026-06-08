# ADR-0001: Stack y arquitectura base de AION

- **Estado:** aceptado
- **Fecha:** 2026-06-08
- **Decisores:** Ariel Marquez

## Contexto
AION debe ser local-first, multiplataforma (macOS/Windows/Android/iOS), multiusuario (SaaS),
autónomo y auto-evolutivo, con estética premium. Decisiones tomadas con investigación verificada
(ver `docs/RESEARCH_2026-06.md` y `docs/RESEARCH_consciencia_creatividad_2026-06.md`).

## Decisión
- **Ejecución:** local-first (cómputo+datos en dispositivo) + plano de control nube mínimo (auth/billing/sync E2E).
- **Núcleo:** Rust, monolito modular (un binario `aion-core` con crates por dominio); núcleo `aion-kernel` inmutable.
- **UI:** Web (Next.js/React/Tailwind) vía Tauri (desktop) + Capacitor (móvil) — una sola UI.
- **LLM:** abstracción `LlmEngine` → Ollama (F1) → mistral.rs embebido (F2) → MLX/Candle móvil (F6).
- **Memoria:** LanceDB; **Skills:** Extism (WASM, deny-all); **Sync:** Automerge (CRDT, E2E).
- **Auto-modificación:** autónoma pero en sandbox WASM + bucle gated (tests/canary/rollback) + kernel inmutable + circuit breakers.
- **Control-plane:** Axum + Postgres + Redis + Stripe + licencias Ed25519 validables offline.

## Alternativas consideradas
| Opción | Pros | Contras |
|--------|------|---------|
| UI Flutter | nativo, un código | reconstruir design system; no reusa la estética web |
| Cloud-centric | máxima potencia | rompe privacidad/coste; no es local-first |
| Frameworks de agentes (LangGraph/Anda) | rápido | menos control; Anda atado a blockchain |
| Auto-modificación sin gates | "evolución" pura | riesgo destructivo (aviso DGM) |

## Consecuencias
- (+) Un núcleo Rust único para las 4 plataformas; privacidad por diseño; IP en memoria darwiniana.
- (−) Dependencia temporal de Ollama en F1 (mitigada por el trait `LlmEngine`).
- Riesgo: lifelong learning sin olvido catastrófico es problema abierto (F4 = I+D honesto).
