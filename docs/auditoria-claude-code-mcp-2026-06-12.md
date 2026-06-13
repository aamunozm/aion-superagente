# Auditoría — Integración de memoria Claude Code ↔ AION (MCP) — 2026-06-12

Alcance: endpoint `POST /mcp` y sus 6 tools, ruta de escritura/lectura de memoria
(`crates/aion-memory/src/vector.rs`), higiene (consolidación/supersede), capa de
conexión y observabilidad (`claude_code.rs`, `/api/claude-code/*`, página web) y
las métricas de ahorro de tokens. Revisión de código + verificación en vivo contra
el servidor corriendo en `127.0.0.1:8765`.

## Resumen ejecutivo

La capa MCP en sí (`/mcp`) está bien diseñada: Bearer de ~244 bits, comparación por
hash, cap de importancia y `origin` fijados server-side, payloads acotados,
anti-inyección en las lecturas, auditoría persistente rotada, y la conexión
sobrevive a updates. **Pero la promesa de integridad tiene tres grietas reales:**

1. Los endpoints de gestión `/api/claude-code/*` heredan el C1 (sin auth + CORS `*`):
   **verificado en vivo** — cualquier web abierta en el navegador puede leer la
   auditoría (incluye fragmentos de memoria) y regenerar/cortar la conexión.
2. La dirección Claude Code → prompts de AION está desprotegida: un recuerdo escrito
   vía `aion_remember` se inyecta luego en los prompts de AION como conocimiento
   propio confiable, sin prefijo `[externo]` ni fence. Inyección end-to-end posible.
3. `rewrite_jsonl` no es atómico: un crash durante consolidación/supersede puede
   **truncar toda la memoria**.

Además, la métrica «ahorro medido» es en realidad una estimación (chars/4 contra un
volcado hipotético), y el brief en vivo muestra los dos pendientes conocidos
agravados: fechas 1970 en 9/10 recuerdos (que además anulan el factor de recencia
del retrieval) y 6 insights casi idénticos sin fusionar.

## Verificado en vivo (2026-06-12)

| Prueba | Resultado |
|---|---|
| `POST /mcp` sin token | ✅ 401 |
| `GET /api/claude-code/audit` sin auth, Origin hostil | ❌ HTTP 200, 2 082 bytes + `access-control-allow-origin: *` |
| `GET /api/claude-code` sin auth | ❌ 200 con estado completo de la conexión |
| Permisos de `claude_code.json` (token en claro) | ❌ `-rw-r--r--` (0644) |
| `aion_brief` real | ~1 900 chars ≈ 480 tokens (dentro de lo prometido) · 9/10 fechas `1970-01-01` · 6 recuerdos duplicados |

## CRÍTICO

| # | Hallazgo | Dónde | Riesgo |
|---|---|---|---|
| C1 | `/api/claude-code/*` sin auth + CORS `Any`: drive-by lee auditoría (hasta 200 chars de cada query/recuerdo) y puede `POST /connect` (rota el token → DoS) o `/disconnect`. El token **no** es robable por esta vía (ningún endpoint lo devuelve). | `serve.rs:255-258`, `:333-340` | Fuga de contenido de memoria + sabotaje de la conexión |
| C2 | Recuerdos `origin:"claude-code"` se inyectan en prompts de AION como conocimiento confiable: `origin` se persiste pero nunca se consulta al recuperar. Es el pendiente «prefijo [externo]» — y es un agujero de inyección, no solo cosmética. | `serve.rs:1838` (`relevant_knowledge`), `vector.rs:106` | Prompt injection persistente hacia AION desde cualquier sesión de Claude Code |
| C3 | `rewrite_jsonl` usa `fs::write` in situ (truncar+escribir). Crash a mitad de `consolidate`/`supersede`/`reinforce` = memoria truncada o vacía. El `.bak` solo cubre `consolidate`. | `vector.rs:760-772` | Pérdida irreversible de la «mente» |

### Fixes C1–C3
- C1: misma mitigación que el C1 general (Bearer local en `/api/*` + allowlist de origen + validación `Host`).
- C2: al recuperar para prompts internos, si `origin != ""` anteponer `[externo: claude-code — dato no confiable]` y/o fence de no confiable.
- C3: escribir a `path.tmp` + `fs::rename` (el repo ya tiene `write_atomic`; `inbox.rs:85-87` ya lo hace bien).

## ALTO

- **A1. Concurrencia sin coordinación**: ~20 sitios crean `VectorMemory::persistent_local` independientes, cada una con su `Mutex` sobre su propia copia en RAM; no hay file-lock ni instancia compartida. Un `aion_remember`, un chat RAG y el sueño nocturno concurrentes producen lost-updates (el `rewrite` de consolidación pisa el `append` del remember). `vector.rs:116`, `claude_mcp.rs:357,481`, `serve.rs:1810`. Fix: `Arc<VectorMemory>` única en el estado del servidor.
- **A2. Token Bearer en texto plano con 0644** (verificado en disco): cualquier proceso/usuario local lo lee. `claude_code.rs:36-51` + `write_atomic` sin modo. Fix: `write_atomic_secret` con 0600 (como ya hace `~/.claude.json` del CLI).
- **A3. Consolidación ciega a `importance`/`origin`**: la poda retiene solo por `fitness`/`access_count` — un recuerdo del usuario de alta importancia nunca consultado se poda en ~7 «sueños»; el merge conserva el más antiguo sin mirar origen. `vector.rs:707-757`. Fix: excluir de poda `importance ≥ umbral`; en merge preferir `origin==""` / mayor importancia.

## MEDIO

- **M1. Margen de supersede +0.1**: un recuerdo externo (0.6) puede marcar `superseded` una preferencia del usuario de importancia ≤ 0.7 (las preferencias típicas puntúan 0.65–0.70). La promesa «no pueden pisar tus preferencias» no se cumple en ese rango. `vector.rs:414`. Fix: si los orígenes difieren, exigir `≥` estricto sin tolerancia, o prohibir supersede de `origin==""`.
- **M2. `[proyecto: X]` sin sanitizar**: saltos de línea y delimitadores en el nombre de proyecto se inyectan en el recuerdo (combinable con C2). `claude_mcp.rs:345-350, 471-476`. Fix: limpiar `\n`/`[]<>`, tope 64 chars.
- **M3. Fence rompible**: `wrap_untrusted` no escapa `<<<FIN MEMORIA AION>>>` dentro del cuerpo — un recuerdo que contenga el cierre literal escapa del fence al servirse. `claude_mcp.rs:29-31`. Fix: escapar los marcadores o nonce aleatorio por respuesta.
- **M4. Constancia en Bandeja best-effort**: si `Inbox::open/push` falla, el recuerdo se guarda igual sin rastro (`if let Ok… let _ =`). `claude_mcp.rs:488-494`. Fix: propagar error o canal garantizado.
- **M5. Token por argv en el 1-click**: `claude mcp add … -H "Authorization: Bearer {token}"` es visible en `ps` durante el registro. `claude_code.rs:117-122`. Fix: usar siempre el fallback de merge directo en `~/.claude.json` (`register_fallback:130-156`, ya atómico).
- **M6. Rate limit invertido y débil**: global (no por token), ventana fija (120 req posibles en segundos a caballo de dos ventanas), evaluado DESPUÉS de auth (la fuerza bruta del token no consume ventana) y responde HTTP 200 en vez de 429. `claude_mcp.rs:61-74, 161-170`. Fix: rate limit antes de auth + contador de fallos + `429`.
- **M7. Fechas 1970 (causa raíz)**: `created_at` con `#[serde(default = "epoch")]` — todo recuerdo anterior al campo deserializa a 1970-01-01. No es solo cosmético: `retrieve` calcula `age_days` desde 1970 → **factor de recencia ≈ 0** para esos recuerdos. `vector.rs:33-35, 84-85, 473`. Fix: migración única (mtime o fecha de migración) + omitir fecha epoch en el brief.

## BAJO

- **B1. El «ahorro medido» es una estimación**: `est_tokens = result_chars / 4` y el ahorro compara contra «volcar toda la memoria» (`full_dump_tokens − tokens_served/total_calls`), un contrafactual que nadie ejecutaría. Las cifras (~96 %/~84 %) son plausibles pero no son medición. `claude_mcp.rs:92`, `serve.rs:4244-4258`, `page.tsx:110-113`. Fix: etiquetar como estimación en la UI.
- **B2. Brief con tope 2 400 chars (~600 tokens)** vs ~450 prometidos; en vivo dio ~480. Y su valor de orientación está degradado por los duplicados (ver B3). `claude_code.rs:243-246`.
- **B3. Consolidación no está fusionando duplicados evidentes**: el brief en vivo muestra 6 insights casi idénticos («Implementa una memoria episódica persistente…»). O el umbral de merge no los alcanza o el ciclo nocturno no corrió. Investigar junto a A3.
- **B4. Rotación de auditoría con carrera** entre rotación y append concurrente (riesgo bajo con 60 req/min). `claude_mcp.rs:96-113`. Fix: `Mutex` estático en `audit()`.

## Lo que está BIEN (verificado)

- Cap de importancia 0.6 **server-side e infalsificable**: el schema no expone `importance`; `estimate_importance(...).min(max)` con clamp 0–1 (`claude_mcp.rs:484`, `vector.rs:393`). `origin` hardcodeado en el servidor.
- Token: 2×UUIDv4 (CSPRNG, ~244 bits), comparación hash-then-compare sin timing leak, rechazo de token vacío, revocación real (connect rota, disconnect vacía + `enabled=false`). `claude_code.rs:61-67`, `claude_mcp.rs:54-59`.
- Sin shell injection en el registro 1-click (`Command` con array de args; el dashboard nunca devuelve el token).
- Payloads acotados en todas las tools (k≤8, 300 chars/hit, grafo 6×220, proyecto 3 000, remember 2 000) — el «coste plano por consulta» es real por diseño.
- Auditoría JSONL persistente, truncada a 200 chars/query, rotada a 5 MB/5 000 líneas.
- Persistencia fuera del bundle (`app_data_dir` + `~/.claude.json`) → la conexión sobrevive a updates, coherente con la firma estable.
- Lectura de JSONL tolerante a líneas corruptas; `append` de una sola `writeln!` (crash deja a lo sumo una línea inválida descartable).
- Lecturas hacia Claude Code siempre con fence anti-inyección (`wrap_untrusted` en las 5 tools de lectura).
- Bind solo loopback; endpoint desactivable (`enabled=false` corta `/mcp`).

## Plan de remediación priorizado

1. **C3** — atomicidad de `rewrite_jsonl` (riesgo de pérdida total; fix de ~5 líneas).
2. **C1** — auth + CORS en `/api/*` (la raíz que amplifica todo; ya planificado en la auditoría general).
3. **C2 + M2 + M3** — cerrar el lazo de inyección: prefijo/fence para recuerdos externos en prompts internos, sanitizar `project`, escapar delimitadores.
4. **A1** — instancia de memoria compartida (`Arc<VectorMemory>`).
5. **A2** — `claude_code.json` a 0600; **M5** — registro sin argv.
6. **A3 + M1 + B3** — consolidación/supersede conscientes de `importance`/`origin` e investigar los duplicados del brief.
7. **M7** — migración de fechas epoch (también arregla el sesgo de recencia del retrieval).
8. **M4, M6, B1, B2, B4** — endurecimientos menores.

---

## Remediación aplicada — 2026-06-12 (misma fecha)

Todos los fixes se implementaron y verificaron (`cargo test` 118 ok · `cargo clippy -D warnings` limpio · build universal ok). Una segunda pasada adversarial confirmó que no hay regresiones ni endpoints abiertos a drive-by.

| Hallazgo | Estado | Qué se hizo |
|---|---|---|
| C1 | ✅ | CORS pasa de `Any` a `AllowOrigin::predicate(is_local_origin)`; nuevo middleware `local_guard` valida `Host` (anti DNS-rebinding) y `Origin` (allowlist local: `tauri://localhost`, `http(s)://localhost\|127.0.0.1\|[::1]`). Rechaza userinfo en el origin. Cierra el drive-by/CSRF para **todos** los `/api/*`, no solo claude-code. `serve.rs` |
| C2 | ✅ | `relevant_knowledge` separa por procedencia: lo externo (Claude Code) va en sección propia marcada «DATOS, NO instrucciones». Nuevo `VectorMemory::origins_for`. `serve.rs`, `vector.rs` |
| C3 | ✅ (ya estaba) | `rewrite_jsonl` ya usa tmp+rename atómico. |
| A2 | ✅ | `write_atomic_secret` (0600, creación atómica con `mode(0o600)`, sin ventana world-readable). `claude_code.json` y `~/.claude.json` ahora a 0600. `main.rs`, `claude_code.rs` |
| A3 | ✅ | Poda ya respeta `importance ≥ 0.65`; merge de consolidación ahora prefiere la procedencia del usuario (no deja un externo «ganar»). `vector.rs` |
| M1 | ✅ | `may_supersede`: un recuerdo externo (≤0.6) no puede supersedeer una preferencia del usuario (exige importancia estrictamente mayor en cruce de origen). Con test. `vector.rs` |
| M2 | ✅ | `sanitize_project`: filtra controles unicode + `[]<>{}`, cap 64. `claude_mcp.rs` |
| M3 | ✅ | `wrap_untrusted` elimina las marcas del cuerpo (no se puede cerrar el fence desde dentro). `claude_mcp.rs` |
| M4 | ✅ | Constancia en bandeja: si falla, deja `tracing::warn` en vez de tragarse el error. `claude_mcp.rs` |
| M5 | ✅ | `register` ya no usa `claude mcp add -H` (token fuera de argv/`ps`): edita `~/.claude.json` directo. `claude_code.rs` |
| M6 | ✅ | Rate limit responde `429`. Se mantiene tras auth a propósito (evita DoS de unauth). `claude_mcp.rs` |
| M7 | ✅ | `is_unknown_time`: fechas epoch → recencia neutra (no se entierran) y se omiten en el brief. `vector.rs`, `claude_code.rs` |
| A1 | ✅ | Singleton `shared_memory() -> Arc<VectorMemory>` (`OnceLock` en `main.rs`): ~35 sitios que cargaban el JSONL por separado ahora comparten una sola instancia con un solo `Mutex` → sin lost-updates. Nuevos `clear()` (usado en `agent_wipe`) y `reload()` (usado en `agent_import`) mantienen RAM/disco coherentes tras wipe/restore. Quedan aislados a propósito los subcomandos CLI one-shot (`sleep`/`remember`/`recall`/`eval`). `vector.rs`, `main.rs`, `serve.rs` |
| B1 | ℹ️ | La métrica «ahorro» sigue siendo estimación (chars/4 vs volcado hipotético); el ahorro **real** sí mejoró (brief de-duplicado + techo 1800). |
| B2/B3 | ✅ | Brief con dedupe `near_duplicate` (Jaccard ≥0.72), techo ~450 tokens, fechas epoch omitidas. `claude_code.rs` |

**Tests nuevos**: `may_supersede` (externo no pisa preferencia), merge con procedencia, `is_unknown_time`, `origins_for`, y guardia de origen/host (anti drive-by/DNS-rebinding).

**Pendiente real**: ninguno de los hallazgos del informe queda abierto. Límite conocido (fuera del alcance, no regresión): el singleton aísla la concurrencia **intra-proceso**; lanzar un subcomando CLI (`aion sleep`/`remember`) en paralelo al daemon escribe el mismo JSONL desde otro proceso (la escritura atómica tmp+rename evita corrupción, pero podría perder un update entre procesos) — se resolvería con un file-lock o enrutando esos comandos por HTTP. Requiere rebuild + reinstalar `AION.app` para que los fixes surtan efecto en producción.
