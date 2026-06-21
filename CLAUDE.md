# AION — Guía de proyecto para Claude Code

> Super-agente de IA **local-first** con mente observable. Núcleo en **Rust**, UI en **Next.js 15**, LLM 100% local (**Ollama / Gemma 12B**, embeddings BGE-M3). Todo corre on-device. Coste de inferencia: cero.

## ⚠️ Carpeta canónica
El proyecto es **esta carpeta**: `~/Desktop/Proyecto-AI-Local` (con guión). Existió una carpeta gemela `Proyecto AI Local` (con espacio) que solo tenía config y **debe quedar en desuso** — abre Claude Code siempre desde aquí. Ambas comparten la misma memoria de Claude Code (mismo slug), así que no se pierde historial al migrar.

## Arquitectura (monorepo)
- **Rust workspace** (`Cargo.toml`, resolver 2): 13 crates en `crates/` + apps en `apps/`.
  - Crates núcleo: `aion-kernel`, `aion-llm`, `aion-memory`, `aion-orchestrator`, `aion-cognition`, `aion-evolution`, `aion-skills`, `aion-browser`, `aion-sync`, `aion-telemetry`, `aion-computer`, `aion-control`, `aion-control-client`.
  - Apps Rust: `apps/aion-core` (backend, binario `aion-core`), `apps/control-plane`.
- **JS workspace** (pnpm + turbo): `apps/web` (Next.js 15, UI principal), `apps/mobile`, `apps/desktop`, `packages/*`.

## Comandos
```bash
# Backend (Rust) — API local en http://127.0.0.1:8765
cargo build --release --workspace
./target/release/aion-core serve

# UI web — http://localhost:3000
pnpm install && pnpm --dir apps/web dev

# Monorepo JS (turbo)
pnpm build   # turbo run build
pnpm lint    # turbo run lint
pnpm dev     # turbo run dev

# Git hooks (una vez tras clonar): pre-commit formatea Rust
sh scripts/setup-hooks.sh
```
Requisitos: macOS Apple Silicon · Ollama (Gemma 12B) · Rust estable (≥1.85) · Node 20+ con pnpm.

## Convenciones
- Rust 2021, `lto = "thin"` en release. Formateo automático vía pre-commit hook — no commitees sin formatear.
- Licencia dual MIT/Apache-2.0. Autor: Ariel Marquez <info@prontoclick.it>. Repo: prontoclick/aion.
- El LLM es intercambiable tras el trait `LlmEngine` (Ollama hoy; mistral.rs/MLX en roadmap).

## Documentación viva
- `README.md` / `USAGE.md` — visión y uso.
- `docs/PRD.md`, `docs/GOVERNANCE.md`, `docs/DESIGN_TOKENS.md`, `docs/adr/` — decisiones y diseño.
- `docs/auditoria-*.md` — auditorías previas (sistema y MCP).

## Memoria compartida con AION (MCP `aion`)
AION corre localmente y expone memoria vía MCP (`mcp__aion__*`) en `http://127.0.0.1:8765`.
- Antes de asumir contexto de un proyecto/decisión: `aion_brief` o `aion_memory_search` (pasa siempre `project: "aion"`).
- Al cerrar una decisión durable (stack, arquitectura, acuerdo): `aion_remember` con `project: "aion"`, conciso y autocontenido.
- Si las tools no responden, AION está cerrado: continúa sin él, no es un error.

## Pendientes conocidos (de auditorías)
- ✅ Resuelto: **auth + CORS** de la API local `:8765` (P0-1 fase 1+2): `local_guard` (Host anti-DNS-rebinding + Origin obligatorio en mutaciones), Bearer local timing-safe (`require_api_token`) en todo `/api/*`, CORS allowlist de orígenes locales. Ver `docs/auditoria-*` y la memoria `aion-auditoria-2026-06`.
- Parcial: **concurrencia inter-proceso** del JSONL de memoria — `aion remember` ahora RUTEA por HTTP al daemon (`POST /api/memory/remember`) si está vivo → escritor único, cierra el lost-update; si no hay daemon, escritura directa (sin rival). Falta lo mismo para `aion sleep` (consolidación más compleja: aún escribe directo).
