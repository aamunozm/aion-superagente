//! Generación de embeddings vía Ollama (`/api/embeddings`, modelo `nomic-embed-text`).

use aion_kernel::{AionError, Result};
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "nomic-embed-text";

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
        Self::new(url, DEFAULT_MODEL)
    }

    /// Devuelve el embedding de un texto.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let body = serde_json::json!({ "model": self.model, "prompt": text });
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

#[derive(Deserialize)]
struct EmbeddingResponse {
    embedding: Vec<f32>,
}
