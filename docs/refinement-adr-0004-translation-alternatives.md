# Refinamiento ADR-0004: Alternativas a Traducción API

**Objetivo:** Evaluar opciones de traducción **offline/local** sin costes de API (Google, DeepL, etc.)

**Contexto:** ADR-0004 propone traducir respuestas a español en fallback. Google Translate sería viable pero cuesta. ¿Hay alternativas locales?

---

## Opciones Evaluadas

### Opción 1: SIN TRADUCCIÓN (Zero-cost fallback)

**Concepto:** Si traducción local falla/no existe, devolver respuesta directamente en inglés al usuario.

```
Usuario pregunta: "¿Cómo configuro la API?" (español)
  ↓
AION retrieva contexto (inglés comprimido)
  ↓
Claude responde: "To configure the API..." (inglés)
  ↓
¿Traducir a español?
  └─ SÍ pero es lento/costoso
  └─ NO: Enviar respuesta en inglés tal cual
     (Usuario entiende inglés técnico, de todas formas)
```

**Pros:**
- ✅ Zero overhead (0ms latencia, 0 tokens)
- ✅ Zero coste
- ✅ Coherencia garantizada (Claude genera en inglés)
- ✅ Usuarios técnicos entienden inglés

**Contras:**
- ❌ Experiencia menos natural para user español
- ❌ Mezcla idiomas (pregunta español, respuesta inglés)
- ⚠️  No es "multilingüe" real, es "inglés optimizado"

**Viabilidad:** ALTA (implementable ya, sin dependencias)

---

### Opción 2: m2m100 Local (Ollama)

**Concepto:** Usar modelo m2m100 (Facebook/Meta, 418M params) en Ollama.

**Status:** Investigado — NO está en hub default de Ollama.

```bash
# Intentaría:
ollama pull m2m100  # ❌ No existe

# Alternativas:
ollama run mistral "Translate Spanish to English: ..." 
  └─ Usa Gemma 4 12B para traducción (overkill)
  
ollama pull m2m100-4bit  # ¿Existe?
```

**Pros:**
- ✅ Offline (local, sin API)
- ✅ Open-source (Meta)
- ✅ Multi-lingual (100 idiomas)

**Contras:**
- ❌ No disponible en hub Ollama default
- ❌ Latencia: ~2-5s (m2m100 es lento)
- ❌ Requiere setup manual (no auto-config)

**Viabilidad:** MEDIA (requiere experimentation)

---

### Opción 3: mBART/mT5 Local

**Concepto:** Usar modelos open-source multilingües más pequeños.

**Candidatos:**
- **mBART50** (Facebook, 610M params): español ↔ inglés
- **mT5-small** (Google, 300M params): 101 idiomas
- **Helsinki-NLP/Opus** (Hugging Face, 100-300M): specialized pairs

```rust
// Pseudo-código:
let translator = OllamaTranslator::load("mbart50")?;
let spanish = translator.translate_to("es", english_response)?;
```

**Pros:**
- ✅ Offline
- ✅ Rápido (~500-1000ms, más que m2m100)
- ✅ Tamaño pequeño (300-600M, ~200-300MB descarga)

**Contras:**
- ⚠️  Requiere GGUF quantization (no todos los formatos existen)
- ⚠️  Calidad variable (mBART es OK, pero no state-of-art)
- ❌ Setup manual en Ollama

**Viabilidad:** MEDIA-ALTA (más probable que m2m100)

---

### Opción 4: LibreTranslate (Local Server)

**Concepto:** Ejecutar servidor LibreTranslate en localhost (basado en OpenNMT).

```bash
# Setup:
docker run -ti -p 5000:5000 libretranslate/libretranslate

# Uso:
curl -X POST "http://localhost:5000/translate" \
  -d '{"q":"Hola", "source":"es", "target":"en"}'
```

**Pros:**
- ✅ Offline (server local)
- ✅ API estándar (fácil integración Rust)
- ✅ Mejor calidad que m2m100 (OpenNMT tuned)
- ✅ Soporte 20+ idiomas

**Contras:**
- ❌ Requiere Docker (dependencia)
- ❌ Latencia: 1-3s por request
- ❌ Memoria: ~2-4GB (modelo en RAM)

**Viabilidad:** MEDIA (viable si hay Docker)

---

### Opción 5: Usar Gemma 4 12B (ya en AION)

**Concepto:** Reutilizar Gemma 4 12B para traducción.

```rust
// En lugar de modelo dedicado, usa el LLM que ya corre:
let response_en = engine.generate(user_prompt).await?;

// Traducir con el MISMO Gemma:
let response_es = engine.generate(
    "Translate to Spanish:\n" + &response_en
).await?;
```

**Pros:**
- ✅ Zero dependencias (ya tienes Gemma)
- ✅ Offline
- ✅ Buena calidad (12B >> m2m100)

**Contras:**
- ❌ Latencia: ~4-8s adicionales (dos llamadas LLM)
- ⚠️  Overhead de tokens (traducción es N tokens extra)
- ❌ Recursos: compite con razonamiento del usuario

**Viabilidad:** BAJA (demasiado caro en latencia)

---

## Comparativa: Las 5 Opciones

| Opción | Coste | Latencia | Calidad | Setup | Viabilidad |
|--------|-------|----------|---------|-------|-----------|
| **1. Sin traducción** | $0 | 0ms | N/A (inglés) | ✅ Ya | ⭐⭐⭐⭐⭐ |
| **2. m2m100 Ollama** | $0 | 2-5s | Media | ⚠️ Manual | ⭐⭐ |
| **3. mBART/mT5 local** | $0 | 0.5-1s | Media-Alta | ⚠️ Manual | ⭐⭐⭐ |
| **4. LibreTranslate** | $0 | 1-3s | Alta | ⚠️ Docker | ⭐⭐⭐ |
| **5. Gemma 4 (reuse)** | 0 tokens extra | 4-8s | Muy alta | ✅ Ya | ⭐ |

---

## Recomendación de Refinamiento

### Fase 1B (antes de Fase 1): Validación de Opciones

```
┌─────────────────────────────────────────────────────┐
│ SPRINT REFINAMIENTO (2-3 días)                      │
├─────────────────────────────────────────────────────┤
│                                                     │
│ 1. Opción 1 (Sin traducción):                       │
│    ✓ Prototipo: respuesta en inglés técnico         │
│    ✓ User test: ¿entienden sin traducción?        │
│    ✓ Tiempo: 1 día (ya existe)                      │
│                                                     │
│ 2. Opción 3 (mBART local):                          │
│    ✓ Encontrar GGUF de mBART español                │
│    ✓ Integrar en Ollama                             │
│    ✓ Medir latencia real                            │
│    ✓ Tiempo: 1-2 días                               │
│                                                     │
│ 3. Opción 4 (LibreTranslate, si Docker):            │
│    ✓ Setup Docker local                             │
│    ✓ Benchmarks: latencia vs Opción 3               │
│    ✓ Decidir: ¿Vale la pena Docker?                │
│    ✓ Tiempo: 0.5 días                               │
│                                                     │
│ DECISIÓN: ¿Cuál es el mejor trade-off?             │
│           (cost vs latency vs quality)              │
│                                                     │
└─────────────────────────────────────────────────────┘
```

---

## Recomendación Técnica

### **Estrategia Hybrid: Opción 1 + Opción 3**

1. **Default:** Sin traducción (Opción 1)
   - Respuesta en inglés comprimido (responsabilidad del usuario)
   - Ultrarrápido (0ms overhead)
   - UX: "Respuesta en inglés (técnico)"

2. **Fallback (opcional):** mBART local si usuario lo requiere
   - Flag en config: `translation_mode: "none" | "mbart" | "google"`
   - Auto-descarga mBART solo si está activado
   - Latencia predecible (~500-1000ms)

3. **Future:** Google Translate si escalas a multi-user
   - Por ahora, local-first

---

## Criterios de Aceptación para Refinamiento

### LLMLingua Validation

- [ ] Prototipo Python real con documentación técnica española
- [ ] Compression ratio medido: ¿es realmente 5-12x? (target: ≥ 5x)
- [ ] Latencia de indexación: target < 500ms
- [ ] Coherencia post-compresión: ¿pierde info crítica?

### Traducción (Opción 1 vs 3)

- [ ] Opción 1: User test con 5 usuarios españoles
  - ¿Aceptable respuesta en inglés técnico?
  - ¿Qué % prefiere traducción?
  
- [ ] Opción 3 (si se elige):
  - [ ] GGUF de mBART encontrado + testeado
  - [ ] Latencia < 1000ms
  - [ ] Calidad de traducción técnica aceptable (manual review)

### API Design

- [ ] `MultilingualMemory` trait/struct dibujado
- [ ] Integración con `VectorMemory` clara
- [ ] Error handling definido
- [ ] Edge cases identificados (idioma no soportado, etc.)

---

## Roadmap: Refinamiento → Fase 1

```
AHORA:
  ├─ Opción 1 + 3 prototipo: 2-3 días
  └─ Validar LLMLingua real: 2 días
     │
     └─→ FASE 1B COMPLETE: Decision punto

DESPUÉS (Fase 1):
  ├─ MultilingualMemory (250 LOC)
  ├─ LLMLingua integración
  ├─ Opción elegida (1 o 3)
  └─ Tests + setup wizard
```

---

## Siguientes Pasos Concretos

**Día 1-2: Validar Opción 1 (Sin traducción)**
- [ ] Crear variante del script de validación que retorne respuesta en inglés
- [ ] Simular: pregunta en español → respuesta en inglés
- [ ] Medir: ¿qué % de pérdida de UX?

**Día 2-3: Validar Opción 3 (mBART local)**
- [ ] Encontrar/descargar modelo mBART GGUF
- [ ] Integrar en Ollama local
- [ ] Medir latencia real de traducción
- [ ] Calidad: comparar vs Google Translate (si tienes acceso)

**Día 4: Decision**
- [ ] Comparar métricas
- [ ] Elegir: Opción 1, Opción 3, o Hybrid
- [ ] Actualizar ADR-0004 con decisión
- [ ] Green light para Fase 1

---

## Documentos Relacionados

- [[adr-0004-multilingual-memory-optimization]]: ADR base
- [[validation-adr-0004-results]]: Validación actual
- [[scripts/validate_multilingual_adr.py]]: Script de prueba

