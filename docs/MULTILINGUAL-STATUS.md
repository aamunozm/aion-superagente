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
| **Gemma (chat local)** | Español/italiano íntegro | Tokens gratis; traducir solo degradaría |
| **Claude Code (puente MCP)** | Inglés equivalente | Tokens de pago; inglés ≈ **40% menos** (medido con tiktoken sobre recuerdos reales) |

**Idiomas de origen**: español **e italiano** (Ariel es chileno viviendo en Italia), ambos
~40% más caros en tokens que el inglés. El gate `language_detector::needs_english_translation`
detecta los dos (acentos agudos/ñ/¿¡ del español, graves à è ì ò ù del italiano, o ≥2 palabras
función de cualquiera). Medido: italiano→inglés ahorra ~23% (hasta 28% en prosa limpia).

**Cómo se genera el inglés** (`apps/aion-core/src/mcp_compact.rs`):

1. Lo traduce un **modelo local** (gratis), **meaning-first** (`temp 0.1`): primero entiende
   la INTENCIÓN —resuelve typos, jerga/regionalismos e idioms por su sentido— y luego la
   expresa en inglés, sin traducir palabra por palabra, preservando hechos/nombres/números/
   rutas tal cual (mini-MAPS, arXiv 2305.04118; ver `docs/auditoria-interpretacion-*`). NO
   resume agresivo: el 40% ya viene de la tokenización. El modelo es **configurable**
   (`provider.translation_model` o env `AION_TRANSLATION_MODEL`): por defecto cae al de fondo,
   pero puedes enchufar un traductor especializado (TranslateGemma/GemmaX2) sin tocar código.
2. **Precomputado y cacheado** por SHA-256 del contenido en
   `~/Library/Application Support/AION/mcp_compact_en.json`. Nunca se traduce en caliente
   dentro de la llamada MCP (eso metería latencia de Gemma a cada búsqueda).
3. **Fail-open absoluto**: en *cache miss* sirve el español original ESTA vez y dispara
   la traducción en segundo plano (`tokio::spawn`) para la próxima. Si Ollama está
   cerrado o la traducción falla, simplemente se sirve español. Nunca bloquea ni corrompe.
4. Preserva la etiqueta de procedencia (`[hecho]`, `[aprendizaje]`…) sin traducirla.
5. **QE por back-translation** (Fase 2): tras traducir, se traduce de vuelta al idioma origen
   y se compara con el original vía BGE-M3. Si la similitud cae bajo umbral (def. 0.50,
   `AION_TRANSLATION_QE_MIN`; `AION_TRANSLATION_QE=0` desactiva), NO se confía: se cachea y
   sirve el ORIGINAL (fiel) en vez del inglés. Red de seguridad GRUESA —calibrada con datos:
   lo coloquial-correcto baja a ~0.67, un error catastrófico a ~0.34— atrapa desastres
   (alucinación, tema cambiado), no errores sutiles (eso lo cubre el meaning-first). NO usa
   auto-juicio del LLM (sobreestima); compara significados con embeddings.

**Conectado**: `aion_memory_search` (coste repetido por consulta), `aion_brief`
(~450 tok, coste GARANTIZADO una vez por sesión) y `aion_library_search` (pasajes de
documentos, vía `compact_grounding()` que traduce solo la prosa y conserva la estructura
`[N] (fuente: …)`/`[tema: …]`). Son los tres consumidores de memoria/biblioteca del puente.

### Cuándo se materializa el ahorro (honesto)

El patrón base es *lazy-warm*: en la PRIMERA aparición de un recuerdo hay *cache miss* → se
sirve español y se traduce en background. El ahorro real llega cuando ese recuerdo se
vuelve a servir. **Resuelto con un warmer de arranque** (`mcp_compact::warm`, lanzado en
`serve.rs` ~25 s tras arrancar): pre-traduce los N recuerdos recientes (def. 40) a las dos
longitudes que truncan los consumidores (180 brief / 300 memory_search), respetando el gate
de 1 traducción a la vez para no competir con el chat. Así **incluso la 1ª consulta de la
sesión sirve inglés**. Desactivable con `AION_MCP_WARM=0`; tamaño con `AION_MCP_WARM_N`.

---

## Estado del código

| Archivo | Qué hace | Estado |
|---|---|---|
| `apps/aion-core/src/language_detector.rs` | `needs_english_translation()`: gate ES **e IT** | ✅ 3 tests sobre datos reales |
| `apps/aion-core/src/mcp_compact.rs` | Traducción Gemma + caché SHA-256 + fail-open + `compact_grounding()` + `warm()` | ✅ 5 tests |
| `apps/aion-core/src/claude_mcp.rs` | `aion_memory_search` y `aion_library_search` compactan | ✅ |
| `apps/aion-core/src/claude_code.rs` | `build_brief` compacta sus líneas de memoria | ✅ |
| `apps/aion-core/src/serve.rs` | Ruta Gemma en español íntegro + warmer de arranque | ✅ |

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
| **Corpus REAL completo, totalmente calentado** (59 recuerdos vigentes, 2026-06-14) | **~14%** — brief 14.1% (2675→2297 tok) · memory_search 13.9% (3730→3213 tok) |

El último número (`scripts/measure_mcp_real.py`) es el más representativo: mide los **59
recuerdos reales** truncados igual que el puente y traducidos al 100 % (sin *cache miss*).
Los 59 tienen señal española pero siguen densos en identificadores/código en inglés → ~14 %.

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

- ✅ **`aion_brief` y `aion_library_search`** conectados (este último vía `compact_grounding`).
- ✅ **Warmer de arranque acotado** (`mcp_compact::warm`, gate de 1 a la vez) — la caché ya
  no depende solo del *lazy-on-miss*; la 1ª consulta de la sesión ya sirve inglés.

Pendiente:

1. **Invalidación**: si un recuerdo se edita, su hash cambia → nueva entrada; las viejas
   se pueden podar por tamaño/LRU si la caché crece (hoy: descarte arbitrario al pasar 10 000).
2. **Toggle de UI** + dashboard de ahorro acumulado por sesión.
