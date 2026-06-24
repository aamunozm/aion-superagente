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
    /// Penalización de repetición (solo motores que la soportan, p. ej. mlx_lm.server).
    /// Imprescindible para el cerebro de voz local: sin ella, Qwen3-4B 4-bit degenera en
    /// bucles ("tienes tienes tienes…"). None = no se envía (APIs como DeepSeek la ignoran).
    repetition_penalty: Option<f32>,
    /// top_p (nucleus). None = no se envía.
    top_p: Option<f32>,
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
            repetition_penalty: None,
            top_p: None,
        }
    }

    /// Fija el muestreo anti-bucle (penalización de repetición + top_p) para este motor.
    /// Pensado para el cerebro de voz local; no afecta a otros motores.
    pub fn with_sampling(mut self, repetition_penalty: f32, top_p: f32) -> Self {
        self.repetition_penalty = Some(repetition_penalty);
        self.top_p = Some(top_p);
        self
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
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": stream,
            "temperature": req.temperature.unwrap_or(1.0),
        });
        let obj = body.as_object_mut().expect("body es objeto");
        // max_tokens: ACOTA la respuesta (en voz evita respuestas de 512 tokens = ~8s y
        // limita el daño de cualquier bucle). Antes no se enviaba → el servidor usaba su tope.
        if let Some(m) = req.max_tokens {
            obj.insert("max_tokens".into(), m.into());
        }
        // repetition_penalty / top_p: anti-degeneración del cerebro de voz local.
        if let Some(rp) = self.repetition_penalty {
            obj.insert("repetition_penalty".into(), rp.into());
        }
        if let Some(tp) = self.top_p {
            obj.insert("top_p".into(), tp.into());
        }
        body
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
        // Búfer de BYTES (no String): evita corromper multibyte partido entre chunks. Ver
        // `crate::take_line`.
        let mut buf: Vec<u8> = Vec::new();
        let mut tokens = 0u32;
        while let Some(item) = stream.next().await {
            let bytes = item.map_err(|e| AionError::Llm(format!("stream: {e}")))?;
            buf.extend_from_slice(&bytes);
            while let Some(line) = crate::take_line(&mut buf) {
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
