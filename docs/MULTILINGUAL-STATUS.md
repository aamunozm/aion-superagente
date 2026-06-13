# Optimización de tokens multilingüe — Estado

**Rama**: `feature/multilingual-memory`
**Objetivo real**: reducir los tokens que la memoria de AION consume en el **puente MCP
hacia Claude Code** (Anthropic API, de pago). En el chat local con Gemma los tokens son
gratis: ahí no se optimiza nada.

---

## El giro (por qué cambió el diseño)

La primera implementación (commits 68cf382…73bbe3d) apuntó mal:

- Atacaba la **ruta local de Gemma**, donde los tokens **no cuestan**.
- "Comprimía" con `TfidfCompressor::compress_to_english()`, que **no traduce**: quitaba
  *stopwords inglesas* de texto **español**. Medido sobre un recuerdo real: 16 de 33
  palabras eran stopwords españolas que el filtro inglés no reconocía → conservaba el
  25% de tokens "raros" → **word-salad español** inyectado en el prompt de Gemma.
- El 80% del código (`MultilingualMemory`, `index_document`, `compressed_en`) era
  **código muerto**: nunca se llamaba en producción.

Se revirtió la ruta Gemma (vuelve a recibir español íntegro) y se borraron los
compresores. Lo que sobrevive: el enum `Language` y el detector heurístico.

---

## Diseño correcto: el idioma se ata al CONSUMIDOR

| Consumidor | Idioma servido | Por qué |
|---|---|---|
| **Gemma (chat local)** | Español íntegro | Tokens gratis; traducir solo degradaría |
| **Claude Code (puente MCP)** | Inglés equivalente | Tokens de pago; inglés ≈ **40% menos** (medido con tiktoken sobre recuerdos reales) |

**Cómo se genera el inglés** (`apps/aion-core/src/mcp_compact.rs`):

1. Lo traduce **Gemma local** (gratis), fiel y literal (`temp 0.1`), preservando
   hechos/nombres/números/rutas. NO resume agresivo: el 40% ya viene de la tokenización.
2. **Precomputado y cacheado** por SHA-256 del contenido en
   `~/Library/Application Support/AION/mcp_compact_en.json`. Nunca se traduce en caliente
   dentro de la llamada MCP (eso metería latencia de Gemma a cada búsqueda).
3. **Fail-open absoluto**: en *cache miss* sirve el español original ESTA vez y dispara
   la traducción en segundo plano (`tokio::spawn`) para la próxima. Si Ollama está
   cerrado o la traducción falla, simplemente se sirve español. Nunca bloquea ni corrompe.
4. Preserva la etiqueta de procedencia (`[hecho]`, `[aprendizaje]`…) sin traducirla.

**Conectado**: `aion_memory_search` (coste repetido por consulta) y `aion_brief`
(~450 tok, coste GARANTIZADO una vez por sesión). Ambos son los únicos consumidores de
memoria del puente; `aion_library_search` queda como siguiente incremento.

### Cuándo se materializa el ahorro (honesto)

El patrón es *lazy-warm*: en la PRIMERA aparición de un recuerdo hay *cache miss* → se
sirve español y se traduce en background. El ahorro real llega cuando ese recuerdo se
vuelve a servir (misma sesión si se repite, o la siguiente). Por eso se conectó el
`brief`: es lo que se pide en CADA sesión, así que tras la 1ª se sirve ya en inglés.
Un *warmer* idle acotado (pre-traducir los recuerdos recientes al arrancar) haría que
incluso la 1ª sesión ahorre — pendiente, con cuidado de no competir con Gemma del chat.

---

## Estado del código

| Archivo | Qué hace | Estado |
|---|---|---|
| `apps/aion-core/src/language_detector.rs` | `has_spanish_signal()`: gate barato y robusto | ✅ 2 tests sobre datos reales |
| `apps/aion-core/src/mcp_compact.rs` | Traducción Gemma + caché SHA-256 + fail-open | ✅ 4 tests |
| `apps/aion-core/src/claude_mcp.rs` | `aion_memory_search` usa `compact_for_bridge()` | ✅ |
| `apps/aion-core/src/claude_code.rs` | `build_brief` compacta sus líneas de memoria | ✅ |
| `apps/aion-core/src/serve.rs` | Ruta Gemma revertida a español íntegro | ✅ |

Borrado: `compressor.rs`, `tfidf_compressor.rs`, `multilingual.rs` (enum `Language`),
`detect_language` (detector frágil), `MultilingualMemory`, `shared_multilingual_memory`,
`index_document`.

### Bug encontrado y corregido en la 2ª verificación

`detect_language` usaba un umbral de densidad (`spanish_norm > 0.05`) que clasificaba
como **inglés** al español **técnico** (pocos acentos, muchos anglicismos) — justo los
recuerdos que Claude Code recupera. Test reproductor: el recuerdo real de auth/CORS
(`[hecho] El pendiente crítico … autenticación … CORS … puerto 8765 …`) salía `English`
→ **se saltaba la traducción**. Se reemplazó por `has_spanish_signal()`, sesgado a
traducir (basta acento/ñ, ¿¡, o dos palabras función), validado sobre 5 recuerdos
técnicos reales (todos → traducir) y 3 notas inglesas (todas → saltar).

---

## Verificación (E2E REALIZADA 2026-06-14)

```bash
# Unit tests (sin Ollama)
cargo test -p aion-core mcp_compact spanish_signal
cargo test -p aion-memory

# Traducción aislada con Gemma local — mide fidelidad y ahorro ES→EN
python3 scripts/verify_mcp_compact.py
```

**Verificado de punta a punta** levantando el binario nuevo en un puerto alterno y
llamando al `/mcp aion_memory_search` real:

1. **El pipeline funciona**: 1ª llamada (caché fría) → español servido (*fail-open*,
   nunca rompe) + traducción disparada en background; 2ª llamada → **inglés**. La caché
   `mcp_compact_en.json` se pobló con 11 traducciones reales de Gemma.
2. **Fidelidad excelente**: puerto 8765, fechas y nombres intactos; los tags de
   procedencia (`[reflexión]`, `[proyecto: aion]`) se preservan sin traducir.
3. **Bug `think:false`**: `gemma4-reason` es un modelo de RAZONAMIENTO; sin ese flag
   gasta todo el presupuesto "pensando" y devuelve vacío. El código Rust YA lo envía
   (`ollama.rs:111` → `mcp_compact` pone `think:false`); el fallo era solo del script.

### Números REALES medidos (tiktoken cl100k)

| Medición | Ahorro |
|---|---|
| Traducción aislada, prosa española limpia (3 recuerdos) | **28%** (137→99 tok) |
| **Payload MCP real** de este usuario (11 recuerdos, query "Rust núcleo") | **12%** (1048→922 tok) |

El 28% aislado **no** se traslada al 12% real porque la memoria ACTUAL de este usuario
está dominada por notas técnicas **cargadas de inglés** (ADR-0004, MultilingualMemory,
BGE-M3, hashes de commit, código): traducirlas ahorra poco. **El ahorro escala con la
densidad de prosa española**; una memoria más personal/de negocio se acercaría al 28%.

---

## Economía honesta

**~12% sobre el payload MCP real de hoy** (no el 28% aislado ni el 43% que estimé al
principio). Para la memoria actual —muy técnica y anglosajona— el ahorro es modesto:
~126 tok menos en una respuesta de `aion_memory_search` de ~1048. Se acumula en cada
sesión y es gratis tras calentar la caché, sin latencia (precomputado) ni riesgo
(fail-open a español). Crecerá si la memoria se vuelve más española.

**Veredicto sincero**: la optimización es correcta, segura y está verificada, pero su
impacto económico es pequeño para este perfil de memoria. Vale por ser local-first y de
riesgo cero; no esperes una factura notablemente menor. Si el objetivo fuera un ahorro
grande, el lever no es traducir sino **recuperar menos y mejor** (subir el umbral de
score del puente, devolver 3 hits en vez de 8) — eso sí recorta tokens de golpe.

---

## Autocontención del runtime (Ollama embebido)

`apps/aion-core/src/ollama_runtime.rs` — al arrancar `serve`, AION garantiza que haya un
Ollama escuchando usando su binario EMBEBIDO (`…/Contents/Resources/ollama-runtime/ollama`),
sin depender de instalaciones externas:

- **Idempotente**: si `:11434` ya responde (Ollama del usuario u otro), lo reutiliza —
  NO lanza un segundo. Verificado: *"Ollama ya responde — reutilizo"*.
- **Auto-arranque**: si está caído, lanza el embebido y hace health-check (~0.5 s en la
  prueba). Verificado de punta a punta.
- **Apagado limpio**: ante SIGTERM/Ctrl-C, termina SOLO el Ollama que lanzó él; uno
  externo del usuario no se toca. Verificado.
- **Fail-open**: si no encuentra el binario o no arranca, AION sigue sirviendo.
- Usa health real (`/api/tags`), no "¿hay proceso?": detecta el caso "proceso vivo pero
  puerto sin servir" que observamos con el Ollama que lanza el desktop.

Nota honesta: el wrapper `aion-desktop` YA lanza el Ollama embebido, así que en uso normal
esto es **defensa en profundidad** + hace que `aion-core serve` sea **autosuficiente en
modo CLI/headless**. No corrige un agujero crítico del flujo de escritorio.

## Siguientes incrementos (opcionales)

1. **`aion_brief` y `aion_library_search`** por el mismo `compact_for_bridge`.
2. **Warmer idle acotado**: pre-traducir los N recuerdos más recientes al arrancar
   (con límite de concurrencia) para que la caché no dependa solo del *lazy-on-miss*.
3. **Invalidación**: si un recuerdo se edita, su hash cambia → nueva entrada; las viejas
   se pueden podar por tamaño/LRU si la caché crece.
