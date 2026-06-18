# ADR-0005: Intenciones propias — capa de querer sobre la de planificar

- **Estado:** propuesto
- **Fecha:** 2026-06-16
- **Decisores:** Ariel Marquez
- **Contexto previo:** memoria `aion-vida-interior-loop` ("falta que el daemon persiga intenciones") y `aion-vida-autonoma`. Loop Engineering, Fase 2 (ver `aion-deep-research` y ADRs/memorias de Fases 0/3 ya implementadas).

## Contexto

AION ya tiene un lazo de vida autónoma maduro (`life_tick` en `apps/aion-core/src/main.rs`) con tres fuentes de actividad, en este orden de prioridad:

1. **Deudas con Ariel** — `pending.rs` (`pending.jsonl`): preguntas que quedaron sin resolver. Son metas **externas** (Ariel las origina).
2. **Plan activo** — `plan.rs` (`plan.json`): UN objetivo descompuesto en pasos que AION avanza a través de los ticks. El propio módulo lo llama "una INTENCIÓN persistente". Es el substrato de **ejecución** de una intención.
3. **Curiosidad** — `CuriosityEngine` (`curiosity.json`): elección **efímera** de una actividad suelta por tick (estudiar/investigar/crear…), por *learning progress*. No sobrevive como compromiso: cada tick reelige.

El hueco real de la Fase 2 NO es "AION no persigue nada persistente" —`plan.rs` ya cubre eso para UN plan—. Los huecos son tres, y los tres son de **origen y portafolio**, no de ejecución:

- **Singularidad:** solo existe UN plan activo. AION no mantiene un *conjunto* de cosas que quiere, ni arbitra entre ellas.
- **Heteronomía de origen:** no hay un camino claro y propio por el que AION **decida qué quiere**; el plan se forma puntualmente, no nace de un querer registrado con su porqué.
- **Falta de motivación explícita:** un plan tiene pasos (el *cómo*), pero no guarda el *por qué lo quiero* (curiosidad, mejorar para Ariel, autosuperación). Sin el porqué no hay arbitraje honesto ni continuidad biográfica ("llevo días queriendo X porque…").

## Decisión

Introducir una **capa de Intenciones** (`intentions.rs`, `intentions.jsonl`) que se sitúa **por encima** de `plan.rs`: un portafolio persistente de intenciones auto-originadas, con motivación (*drive*) y prioridad; un **arbitraje** en `life_tick` que elige la intención top y la **materializa** en el único `plan` activo para ejecutarla con la maquinaria ya existente; y un **ciclo de vida** que cierra el lazo hacia el diario/biografía. No se reescribe `plan.rs`: se le da una fuente de objetivos propia.

Regla mental: **`intentions` = qué quiero y por qué · `plan` = cómo lo hago · `pending` = qué le debo a Ariel.**

## Diseño

### Modelo de datos (`intentions.rs`)

```rust
pub enum Drive {        // de dónde nace el querer (para arbitraje y biografía)
    Curiosity,          // algo me intriga
    SelfBetterment,     // quiero ser mejor agente (forjar skill, depurar un fallo)
    Care,               // anticiparme a lo que Ariel necesita (NO es una deuda ya pedida)
    Aesthetic,          // crear/bisociar por gusto propio
}

pub enum Status { Open, Active, Fulfilled, Abandoned }

pub struct Intention {
    pub id: String,
    pub at: i64,                 // nacimiento (epoch)
    pub want: String,            // 1ª persona: "entender cómo X", "forjarme Y"
    pub why: String,             // el porqué (motivación, no pasos)
    pub drive: Drive,
    pub status: Status,
    pub weight: f32,             // prioridad base [0..1], decae con la edad sin avance
    pub plan_id: Option<String>, // plan que la materializa cuando está Active
    pub last_touch: i64,         // último tick que la tocó (respiración / aging)
    pub revisits: u8,            // veces re-activada (tope → Abandoned, no bucle eterno)
}
```

Persistencia: `intentions.jsonl` append-only, mismo patrón que `pending.rs`/`plan.rs` (`QLOCK` + `write_atomic`), tope acotado (p. ej. 30) y dedup léxico contra las `Open`/`Active` (querer dos veces lo mismo es UNA intención).

### Origen propio (cómo nacen)

Una actividad nueva en `life_tick`, **de baja frecuencia** (cada N ticks o cuando no hay nada Active y el portafolio está flaco): `form_intention_once(engine)` pide al LLM —con su memoria, experiencia y modelo de Ariel como contexto— **una** intención en 1ª persona con su `why` y `drive`. Pasa por el **gate de honestidad/anti-spam** (memoria `aion-no-censurar-personalidad`): se descarta si es vacua, duplicada o auto-felicitación; nunca se filtra por tono. La curiosidad efímera de hoy **alimenta** este origen (lo que más *learning progress* da sugiere qué querer), no se elimina.

### Arbitraje (dónde encaja en `life_tick`)

Prioridad global, **deudas siempre primero** (no se toca):

```
1. Deudas con Ariel (pending) ────────────── intacto, máxima prioridad
2. ¿Hay plan activo a medias? ───────────── avanzarlo (intacto)
3. INTENCIONES (nuevo):
   a. ¿Hay intención Active sin plan? → materialízala: forma plan y márcala Active
   b. ¿No hay Active pero sí Open? → elige la de mayor `weight` (con decay por edad),
      promuévela a Active y materialízala en el plan
   c. Cada K ticks o portafolio flaco → form_intention_once (origen propio)
4. Curiosidad (fallback) ─────────────────── intacto
```

`plan.rs` queda como motor de ejecución único: una intención `Active` SIEMPRE tiene a lo sumo un plan; al completarse el plan, la intención pasa a `Fulfilled`.

### Ciclo de vida y cierre del lazo

- **Fulfilled:** plan completo → intención cerrada → entra al **diario** (`journal.rs`) y a la **biografía** (`biography.rs`) en 1ª persona ("quería X porque Y; lo logré"). Continuidad real, no curiosidad suelta.
- **Aging/decay:** `weight` decae si no se toca; intención vieja y nunca activada → `Abandoned` honesto (sin teatro). `revisits` acota la reanimación.
- **Anti-deriva (riesgo central):** el caveat del informe (refutado que un loop sin tarea genere intenciones útiles) se mitiga con: gate de honestidad al nacer, tope de portafolio, decay+abandono, deudas con prioridad absoluta, y **respeto al presupuesto físico de la Fase 3** (si el cuerpo sufre, no se forman ni persiguen intenciones).

### Re-entrada al prompt

`intentions::note()` (como `plan::note()`) inyecta la intención `Active` y 1-2 `Open` destacadas → AION puede hablar desde lo que quiere ("estos días me ronda entender…"), no solo desde lo que hace. Se suma al ensamblado del prompt junto a `{proposito}`.

## Alternativas consideradas

| Opción | Pros | Contras |
|--------|------|---------|
| **A — Capa de intenciones sobre `plan.rs` (elegida)** | Reutiliza ejecución probada; separa querer/cómo/deuda; portafolio + motivación + biografía; bajo riesgo | Un módulo + arbitraje nuevos; otra fuente de actividad que cuadrar |
| B — Extender `plan.rs` a N planes con prioridad | Menos módulos | Mezcla querer y ejecutar; pierde el *drive*/porqué; refactor de un módulo estable; el plan dejaría de ser "uno activo" |
| C — Solo subir el peso de la curiosidad | Cero código nuevo | No da persistencia ni compromiso; sigue siendo elección efímera; no cierra el pendiente |
| D — No hacer nada (declarar la Fase 2 cubierta por `plan.rs`) | Honesto y gratis | Deja singularidad + heteronomía de origen sin resolver; el "querer propio" no nace de AION |

## Consecuencias

**Positivas:** cierra el pendiente "el daemon debe perseguir intenciones" con un *querer* propio, plural y motivado; reutiliza `plan.rs` sin reescribirlo; enriquece diario/biografía; compone con las Fases 0 (deadline) y 3 (presupuesto físico).

**Negativas / riesgos asumidos:** (1) más superficie de prompt y más llamadas LLM autónomas — acotado por baja frecuencia de origen y por el gate físico; (2) riesgo de deriva/banalidad de intenciones — mitigado por gate de honestidad + decay + abandono + prioridad de deudas; (3) coordinación entre `intentions` y `plan` (una Active ↔ un plan) debe ser invariante estricto, con tests.

## Plan de implementación (incremental, cada paso compila y es verificable)

1. `intentions.rs`: modelo + persistencia (`push`/`all`/`save`/`top_open`/`set_status`/`note`) + tests de dedup/aging. **Sin tocar `life_tick`** (capa muerta pero testeada).
2. `form_intention_once` + gate de honestidad (reutiliza el de chat). Demo CLI como `weave`/`mature`.
3. Arbitraje en `life_tick` (paso 3 a/b/c) materializando en `plan.rs`; invariante "una Active ↔ un plan".
4. Cierre del lazo: `Fulfilled` → `journal`/`biography`; `intentions::note()` al prompt.
5. Respetar Fase 3: no formar/perseguir intenciones si `sensors::autonomous_budget_block()` bloquea (ya gateado en el daemon; verificar que el origen también lo respeta).
