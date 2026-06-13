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

## Verificación

```bash
# Unit tests (sin Ollama)
cargo test -p aion-core mcp_compact language_detector
cargo test -p aion-memory

# End-to-end con Gemma local (requiere Ollama arriba) — mide ahorro real ES→EN
python3 scripts/verify_mcp_compact.py
```

**Medición previa (tiktoken, traducción fiel manual)**: 244 tok ES → 138 tok EN = **43%**.
Pendiente: confirmar con Gemma real vía `verify_mcp_compact.py` (Ollama estaba cerrado
al implementar).

---

## Economía honesta

~43% sobre la memoria que entra por MCP. Por sesión: si llamo `aion_memory_search` unas
pocas veces (~600 tok/llamada de memoria), el ahorro son cientos de tokens/sesión —
modesto en €, pero **se acumula en cada sesión, gratis tras calentar la caché**, y no
añade latencia (precomputado) ni riesgo (fail-open a español). Es la optimización
correcta y de bajo riesgo para este puente.

---

## Siguientes incrementos (opcionales)

1. **`aion_brief` y `aion_library_search`** por el mismo `compact_for_bridge`.
2. **Warmer idle acotado**: pre-traducir los N recuerdos más recientes al arrancar
   (con límite de concurrencia) para que la caché no dependa solo del *lazy-on-miss*.
3. **Invalidación**: si un recuerdo se edita, su hash cambia → nueva entrada; las viejas
   se pueden podar por tamaño/LRU si la caché crece.
