# Auditoría integral del sistema AION — 2026-06-12

Auditoría en cuatro frentes ejecutada por agentes independientes: **arquitectura/calidad**,
**seguridad**, **frontend/apps** e **infra/DevOps**. Solo lectura; los fallos de CI de hoy
(clippy + licencias cargo-deny) se corrigieron aparte en `fix(ci)` (commit `12d4fd0`).

**Métricas**: 21.661 LOC Rust (13 crates + 2 apps) · ~6.300 LOC TS · 104 tests Rust, 0 tests JS
· grafo de dependencias acíclico · 2 TODO reales en todo el repo.

---

## Resumen ejecutivo

El núcleo cognitivo de AION tiene una postura defensiva notablemente buena (HITL fail-closed,
anti-inyección, jail de archivos, Keychain, anti-SSRF, cero secretos commiteados) y una
arquitectura de workspace ejemplar. Pero hay **una grieta transversal que invalida la promesa
de privacidad del producto**: la API local `:8765` expone ~60 endpoints sin autenticación y
con CORS abierto a cualquier origen. Cualquier web abierta en el navegador puede disparar el
agente, exportar/borrar la memoria o escribir credenciales. Es el fix nº 1.

El segundo patrón dominante: **arquitectura aspiracional**. `aion-core` concentra el 64% del
código (la inversión app→crates está rota), `packages/design-system` e `ipc-bindings` están
huérfanos o vacíos, las migraciones SQL del control-plane son código muerto, y el dark mode
anunciado en la UI no funciona.

---

## CRÍTICO

| # | Hallazgo | Dónde | Riesgo |
|---|---|---|---|
| C1 | API local sin auth + `CorsLayer::allow_origin(Any)` → CSRF/drive-by desde cualquier web; sin validación de `Host` (DNS rebinding) | `apps/aion-core/src/serve.rs:255-350` | Exfiltración de memoria (`/api/agent/export`), wipe, envenenamiento, escritura de credenciales |
| C2 | Webhook Stripe acepta cualquier POST con 200, sin firma ni idempotencia (mina activada al configurar `STRIPE_SECRET_KEY`) | `apps/control-plane/src/billing.rs:32` | Manipulación de suscripciones/licencias |
| C3 | Control-plane 100% memoria+JSONL; las migraciones Postgres (`infra/migrations/0001_init.sql`) no las aplica nadie (sin sqlx) | `apps/control-plane/src/store.rs:40` | Pérdida de estado de negocio (auth, licencias, billing) ante reinicio/corrupción |
| C4 | Sin backup automatizado de la "mente" (memoria, grafo, audit, skills); el export `.aion` existe pero nada lo ejecuta | `~/Library/Application Support/AION/` | Un JSONL corrupto a mitad de `append` = pérdida irreversible |
| C5 | Dark mode roto: el toggle alterna `.dark` pero ningún CSS lo define (los tokens dark viven en el design system que nadie importa) y el tema no se inicializa en `layout.tsx` | `apps/web/src/app/settings/page.tsx:252` · `globals.css` | La UI promete claro/oscuro y miente |

### Mitigaciones C1 (la raíz que amplifica el resto)
1. Token de sesión local generado al arrancar, exigido como `Authorization: Bearer` en todos los `/api/*` (mismo patrón que ya usa `/mcp`).
2. `allow_origin` restringido a `tauri://localhost` + `http://localhost:3000` (dev).
3. Validar header `Host` ∈ {`127.0.0.1`, `localhost`} contra DNS rebinding.
4. El frontend ya guarda `aion_token` en localStorage y nunca lo envía (`apps/web/src/lib/api.ts`) — cablear el header en el cliente.

---

## ALTO

**Seguridad**
- A1. JWT secret con fallback silencioso a `"dev-insecure-secret-change-me"` → tokens forjables y licencias Ed25519 emitibles para cualquier usuario. Fail-closed: abortar arranque si falta. `apps/control-plane/src/main.rs:21-23`
- A2. CORS `Any` también en el control-plane. `apps/control-plane/src/routes.rs:24-27`

**Arquitectura**
- A3. `aion-core` = 13.887 LOC (64% del Rust): `serve.rs` 4.119 líneas, `main.rs` 2.193, `graph.rs` 1.757 y `agent_tools.rs` 1.709 son lógica de dominio que pertenece a crates. `aion-cognition` (348 LOC) y `aion-sync` (180) casi vacíos.
- A4. `serve.rs` con **cero tests**; 18 de 28 módulos de `aion-core` sin test (incl. `claude_mcp.rs`, `credentials.rs`, `a2a.rs`). `agent()`/`crew()`/`chat()` duplican ~300 líneas de streaming SSE entre sí.
- A5. Auditoría de acciones del computador best-effort silenciosa: `let _ = self.audit.record(...)` — una acción sobre el PC puede ejecutarse sin traza. `crates/aion-computer/src/lib.rs:65`

**Frontend**
- A6. Design system huérfano (0 imports de `@aion/design-system`) e identidad bifurcada: tokens definen "plasma teal `#0FB5BA`", `globals.css` redefine las mismas variables con dorado `#c8a951`. `packages/web` e `ipc-bindings` vacíos; contrato Rust↔TS 100% a mano (sin ts-rs/specta) — hoy sin drift, pero nada lo impide.
- A7. Streams sin `res.ok`; `.catch(() => {})` extendido — incluida `confirmDecision` (api.ts:65): **una aprobación HITL puede perderse en silencio**. Chat: `localStorage.setItem` de toda la conversación + re-render completo por cada token SSE.

**DevOps**
- A8. Toolchain `stable` flotante + `-D warnings` = roturas espontáneas de CI con cada release de Rust (causa exacta del fallo de hoy). Fijar versión en `rust-toolchain.toml`.
- A9. Actions por tag mutable (sin pinning SHA); CI no compila los targets que se shippean (x86_64-apple-darwin, Windows).
- A10. Resiliencia: `openai.rs:33` y `embedder.rs:26` con clients reqwest **sin timeout** (cuelgue indefinido posible); 0 retries/backoff/circuit-breakers en todo el workspace. Buen guardarraíl: `max_steps: 8` en ReAct y timeouts bien razonados en `ollama.rs`.
- A11. Observabilidad: 0 spans, 0 métricas, `infra/observability/` vacío. Imposible responder "¿por qué tardó 40s esta respuesta?".

---

## MEDIO (selección)

- M1. Path traversal vía `project_id` sin sanear (`projects.rs:75-78,201-210`, `serve.rs:3464`) — explotable por C1; aplicar canonicalize+jail como ya hace `FileReadTool`.
- M2. Token A2A: validación omitida si está vacío + comparación no constante (`serve.rs:3824`).
- M3. `run_command` = `sh -c` con input del LLM — mitigado por HITL fail-closed; riesgo residual de habituación del usuario al botón "confirmar".
- M4. `UserStore`: `InMemoryStore` case-sensitive vs `FileStore` case-insensitive en email (tests no reproducen producción). `apps/control-plane/src/store.rs`
- M5. Accesibilidad muy por debajo de AA: 11 aria en toda la app, modales sin `role="dialog"`/focus trap/Escape, scrollbars ocultas, 9 `<label>`.
- M6. Google Fonts vía CDN en producto local-first (filtra metadata, falla offline); `Inter` declarada pero nunca cargada.
- M7. Páginas-monolito: `settings/page.tsx` 681 líneas/25 useState; `workspace` 556/21; `chat` 594. `window.prompt`/`alert()` nativos.
- M8. launchd: rutas hardcodeadas, logs en `/tmp` (legibles por cualquier proceso, se borran al reiniciar); sin LaunchAgent para `serve` ni healthcheck.
- M9. `codesign --deep` (deprecado) sin hardened runtime ni notarización; fallback ad-hoc silencioso que pierde permisos TCC sin que el build falle. `apps/desktop/build-universal.sh:43-49`
- M10. Releases: versión `0.0.1` congelada, sin changelog, tags rodantes como pseudo-releases; CI web sin ESLint ni tests; pre-commit reescribe el stage con `cargo fmt --all`.
- M11. `reqwest`/`axum` declarados por crate (no en `workspace.dependencies`) con features distintas; `AionError` envuelve `String` perdiendo la cadena causal; IO síncrono en handlers async (`graph.rs`, `library.rs`).
- M12. i18n: flash de idioma al montar, claves sin tipar, `lang="es"` estático.

## BAJO

- Fallos silenciados en tools del navegador (`driver.rs:209,249,261` — un click fallido se reporta como éxito al LLM).
- `data/*.bak` con memoria cognitiva sin cifrar en el repo (no trackeados, pero limpiar); verificar historial de `data/license_signing_key.hex` si el repo fue público.
- Rate-limit `/mcp` global (no por token); CSP con `unsafe-inline` en styles; capability Tauri con `args: true`.
- `legacy/gemma4-reasoning` sin limpiar; 1 solo ADR para decisiones mayores (GWT, GAAMA-KG, A2A); docker-compose con credenciales fijas autodescrito "para producción".
- `apps/mobile` cascarón (Capacitor 6 sin plataformas generadas); cero code-splitting; `strip` ausente del perfil release.

---

## Lo que está bien (no romperlo)

- Workspace acíclico con `aion-kernel` como kernel de contratos; `thiserror` consistente, sin `anyhow`, casi cero `unwrap()` reales en producción.
- **HITL fail-closed real** (`react.rs:386-394`): sin canal de confirmación → denegado.
- Credenciales solo en Llavero de macOS; el LLM jamás recibe el valor.
- Anti-prompt-injection con delimitadores en web/MCP/A2A; anti-SSRF en fetch; jail `canonicalize+starts_with(HOME)` en file tools.
- Argon2id + JWT con verificación de exp y tests; audit log append-only bien diseñado.
- TS `strict` impecable (cero `any`); SSE de Mente con reconexión/backoff/cleanup ejemplar; higiene de efectos buena.
- CI Rust básico completo (fmt+clippy+test+rust-cache) y security-audit semanal; `.gitignore` correcto; export/clonado `.aion` bien diseñado.

---

## Plan de acción priorizado

**Fase 0 — esta semana (seguridad, ~2-3 días)**
1. C1: Bearer local en `/api/*` + CORS allowlist + validación de `Host` (backend) y header en `api.ts` (frontend).
2. A1: abortar arranque del control-plane sin `AION_JWT_SECRET`; A2: CORS allowlist.
3. C2: webhook Stripe → 501 incondicional hasta implementar verificación de firma.
4. M1: validar `project_id` (UUID + canonicalize+jail); M2: token A2A obligatorio + comparación constante.
5. A5: `tracing::error!` en fallo de auditoría de `aion-computer` (o hacerla bloqueante).

**Fase 1 — quick wins (1 día)**
6. Timeouts en `openai.rs` y `embedder.rs` (4 líneas). 7. Pinnear toolchain Rust. 8. Pinning SHA de actions + `cargo check` x86_64/Windows en CI. 9. LaunchAgent de backup diario reutilizando el export `.aion` + logs a `~/Library/Logs/AION/`. 10. Borrar `data/*.bak`. 11. `res.ok` + feedback de error en `confirmDecision` y streams.

**Fase 2 — producto (1-2 semanas)**
12. Unificar identidad visual: decidir teal vs dorado, importar `@aion/design-system` y restaurar dark mode (init en `layout.tsx`).
13. Trocear `serve.rs` en `routes/{chat,agent,crew}.rs` + helper SSE común; tests para `serve.rs` y `claude_mcp.rs`.
14. ts-rs sobre los structs de respuesta → `packages/ipc-bindings` real.
15. Chat: persistencia debounced + memo de turnos.
16. Accesibilidad mínima: dialog/menu roles, focus trap, Escape, labels.

**Fase 3 — plataforma (1 mes)**
17. `graph.rs` y `agent_tools.rs` a crates propios; absorber conciencia en `aion-cognition`.
18. Control-plane: sqlx + migraciones reales detrás de `DATABASE_URL` (o decidir explícitamente que JSONL es el diseño y borrar el SQL muerto).
19. Observabilidad: `#[instrument]` en el loop ReAct, métricas de latencia/paso, tokens/s, tasa de fallo de tools.
20. Retries con backoff (distinguiendo connection-refused de read-timeout) en `aion-llm`.
21. Versionado real + CHANGELOG + runbook del release manual de macOS; firma sin `--deep`, de dentro hacia afuera.
