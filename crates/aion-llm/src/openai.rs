//! Motor LLM para APIs **OpenAI-compatible** (OpenRouter, Groq, Google Gemini vía
//! compat, OpenAI, etc.). Implementa [`LlmEngine`] usando `/chat/completions`.
//! Permite a AION usar una API externa en vez del modelo local.

use aion_kernel::traits::{GenerateRequest, LlmEngine, StreamChunk};
use aion_kernel::types::{Message, Role};
use aion_kernel::{AionError, Result};
use async_trait::async_trait;
use futures_util::StreamExt;

/// Cliente para una API OpenAI-compatible.
pub struct OpenAiEngine {
    base_url: String,
    api_key: String,
    model: String,
    id: String,
    http: reqwest::Client,
}

impl OpenAiEngine {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let base_url = base_url.into();
        let model = model.into();
        Self {
            id: format!("openai-compat:{model}"),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model,
            http: reqwest::Client::new(),
        }
    }

    fn body(&self, req: &GenerateRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<_> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                serde_json::json!({ "role": role, "content": m.content })
            })
            .collect();
        serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": stream,
            "temperature": req.temperature.unwrap_or(1.0),
        })
    }
}

#[async_trait]
impl LlmEngine for OpenAiEngine {
    fn id(&self) -> &str {
        &self.id
    }

    async fn generate(&self, req: GenerateRequest) -> Result<Message> {
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&self.body(&req, false))
            .send()
            .await
            .map_err(|e| AionError::Llm(format!("petición a la API falló: {e}")))?;
        if !resp.status().is_success() {
            return Err(AionError::Llm(format!("API devolvió {}", resp.status())));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AionError::Llm(format!("respuesta inválida: {e}")))?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(Message {
            role: Role::Assistant,
            content,
            thinking: None,
        })
    }

    async fn generate_stream(
        &self,
        req: GenerateRequest,
        mut on_chunk: Box<dyn FnMut(StreamChunk) + Send>,
    ) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&self.body(&req, true))
            .send()
            .await
            .map_err(|e| AionError::Llm(format!("petición a la API falló: {e}")))?;
        if !resp.status().is_success() {
            return Err(AionError::Llm(format!("API devolvió {}", resp.status())));
        }
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut tokens = 0u32;
        while let Some(item) = stream.next().await {
            let bytes = item.map_err(|e| AionError::Llm(format!("stream: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(nl) = buf.find('\n') {
                let line = buf[..nl].trim().to_string();
                buf.drain(..=nl);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    on_chunk(StreamChunk::Done {
                        tokens,
                        tokens_per_sec: 0.0,
                    });
                    return Ok(());
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                        if !t.is_empty() {
                            tokens += 1;
                            on_chunk(StreamChunk::Answer {
                                text: t.to_string(),
                            });
                        }
                    }
                }
            }
        }
        on_chunk(StreamChunk::Done {
            tokens,
            tokens_per_sec: 0.0,
        });
        Ok(())
    }

    async fn health(&self) -> Result<()> {
        // Una API externa configurada se considera disponible (se valida al usarla).
        if self.api_key.is_empty() || self.base_url.is_empty() {
            return Err(AionError::Llm("API externa sin configurar".into()));
        }
        Ok(())
    }
}
