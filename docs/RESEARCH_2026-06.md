# AION — Investigación SOTA (verificada) · 7-8 jun 2026

> Datos verificados directamente en fuentes primarias (GitHub + `gh`). El paso de
> verificación adversarial del workflow falló técnicamente, así que se validó a mano.

## 1. Frameworks de agentes en Rust

| Proyecto | ★ | Versión | Licencia | Veredicto |
|----------|---|---------|----------|-----------|
| **AutoAgents** (liquidos-ai) | 668 | v0.3.7 (mar 2026) | MIT/Apache-2.0 | ✅ **Referencia clave**: Rust nativo, backends locales (Ollama/mistral.rs/llama.cpp), **sandbox WASM para tools**, ReAct, pub/sub multi-agente. Joven pero alineadísimo con AION |
| Anda (ldclabs) | 432 | v0.12.0 | Apache/MIT | ❌ Memoria atada a blockchain ICP/dTEE → **no local-first**. Descartado |

**Decisión:** estudiar AutoAgents como referencia de arquitectura; construir orquestador propio (control total) inspirado en su patrón ReAct + pub/sub + WASM-tools.

## 2. Runtime LLM on-device (Rust)

| Proyecto | ★ | Versión | Veredicto |
|----------|---|---------|-----------|
| **Candle** (HuggingFace) | 20.4k | activo (jun 2026) | ✅ ML framework Rust más maduro. Metal/CUDA/CPU, GGUF, embebible (incl. móvil) |
| **mistral.rs** | 7.3k | v0.8.3 (1 jun 2026) | ✅ Motor de inferencia. Metal/CUDA/CPU, **soporta Gemma 4 (E4B, 26B)**, visión, GGUF/GPTQ/AWQ/UQFF, SDK Rust. Sin soporte móvil explícito |

**Decisión:** **mistral.rs** para desktop (macOS/Windows) — soporta Gemma 4 ya. **Candle** o llama.cpp/MLX para el camino móvil. Modelos pequeños móviles: Gemma 4 E2B/E4B, Phi, Qwen.

## 3. Browser agéntico

| Proyecto | ★ | Versión | Veredicto |
|----------|---|---------|-----------|
| **browser-use** | **97.7k** | v0.12.9 | 🏆 Líder absoluto del sector. Python. DOM + visión sobre Playwright/CDP |
| chromiumoxide (Rust CDP) | — | (verificar) | Candidato para driver CDP nativo en Rust |

**Tensión:** browser-use es Python y domina; nuestro core es Rust. **Opciones:** (a) driver CDP propio en Rust (chromiumoxide) para control total; (b) browser-use como *sidecar* Python probado. Recomendado: empezar con sidecar, migrar a Rust si hace falta.

## 4. Memoria de agentes

- **LanceDB** (10.5k★, Apache-2.0): vector DB **embebido**, multimodal, híbrido (vector+FTS+SQL), SDK Rust, local-first. ✅ **Elegido** para memoria largo plazo.
- **mem0**: episódica/semántica/procedural + grafo; LoCoMo 92.5. Buen patrón de referencia.
- ⚠️ **Hallazgo clave:** memoria **"darwiniana"**, consolidación tipo **sueño** y **fitness scoring/poda** NO existen como producto off-the-shelf → **es IP/diferenciación propia de AION**. La construimos nosotros.

## 5. Auto-mejora y sandbox

- **DGM (Darwin Gödel Machine)** (Apache-2.0, Python): se auto-modifica el código y valida cada cambio en SWE-bench/Polyglot. ⚠️ **Los propios autores advierten** que ejecuta "código no confiable generado por el modelo" que "puede comportarse de forma destructiva". → **Confirma que nuestro enfoque gated + sandbox es el correcto, no opcional.**
- **Extism** (5.6k★, v1.30.0): framework WASM para cargar módulos de forma **segura**. ✅ **Elegido** para ejecutar skills auto-generadas + parches.
- **Wasmtime**: runtime WASM de bajo nivel (alternativa/base).

## Stack final recomendado (verificado)

| Capa | Elección | Estado |
|------|----------|--------|
| Núcleo | Rust | — |
| LLM desktop | **mistral.rs** | ✅ soporta Gemma 4 |
| LLM móvil | Candle / llama.cpp / MLX + Gemma 4 E2B/E4B | ✅ |
| Orquestador | Propio (patrón AutoAgents) | build |
| Memoria largo plazo | **LanceDB** | ✅ |
| Memoria darwiniana/sueño | **Propia (IP)** | build |
| Skills sandbox | **Extism (WASM)** | ✅ |
| Auto-mejora | Bucle gated (lección DGM) | build |
| Browser | browser-use (sidecar) → chromiumoxide | ✅/verificar |
| UI | Flutter | — |
| Sync | Automerge (CRDT) | verificar |
| Backend control | Rust/Axum + Postgres + Stripe | — |
