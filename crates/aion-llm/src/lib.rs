//! # aion-llm
//!
//! Capa de inferencia LLM de AION. Implementa el trait [`aion_kernel::LlmEngine`].
//!
//! - F1: [`OllamaEngine`] — reusa el modelo `gemma4-reason` (Gemma 4 12B abliterated)
//!   servido por Ollama en `:11434`. Soporta razonamiento (thinking) en streaming.
//! - F2: `MistralRsEngine` (embebido) — pendiente.
//! - F6: motores móviles (MLX/Candle) — pendiente.

mod ollama;
mod openai;

pub use ollama::OllamaEngine;
pub use openai::OpenAiEngine;
