//! Generación de embeddings vía Ollama (`/api/embeddings`).
//!
//! Modelo por defecto: **BGE-M3** (`bge-m3`). Sustituye a `nomic-embed-text`, que en
//! español/multilingüe rinde muy mal (Recall@1 ~0.15 vs ~0.94 de BGE-M3, benchmark
//! mar-2026). BGE-M3 es MIT, 1024 dim, contexto 8192, y corre en Ollama. Cambiable
//! con `AION_EMBED_MODEL` (p. ej. `qwen3-embedding:4b`).

use aion_kernel::{AionError, Result};
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "bge-m3";

/// Cliente de embeddings local.
pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl OllamaEmbedder {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            http: reqwest::Client::new(),
        }
    }

    pub fn default_local() -> Self {
        let url = std::env::var("AION_OLLAMA_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = std::env::var("AION_EMBED_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Self::new(url, model)
    }

    /// Nombre del modelo de embeddings en uso (para marcar la memoria y detectar
    /// cuándo hay que reindexar tras un cambio de modelo).
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Devuelve el embedding de un texto.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // keep_alive: mantén el modelo de embeddings CALIENTE (si no, Ollama lo descarga a
        // los 5 min y lo RECARGA en la siguiente recuperación → latencia). num_ctx FIJO a
        // 8192 (el nativo de BGE-M3): sin esto Ollama lo cargaba con 32768 (sobredimensionado
        // → rope extension, más RAM y carga más lenta; "context size too large for model").
        let body = serde_json::json!({
            "model": self.model,
            "prompt": text,
            "keep_alive": std::env::var("AION_KEEP_ALIVE").unwrap_or_else(|_| "24h".into()),
            "options": { "num_ctx": 8192 }
        });
        let resp = self
            .http
            .post(format!("{}/api/embeddings", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AionError::Memory(format!("petición de embedding falló: {e}")))?;
        if !resp.status().is_success() {
            return Err(AionError::Memory(format!(
                "Ollama embeddings devolvió {}",
                resp.status()
            )));
        }
        let parsed: EmbeddingResponse = resp
            .json()
            .await
            .map_err(|e| AionError::Memory(format!("respuesta de embedding inválida: {e}")))?;
        Ok(parsed.embedding)
    }
}

/// OllamaEmbedder es UNA implementación del trait [`aion_kernel::Embedder`]. Así la
/// memoria depende del trait, no de Ollama: un embedder MLX/Candle u otro puede sustituirlo
/// sin tocar `VectorMemory`. Delega en los métodos inherentes (que siguen disponibles para
/// los llamadores directos).
#[async_trait::async_trait]
impl aion_kernel::Embedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        OllamaEmbedder::embed(self, text).await
    }
    fn model(&self) -> &str {
        OllamaEmbedder::model(self)
    }
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    embedding: Vec<f32>,
}
