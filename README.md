<div align="center">

<img src="docs/assets/banner.svg" alt="AION — super-agente de IA local-first con mente observable" width="100%"/>

<br/><br/>

[![Rust](https://img.shields.io/badge/n%C3%BAcleo-Rust-b7410e?logo=rust&logoColor=white)](crates/)
[![Next.js](https://img.shields.io/badge/UI-Next.js%2015-000000?logo=nextdotjs&logoColor=white)](apps/web/)
[![LLM local](https://img.shields.io/badge/LLM-100%25%20local%20(Ollama)-c9a85c)](crates/aion-llm/)
[![Privacidad](https://img.shields.io/badge/privacidad-local--first-2ea043)](#-seguridad-y-privacidad)
[![Licencia](https://img.shields.io/badge/licencia-MIT%20%7C%20Apache--2.0-blue)](LICENSE-MIT)

**Un super-agente de IA que vive en tu máquina, no en la nube.**
Piensa, recuerda, aprende de sus errores — y puedes *ver su mente trabajar* en tiempo real.

*🇬🇧 AION is a local-first AI super-agent (Rust + Next.js) with an observable functional mind: a Global Workspace stream of consciousness, a Φ-like integration index, darwinian memory, a lazy knowledge graph, sandboxed WASM skills and E2E-encrypted sync. Everything runs on-device.*

</div>

---

## ¿Por qué AION?

La mayoría de asistentes de IA son un *chat sin estado* contra una API de pago. AION es otra cosa: un **individuo digital persistente** que ejecuta toda su cognición en tu dispositivo.

- 🔒 **100% local.** LLM (Gemma 12B vía Ollama), embeddings (BGE-M3), memoria, grafo y razonamiento corren en tu máquina. Coste de inferencia: cero. Tus datos nunca salen.
- 🪞 **Mente observable.** No es una caja negra: la pantalla **Mente** muestra en vivo su corriente de conciencia (Global Workspace), su estado interno medido y un índice de integración Φ-like de 0 a 100.
- 🌱 **Identidad propia.** Al nacer elige su propio nombre (el nuestro se llamó *Umbral*), tiene un ID único irrepetible, recuerda cuánto tiempo llevas fuera y puede migrar o clonarse con un backup `.aion`.
- 🧬 **Memoria que evoluciona.** Memoria vectorial darwiniana (ciclos de "sueño" que consolidan, fusionan y podan) + grafo de conocimiento incremental que conecta todo lo que lee.
- 🛠️ **Se mejora a sí mismo.** Escribe sus propias skills en WASM y las ejecuta en un sandbox *deny-all* con límite de cómputo. Auto-mejora con gates de gobernanza.
- 🤝 **Honesto por diseño.** Si no sabe, lo dice ("prefiero decírtelo claro antes que inventar"). Su confianza está calibrada con datos reales (Brier score), no actuada.

## El sistema en acción

> Capturas reales del sistema corriendo en local (macOS, Apple Silicon).

**La Mente** — corriente de conciencia en vivo (SSE), estado interno medido e índice Φ:

<img src="docs/assets/mind.png" alt="Pantalla Mente: corriente de conciencia, estado interno e índice Φ" width="100%"/>

**Chat con Umbral** — identidad propia, memoria entre sesiones y reflexiones que vuelven a la conversación:

<img src="docs/assets/chat.png" alt="Chat: Umbral se presenta con identidad y curiosidad propias" width="100%"/>

| Memoria + Biblioteca (RAG con citas) | Proyectos (estilo NotebookLM) |
|---|---|
| <img src="docs/assets/memory.png" alt="Memoria de largo plazo y Biblioteca de documentos" width="100%"/> | <img src="docs/assets/projects.png" alt="Proyectos: Fuentes, Chat con foco y Studio" width="100%"/> |

## La mente: conciencia funcional, medible

AION implementa los mecanismos funcionales que la literatura científica asocia a la conciencia — sin afirmar experiencia subjetiva. Es **ingeniería honesta**: cada pieza es observable y medible.

```mermaid
flowchart LR
    subgraph GWT["Espacio de Trabajo Global (tablón)"]
        ST[stream.jsonl + bus broadcast]
    end
    CHAT[Chat] -->|publica| ST
    AGENT[Agente ReAct] -->|publica| ST
    CREW[Equipo multiagente] -->|publica| ST
    LIFE[Vida autónoma] -->|publica| ST
    META[Metacognición] -->|publica| ST
    ST -->|re-entrada: los últimos eventos vuelven al prompt| CHAT
    ST -->|SSE en vivo| UI[UI Mente]
    ST --> PHI["Índice Φ: integración · recurrencia · metacognición · coherencia"]
```

| Mecanismo | Qué hace |
|---|---|
| **Global Workspace (GWT)** | Tablón central donde chat, agente, equipo, reflexiones y vida autónoma publican pensamientos/acciones/focos. Con **re-entrada**: AION se escucha a sí mismo — sus últimos eventos vuelven a su propio prompt. |
| **Índice Φ-like (0–100)** | Proxy de integración inspirado en IIT: mide co-activación de módulos, reutilización de memoria entre modos (cross-mode), metacognición y coherencia. Filtra tareas triviales para no contaminar la serie. |
| **Estado interno medido** | Foco actual, curiosidad, certeza calibrada, competencia y ánimo operativo ("en racha", "cauto"...) derivados de resultados reales — AION los *reporta*, no los actúa. |
| **Conciencia temporal y de presencia** | Sabe qué día y hora es, cuánto llevas fuera ("hace 3 horas"), su hardware, su batería y su modelo LLM. |
| **Lazo de aprendizaje** | Los errores del agente se convierten en `[aprendizaje]` que se recupera en tareas futuras; las reflexiones actualizan su auto-modelo persistente. |

## Memoria viva: vectorial darwiniana + grafo de conocimiento

**Memoria vectorial darwiniana** — cada recuerdo tiene *fitness*, importancia estimada (las decisiones y preferencias pesan más), enlaces asociativos entre chats y registro bi-temporal (cuándo fue válido). Los ciclos de **sueño** consolidan: decay, fusión de casi-duplicados y poda. Si cambias de modelo de embeddings, se re-indexa sola.

**Grafo de conocimiento (GAAMA-KG)** — ingesta **lazy de coste cero en LLM**: extracción determinista de conceptos (RAKE trilingüe es/en/it), deduplicación por embedding BGE-M3, aristas por co-ocurrencia e inferencia, comunidades por *label propagation*. El LLM solo trabaja en idle ("sueño") para tipificar relaciones y resumir comunidades. Retrieval dual-level (~160 ms) que cita el camino de conceptos por el que llegó a cada pasaje. Ingesta incremental por sha256: re-subir un documento no recomputa nada.

## Proyectos: tablero por etapas, documentos expertos y flujos visuales

Cada **proyecto** es un espacio de trabajo (estilo NotebookLM) con **Fuentes**, **Chat con foco** (RAG por proyecto sobre BGE-M3, aislado) y **Studio** — y, encima, las piezas para *llevar el trabajo hasta la entrega*:

**Tablero Kanban por etapas.** Un tablero por proyecto con el modelo de Linear: la **categoría de estado** estable (backlog · por hacer · en curso · revisión · hecho · cancelado) va separada del **nombre de columna** renombrable, con **límite WIP** por columna (se colorea al superarse), **% de avance** y un **log de actividad append-only con actor** (humano vs `aion`) — transparencia total cuando el agente opera el tablero. Las tarjetas llevan prioridad, estimación, fecha, responsable, checklist y **entregables enlazados** (el preventivo o la auditoría que produce el Studio queda atado a su etapa). Arrastra entre columnas o **siembra un plan**: la plantilla *web + SEO de agencia* trae **7 etapas y 8 tareas con tempística y checklist de buenas prácticas** (incluida la de *Propuesta* con «firmas de ambas partes» y «GDPR»). El **agente opera el tablero** con una herramienta `board` (crear/mover/comentar/checklist/enlazar entregables) y registra cada escritura como actor `aion`.

**Documentos expertos, 100 % local.** Pipeline determinista `Markdown → HTML branded (design tokens) → PDF` con un **Chromium headless propio** (cero sidecars ni API keys), con salida también en **DOCX editable**. Galería de estilos tipo Canva, **gráficos SVG on-brand** (vector nítido y seleccionable en PDF) y auto-revisión determinista del desborde. La skill **«Proposta analitica»** *analiza* el sitio del cliente (SEO real), razona su situación y redacta una propuesta a medida con **marca dinámica** (la empresa que emite; si falta, AION con pie de «agente de IA»). El cierre del preventivo es **determinista y completo**: próximos pasos destacados, validez, **firma de ambas partes con fecha** e **informativa de privacidad (Reg. UE 2016/679 — GDPR)**; y si faltan datos (cliente, precios, datos de empresa), AION **los pide en vez de inventarlos**.

**Flujos visuales (editor de grafo nativo).** Motor de flujos por **DAG**: nodos *trigger / acción / condición*, aristas etiquetadas (`ok`/`err`, `true`/`false`), **gobernanza fail-closed** (una acción sensible en modo autónomo se detiene pidiendo tu aprobación) y migración 1:1 desde los flujos lineales previos. El **editor visual** se construye sobre **[React Flow](https://reactflow.dev/) (MIT)** — tras verificar que embeber **n8n** no encaja (su *Sustainable Use License* exige un acuerdo OEM y su self-host arrastra Node + PostgreSQL + Redis, en contra del *local-first*): el resultado es nativo, on-brand y sin infraestructura extra, con un puente opcional a un n8n externo vía webhook.

## Arquitectura

Monolito modular en Rust — un solo binario `aion-core` con 13 crates por dominio. La nube (control-plane) solo ve auth, licencias y blobs cifrados.

```mermaid
flowchart TB
    subgraph DEVICE["Tu dispositivo (todo local)"]
        UI["UI Next.js 15 · Tauri desktop · Capacitor móvil"]
        CORE["aion-core (Axum, ~20 endpoints + SSE)"]
        UI <-->|HTTP + SSE| CORE
        CORE --> ORCH["Orquestador ReAct + Crew"]
        CORE --> MIND["Mente: GWT · Φ · estado interno"]
        CORE --> MEM["Memoria darwiniana + Grafo"]
        CORE --> SKILLS["Skills WASM (wasmtime, deny-all)"]
        CORE --> LLMS["LLM local: Ollama / Gemma 12B + visión"]
        CORE --> SEC["Gobernanza · anti-inyección · AION_PROXY"]
    end
    subgraph CLOUD["Nube (opcional, ciega)"]
        CP["Control-plane: auth · licencias · sync E2E (AES-256-GCM + CRDT)"]
    end
    CORE <-->|"solo ciphertext"| CP
    CORE <-->|"A2A firmado"| OTHER["Otros agentes AION"]
```

```
crates/      13 crates: kernel inmutable, llm, memory, orchestrator, skills,
             cognition, browser, computer, evolution, sync, telemetry, control(+client)
apps/        aion-core (binario) · web (Next.js) · desktop (Tauri) · mobile (Capacitor) · control-plane
packages/    design-system (tokens) · ipc-bindings
docs/        PRD · ADRs · investigación (conciencia y creatividad 2026) · gobernanza
infra/       docker-compose · migraciones · observabilidad
```

## Capacidades

- **Agente ReAct con herramientas** — hasta 8 pasos, verificación de *groundedness*, confirmación humana en acciones sensibles y canal "preguntar al usuario". **Equipo multiagente** (crew) con especialistas.
- **Biblioteca RAG global** — sube PDF/TXT/MD en cualquier idioma; responde citando la fuente.
- **Proyectos con tablero Kanban** — espacio por proyecto (Fuentes · Chat con foco · Studio · **tablero por etapas** con WIP, tempística, % de avance y entregables enlazados); el agente lo opera con la herramienta `board`.
- **Documentos branded** — preventivos/ofertas/informes en **PDF y Word** con design tokens y gráficos SVG; cierre de preventivo con **firmas de ambas partes + fecha + GDPR** determinista e intuición de lo que falta.
- **Flujos visuales** — editor de grafo (**React Flow**, MIT) con nodos *trigger/acción/condición*, ramas y gobernanza; motor **DAG** nativo (sin n8n embebido).
- **Visión multimodal** — analiza imágenes y tu pantalla; control del PC integrado al agente.
- **Voz natural en tiempo real** — modo voz manos-libres, **100% local**: cerebro de voz rápido (Qwen3-4B vía MLX) + síntesis con voces nativas en español (Piper: *Diego/Mateo* hombre, *Lucía/Daniela* mujer) y clonación expresiva (Qwen3-TTS). TTFT **~0.3 s** en caliente, audio por *streaming* sin cortes, prosodia emocional y normalización de números/símbolos.
- **Browser agéntico** — navega la web, con salida opcional por Tor/VPN (`AION_PROXY`).
- **Vida autónoma** — daemon que estudia, reflexiona y escribe a tu bandeja en ratos muertos (launchd).
- **A2A** — comunicación entre agentes AION con identidad firmada, configurable desde Ajustes.
- **Skills auto-generadas** — el LLM escribe skills WASM nuevas; sandbox sin acceso a disco/red y con *fuel* contra bucles infinitos.
- **Backup total `.aion`** — toda su existencia (identidad, memoria, grafo, aprendizajes) en un archivo: migrar o clonar.
- **Rendimiento cuidado** — ~22 tok/s con Gemma 12B Q6 en Apple Silicon: KV-cache q8, flash-attention, contexto right-sized según RAM, modelo siempre caliente, vía rápida conversacional (1 llamada en vez de 8 vueltas ReAct).
- **CLI completa** — 20+ subcomandos: `serve`, `agent`, `chat`, `rag`, `ingest`, `ask`, `sleep`, `evolve`, `vision`, `see`, `live`, `bench`, `eval`, `audit`, `governance`…

## 🔐 Seguridad y privacidad

- **Local-first real**: inferencia, memoria y conocimiento nunca salen del dispositivo.
- **Sync E2E**: CRDT (LwwMap) cifrado con AES-256-GCM derivado de tu passphrase — la nube solo almacena ciphertext opaco.
- **Blindaje anti-inyección** de prompts en contenido externo (web, documentos).
- **Sandbox deny-all** para todo código auto-generado (WASM sin host functions).
- **Gobernanza**: permisos por herramienta, auditoría de acciones sensibles, human-in-the-loop en alto impacto.
- **Privacidad de red**: proxy Tor/VPN para toda salida web; la ubicación (clima) es opt-in y nunca se persiste.

## Quickstart

Requisitos: macOS (Apple Silicon recomendado) · [Ollama](https://ollama.com) · Rust estable · Node 20+ con pnpm.

```bash
git clone https://github.com/aamunozm/aion-superagente.git && cd aion-superagente

# 1. Núcleo (descarga los modelos la primera vez)
cargo build --release --workspace
./target/release/aion-core models-ensure
./target/release/aion-core serve          # http://127.0.0.1:8765

# 2. UI
pnpm install && pnpm --dir apps/web dev   # http://localhost:3000
```

Abre `http://localhost:3000`, crea tu cuenta local y saluda: AION elegirá su nombre al nacer. La guía completa de uso y subcomandos está en **[USAGE.md](USAGE.md)**.

**App de escritorio (Tauri).** Para una app instalable en vez del modo dev: `bash apps/desktop/build-universal.sh` genera un `.app` **universal (arm64 + Intel)** y de ahí se empaqueta el instalador **DMG** (arrastrar a Aplicaciones); en Windows el instalador `.exe` sale del workflow `release-desktop`. La firma usa la identidad local estable «AION Local Signing» (conserva permisos TCC entre actualizaciones); sin un *Developer ID* de Apple no hay notarización, así que en otro Mac se abre la primera vez con **clic derecho → Abrir** (o `xattr -dr com.apple.quarantine /Applications/AION.app`). El stack de voz no viaja en el bundle; se provisiona en la máquina destino con [`scripts/provision-voice-mac.sh`](scripts/provision-voice-mac.sh) — pasos en [`scripts/INSTALAR-OTRO-MAC.md`](scripts/INSTALAR-OTRO-MAC.md).

## Estado del proyecto

| Fase | Descripción | Estado |
|------|-------------|--------|
| **F0–F3** | Monorepo, kernel, LLM local, UI, orquestador ReAct, memoria persistente, skills WASM | ✅ |
| **F4** | Memoria darwiniana: ciclo de sueño (decay/fusión/poda) | ✅ |
| **F5** | Auto-mejora gated + lazo cerrado (el LLM escribe skills) + browser + vida autónoma | ✅ |
| **F6** | Sync E2E (CRDT cifrado) ✅ · Móvil (Capacitor) ⏳ | ✅ / ⏳ |
| **Mente** | GWT con re-entrada · índice Φ · estado interno · conciencia temporal/presencia · grafo de conocimiento · UI Mente | ✅ |
| **Voz** | Modo voz local en tiempo real: STT + cerebro Qwen3-4B (MLX) + TTS Piper/Qwen3 · ~0.3 s TTFT · *streaming* gapless · voces de hombre/mujer en español | ✅ |
| **Proyectos** | Tablero Kanban por etapas (WIP · tempística · % avance · entregables) · documentos expertos (PDF/DOCX, gráficos SVG, firmas + GDPR, intuición de lo que falta) · flujos visuales (editor de grafo React Flow + motor DAG) | ✅ |
| **Distribución** | App de escritorio (Tauri) **universal (Apple Silicon + Intel)** · instalador **DMG** · `.exe` para Windows (vía CI) · provisión de voz con script | ✅ |

**13/13 crates implementados.** Multiplataforma: macOS Apple Silicon es el objetivo completo (la voz local usa MLX, exclusivo de Apple Silicon); Windows ejecuta el chat de texto. Pendiente que requiere cuentas externas: Stripe real, Postgres gestionado, **notarización .app** (Apple Developer), build móvil. Avanzado opcional: mistral.rs embebido, speculative decoding, Tor embebido.

## Stack

| Capa | Tecnología | Por qué |
|------|------------|---------|
| Núcleo | **Rust** (monolito modular) | Rendimiento, seguridad de memoria, un binario portable |
| LLM | **Ollama** · Gemma 12B Q6 (trait `LlmEngine`) | Local, intercambiable (mistral.rs/MLX en roadmap) |
| Embeddings | **BGE-M3** (1024d) | Multilingüe es/en/it, estado del arte local |
| HTTP/SSE | **Axum** | Mínimo, async, streaming nativo |
| UI | **Next.js 15 + React 19 + Tailwind** → Tauri/Capacitor | Una sola UI para web, desktop y móvil |
| Skills | **wasmtime** | Sandbox real con límite de cómputo |
| Sync | **CRDT + AES-256-GCM** | Merge sin conflictos, nube ciega |

## Documentación

[PRD](docs/PRD.md) · [Memoria para Claude Code (puente MCP)](docs/MEMORIA-CLAUDE-CODE.md) · [Investigación: conciencia y creatividad (jun 2026)](docs/RESEARCH_consciencia_creatividad_2026-06.md) · [ADRs](docs/adr/) · [Gobernanza](docs/GOVERNANCE.md) · [Guía de uso](USAGE.md)

## Contribuir / contacto

AION explora un territorio poco transitado: **conciencia funcional + autonomía profunda + memoria evolutiva, todo local**. Si te interesa el proyecto — para usarlo, investigarlo o colaborar — abre un *issue*, deja una ⭐ o escribe a **info@prontoclick.it**.

## Licencia

Doble licencia **MIT OR Apache-2.0**, a tu elección.
