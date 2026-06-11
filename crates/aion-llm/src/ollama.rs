//! Implementación de [`LlmEngine`] sobre Ollama (`/api/chat`).
//!
//! Reusa el conocimiento del prototipo `legacy/gemma4-reasoning`: modelo
//! `gemma4-reason`, sampling temp 1.0 / top_p 0.95 / top_k 64, y separación de
//! razonamiento (`thinking`) y respuesta final.

use aion_kernel::traits::{GenerateRequest, LlmEngine, StreamChunk};
use aion_kernel::types::{Message, Role};
use aion_kernel::{AionError, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "gemma4-reason";

/// Ventana de contexto estable (evita recargas y ahorra RAM). Configurable.
fn num_ctx() -> u32 {
    std::env::var("AION_NUM_CTX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8192)
}

/// Cuánto mantener el modelo caliente en memoria tras cada uso. Configurable.
fn keep_alive() -> String {
    // El modelo se queda CALIENTE toda la sesión (24h) → sin recargas de ~10s tras estar
    // inactivo. Latencia mínima con CUALQUIER modelo. (AION_KEEP_ALIVE lo ajusta; en equipos
    // con poca RAM puede ponerse "10m" para liberar memoria al estar ocioso.)
    std::env::var("AION_KEEP_ALIVE").unwrap_or_else(|_| "24h".into())
}

/// Motor LLM que habla con un servidor Ollama local.
pub struct OllamaEngine {
    base_url: String,
    model: String,
    id: String,
    http: reqwest::Client,
}

impl OllamaEngine {
    /// Crea un motor con base_url y modelo explícitos.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let base_url = base_url.into();
        let model = model.into();
        let id = format!("ollama:{model}");
        // FIABILIDAD: timeouts. Sin esto, si el runner local se cuelga (p. ej. por
        // contención de GPU con otro ollama), la petición se queda colgada PARA SIEMPRE
        // y bloquea la cola. `read_timeout` corta si NO llegan bytes en 120s (las
        // generaciones normales fluyen token a token, así que no las afecta).
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Self {
            base_url,
            model,
            id,
            http,
        }
    }

    /// URL del runtime Ollama: `AION_OLLAMA_URL` si está definida (p. ej. el
    /// Ollama embebido en un puerto privado), si no el valor por defecto.
    pub fn base_url_from_env() -> String {
        std::env::var("AION_OLLAMA_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
    }

    /// Motor con los valores por defecto de AION (URL configurable + gemma4-reason).
    pub fn default_local() -> Self {
        Self::new(Self::base_url_from_env(), DEFAULT_MODEL)
    }

    /// Precarga el modelo en memoria (warmup) para que el PRIMER mensaje no pague la
    /// carga. Una petición vacía con keep_alive deja el modelo listo y caliente.
    pub async fn warmup(&self) {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": "ok" }],
            "stream": false,
            "think": false,
            "keep_alive": keep_alive(),
            "options": { "num_ctx": num_ctx(), "num_predict": 1 },
        });
        let _ = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await;
    }

    fn build_body(&self, req: &GenerateRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<OllamaMessage> = req.messages.iter().map(OllamaMessage::from).collect();
        let mut options = serde_json::Map::new();
        if let Some(t) = req.temperature {
            options.insert("temperature".into(), t.into());
        }
        if let Some(m) = req.max_tokens {
            options.insert("num_predict".into(), m.into());
        }
        // num_ctx ESTABLE: una ventana fija evita que Ollama recargue el modelo (cambiar
        // num_ctx fuerza recarga). 8192 sobra para chat (la conversación se comprime) y
        // carga más rápido y usa menos RAM que 32768, sin pérdida de calidad real.
        options.insert("num_ctx".into(), num_ctx().into());
        serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": stream,
            "think": req.think,
            // keep_alive: mantiene el modelo CALIENTE en memoria → los siguientes
            // mensajes no pagan la recarga (2–9 s).
            "keep_alive": keep_alive(),
            "options": options,
        })
    }

    /// ¿Está el modelo listo (ya existe en Ollama)? En el primer arranque de una
    /// máquina nueva, el modelo se descarga (~9 GB); hasta entonces no está listo.
    pub async fn model_ready(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        let Ok(resp) = self.http.get(&url).send().await else {
            return false;
        };
        let Ok(text) = resp.text().await else {
            return false;
        };
        // El nombre puede venir como "gemma4-reason" o "gemma4-reason:latest".
        text.contains(&format!("\"{}\"", self.model))
            || text.contains(&format!("{}:latest", self.model))
            || text.contains(&format!("\"{}:", self.model))
    }

    /// Genera una respuesta a partir de un prompt + una imagen (base64) — visión
    /// multimodal. Requiere un modelo con visión (p. ej. gemma 4 abliterated).
    pub async fn generate_with_image(&self, prompt: &str, image_b64: &str) -> Result<Message> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": prompt, "images": [image_b64] }],
            "stream": false,
            "think": false,
            "keep_alive": keep_alive(),
            "options": { "num_ctx": num_ctx() },
        });
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AionError::Llm(format!("petición a Ollama falló: {e}")))?;
        if !resp.status().is_success() {
            return Err(AionError::Llm(format!("Ollama devolvió {}", resp.status())));
        }
        let parsed: OllamaChatResponse = resp
            .json()
            .await
            .map_err(|e| AionError::Llm(format!("respuesta inválida de Ollama: {e}")))?;
        Ok(Message {
            role: Role::Assistant,
            content: parsed.message.content,
            thinking: None,
        })
    }
}

#[async_trait]
impl LlmEngine for OllamaEngine {
    fn id(&self) -> &str {
        &self.id
    }

    async fn generate(&self, req: GenerateRequest) -> Result<Message> {
        let body = self.build_body(&req, false);
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AionError::Llm(format!("petición a Ollama falló: {e}")))?;

        if !resp.status().is_success() {
            return Err(AionError::Llm(format!("Ollama devolvió {}", resp.status())));
        }

        let parsed: OllamaChatResponse = resp
            .json()
            .await
            .map_err(|e| AionError::Llm(format!("respuesta inválida de Ollama: {e}")))?;

        Ok(Message {
            role: Role::Assistant,
            content: parsed.message.content,
            thinking: parsed.message.thinking.filter(|t| !t.is_empty()),
        })
    }

    async fn generate_stream(
        &self,
        req: GenerateRequest,
        mut on_chunk: Box<dyn FnMut(StreamChunk) + Send>,
    ) -> Result<()> {
        let body = self.build_body(&req, true);
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AionError::Llm(format!("petición a Ollama falló: {e}")))?;

        if !resp.status().is_success() {
            return Err(AionError::Llm(format!("Ollama devolvió {}", resp.status())));
        }

        // Ollama emite NDJSON: una línea JSON por fragmento.
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(item) = stream.next().await {
            let bytes = item.map_err(|e| AionError::Llm(format!("error de stream: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(nl) = buf.find('\n') {
                let line = buf[..nl].trim().to_string();
                buf.drain(..=nl);
                if line.is_empty() {
                    continue;
                }
                let chunk: OllamaChatResponse = match serde_json::from_str(&line) {
                    Ok(c) => c,
                    Err(_) => continue, // línea parcial/no-JSON: ignorar
                };

                if let Some(t) = chunk.message.thinking {
                    if !t.is_empty() {
                        on_chunk(StreamChunk::Thinking { text: t });
                    }
                }
                if !chunk.message.content.is_empty() {
                    on_chunk(StreamChunk::Answer {
                        text: chunk.message.content,
                    });
                }
                if chunk.done {
                    let tps = match (chunk.eval_count, chunk.eval_duration) {
                        (Some(c), Some(d)) if d > 0 => c as f32 / (d as f32 / 1e9),
                        _ => 0.0,
                    };
                    on_chunk(StreamChunk::Done {
                        tokens: chunk.eval_count.unwrap_or(0),
                        tokens_per_sec: tps,
                    });
                }
            }
        }
        Ok(())
    }

    async fn health(&self) -> Result<()> {
        let resp = self
            .http
            .get(format!("{}/api/version", self.base_url))
            .send()
            .await
            .map_err(|e| AionError::Llm(format!("Ollama no responde: {e}")))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(AionError::Llm(format!(
                "Ollama no saludable: {}",
                resp.status()
            )))
        }
    }
}

// ── DTOs del protocolo Ollama ──────────────────────────────────────────────

#[derive(Serialize)]
struct OllamaMessage {
    role: &'static str,
    content: String,
}

impl From<&Message> for OllamaMessage {
    fn from(m: &Message) -> Self {
        let role = match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        OllamaMessage {
            role,
            content: m.content.clone(),
        }
    }
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaRespMessage,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default)]
    eval_duration: Option<u64>,
}

#[derive(Deserialize)]
struct OllamaRespMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_id_includes_model() {
        let e = OllamaEngine::new("http://localhost:11434", "gemma4-reason");
        assert_eq!(e.id(), "ollama:gemma4-reason");
    }

    #[test]
    fn body_includes_think_flag() {
        let e = OllamaEngine::default_local();
        let req = GenerateRequest {
            messages: vec![Message::user("hola")],
            think: true,
            temperature: Some(1.0),
            max_tokens: None,
        };
        let body = e.build_body(&req, false);
        assert_eq!(body["think"], true);
        assert_eq!(body["stream"], false);
        assert_eq!(body["options"]["temperature"], 1.0);
    }
}
