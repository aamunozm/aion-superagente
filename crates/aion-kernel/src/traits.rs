//! Contratos (traits) del núcleo. Toda implementación concreta vive en otros crates.

use crate::errors::Result;
use crate::types::Message;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Petición de generación al LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub think: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Fragmento de streaming: puede ser razonamiento o respuesta final.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StreamChunk {
    /// Token del bloque de razonamiento `<think>`.
    Thinking { text: String },
    /// Token de la respuesta final visible.
    Answer { text: String },
    /// Fin de la generación, con métricas.
    Done { tokens: u32, tokens_per_sec: f32 },
}

/// Motor de inferencia LLM. Abstracción central que desacopla Ollama (F1) de
/// mistral.rs embebido (F2) y de los motores móviles MLX/Candle (F6).
#[async_trait]
pub trait LlmEngine: Send + Sync {
    /// Identificador del motor (p. ej. "ollama:gemma4-reason").
    fn id(&self) -> &str;

    /// Genera una respuesta completa (no streaming).
    async fn generate(&self, req: GenerateRequest) -> Result<Message>;

    /// Genera en streaming, invocando `on_chunk` por cada fragmento.
    async fn generate_stream(
        &self,
        req: GenerateRequest,
        on_chunk: Box<dyn FnMut(StreamChunk) + Send>,
    ) -> Result<()>;

    /// Comprueba que el motor está disponible (modelo cargado / servicio arriba).
    async fn health(&self) -> Result<()>;
}

/// Generador de embeddings. Abstracción que desacopla la memoria del *runtime* que los
/// produce: hoy Ollama+BGE-M3 (F1), mañana un embebido MLX/Candle u otro modelo, sin que
/// la memoria sepa cuál. El modelo de embeddings deja de ser una pieza fija.
///
/// IMPORTANTE: cambiar de embedder (o de modelo) cambia el espacio vectorial y/o la
/// dimensión → los embeddings ya guardados dejan de ser comparables. Por eso [`model`]
/// existe: la memoria marca con qué modelo indexó y reindexa si cambia.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Devuelve el vector de embedding de un texto.
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Nombre del modelo en uso (marca de procedencia para detectar reindexados).
    fn model(&self) -> &str;
}

/// Un recuerdo recuperado de la memoria, con su relevancia.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub id: String,
    pub content: String,
    pub score: f32,
}

/// Almacén de memoria (vectorial + estructurado). Implementado sobre LanceDB.
/// Base sobre la que se construye la memoria darwiniana (F4).
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Guarda un contenido y devuelve su id.
    async fn store(&self, content: &str) -> Result<String>;

    /// Recupera los `k` recuerdos más relevantes para una consulta.
    async fn retrieve(&self, query: &str, k: usize) -> Result<Vec<MemoryHit>>;
}

/// Resultado de ejecutar una skill en el sandbox WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutput {
    pub output: serde_json::Value,
}

/// Host de skills: ejecuta módulos WASM (Extism) bajo una política de capabilities
/// deny-all por defecto. Garantiza que el código auto-generado no dañe el sistema.
#[async_trait]
pub trait SkillHost: Send + Sync {
    /// Lista las skills disponibles.
    async fn list(&self) -> Result<Vec<String>>;

    /// Invoca una skill por nombre con una entrada JSON.
    async fn invoke(&self, name: &str, input: serde_json::Value) -> Result<SkillOutput>;
}
