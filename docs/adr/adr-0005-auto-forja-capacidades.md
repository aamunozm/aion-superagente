# ADR-0005: Auto-forja de capacidades reales (Facultad 4) — modelo por niveles y gobernanza

- **Estado:** propuesto (pendiente de aprobación de Ariel antes de implementar Tier 2+)
- **Fecha:** 2026-06-17
- **Decisores:** Ariel Marquez
- **Relacionado:** `crates/aion-skills` (sandbox WASM), `aion-evolution` (run_self_evolve),
  `react.rs` (Facultades 1–3), `reflection.rs` (gobernanza SSGM-lite, patrón a reusar),
  HITL existente (`confirm_action` / `ConfirmFn`).

## Contexto

Hoy `skill_forge` solo genera funciones **WASM puras `i64→i64`** (factorial, primos…). El
sandbox (`aion-skills/src/host.rs`) **rechaza cualquier módulo con imports** y lo instancia en
un linker vacío: cero acceso a red, archivos o syscalls **por diseño**. Es seguro pero
estrecho: AION no puede forjarse una capacidad real (parsear, transformar texto, consultar
una API).

La Facultad 4 que Ariel pidió es que AION **adquiera capacidades nuevas reales** cuando no sabe
hacer algo —y, en su forma fuerte, que se escriba herramientas que toquen el mundo (red,
archivos). Eso es la acción de **mayor impacto y mayor riesgo** posible en el sistema: un
agente que reescribe y ejecuta su propio código con I/O. La literatura (SSGM, arXiv:2603.11768;
ya citada en `reflection.rs`) documenta deriva procedural y capacidades internalizadas
peligrosas que, a diferencia del RAG estático, se vuelven **acumulativas y persistentes**.

Por eso esta decisión NO se implementa de golpe: se define un **modelo por niveles** donde la
capacidad crece y la superficie de riesgo se abre **solo** detrás de gobernanza explícita.

## Decisión

Modelo de capacidad de las skills auto-forjadas en **cuatro niveles (tiers)**, con un principio
rector: *el riesgo se abre por pasos, cada paso con su control, y lo irreversible nunca se
delega a código generado por el modelo.*

### Tier 0 — Cómputo puro `i64→i64` (HOY, vigente)
Lo actual. Sin imports, linker vacío, tests + ratchet. **Riesgo: nulo.** Se conserva.

### Tier 1 — Transformaciones puras de TEXTO/DATOS `string→string` (SIGUIENTE, seguro)
Permitir forjar funciones puras que transforman texto/datos: parsear un formato, extraer con
regex, formatear una tabla, normalizar, convertir unidades. **Sigue SIN imports de host**
(sin red, sin archivos) → **cero superficie de riesgo nueva**; es solo cómputo determinista
sobre cadenas.
- Coste técnico real: marshalling de strings por la **memoria lineal** del WASM (alloc en el
  módulo, el host escribe la entrada y lee la salida por offset/len). Es la parte no trivial.
- Gobernanza: la misma de Tier 0 (sandbox deny-all + tests + ratchet). El modelo genera el
  módulo; si no compila o falla los tests, se descarta (fail-closed).
- **Recomendación: empezar por aquí.** Gran salto de capacidad (AION ya puede forjarse
  herramientas de datos útiles) con riesgo nulo y sin decisiones de seguridad pendientes.

### Tier 2 — Capacidades con I/O de SOLO-LECTURA y acotado (GATED, futuro)
Exponer al WASM un conjunto **curado y mínimo** de funciones de host de solo-lectura:
- HTTP **GET** a una **allowlist de dominios** (no escritura, no métodos mutantes).
- Lectura de archivos **dentro de HOME** y bajo una allowlist de rutas.

Cada capacidad de Tier 2 requiere, **todas**:
1. **Modelo de capacidad explícito** — la skill declara qué host-fns usa; el host solo enlaza
   esas, nada más (capabilities, no ambient authority).
2. **HITL antes de la PRIMERA ejecución** — reusar `confirm_action`/`ConfirmFn`: Ariel aprueba
   una skill con I/O la primera vez que se va a ejecutar (igual que login/compra hoy).
3. **Límites de recursos** — `wasmtime` con *fuel* (CPU), tope de memoria y **timeout** por
   ejecución; sin esto un módulo puede colgar o consumir.
4. **Allowlist** de dominios/rutas, fail-closed (lo no listado se deniega).
5. **Auditoría completa** — cada ejecución con I/O se registra (qué skill, qué accedió).
6. **Cuarentena** — una skill nueva con I/O no se "consolida" hasta validarse, igual que la
   cuarentena de `reflection.rs` (confianza baja hasta reconfirmar).
7. **Aislamiento evolución↔ejecución** — forjar y ejecutar son fases separadas (patrón SSGM).

### Tier 3 — Escritura/efectos irreversibles (NO se delega a código auto-forjado)
Escribir/borrar archivos arbitrarios, ejecutar comandos de shell, peticiones de red mutantes,
pagos: **NO** viven en WASM generado por el modelo. Eso son **herramientas first-class** del
binario (auditadas, con HITL), no capacidades que el agente se escribe a sí mismo. La auto-forja
se topa aquí con el límite del "hogar digital propio": AION compone y crea, pero lo irreversible
pasa siempre por una herramienta revisada + tu OK.

## Gobernanza transversal
- **Fail-closed** en todo: sin capacidad declarada/aprobada → no se enlaza ni se ejecuta.
- **HITL** en todo Tier ≥ 2 antes de la primera ejecución.
- **Auditoría** de toda ejecución con I/O.
- **Reutilización** de mecanismos ya probados: `ConfirmFn` (HITL), patrón cuarentena/decaimiento
  de `reflection.rs`, ratchet de `aion-evolution`.

## Consecuencias
- **Positivas:** AION gana capacidad real de forma incremental; el riesgo se abre por pasos
  controlables; lo irreversible nunca queda en manos de código que el modelo se escribe solo.
- **Coste:** Tier 1 exige marshalling de memoria WASM; Tier 2 exige diseñar el set de host-fns,
  el capability model y el sandbox con límites — un ADR de implementación propio.
- **Riesgo residual:** Tier 2 amplía superficie aunque sea de solo-lectura (exfiltración vía
  GET a allowlist, lectura de archivos sensibles). Mitigado por allowlist + HITL + auditoría +
  que sale por `AION_PROXY`.

## Alternativas consideradas
- **Abrir el sandbox a I/O completo de una vez:** rechazada — superficie enorme, irreversible,
  contradice "validación antes de acciones sensibles / HITL en alto impacto".
- **Composición de herramientas existentes (macros/recetas) en vez de WASM con I/O:** atractiva
  y más segura (solo usa herramientas que ya tienen su gobernanza); a evaluar como vía paralela
  o sustituta de Tier 2 antes de exponer host-fns crudas.

## Plan recomendado
1. Implementar **Tier 1** (texto puro) ya — seguro, gran valor, sin decisiones pendientes.
2. Tras aprobar este ADR, redactar el **ADR de implementación de Tier 2** (set exacto de
   host-fns, capability model, diseño de sandbox con fuel/timeout, allowlists) y solo entonces
   tocar el sandbox.
3. Evaluar la alternativa de **macros/recetas** como camino más seguro que el I/O en WASM.
