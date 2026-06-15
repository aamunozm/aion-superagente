//! Motor LLM para APIs **OpenAI-compatible** (OpenRouter, Groq, Google Gemini vía
//! compat, OpenAI, etc.). Implementa [`LlmEngine`] usando `/chat/completions`.
//! Permite a AION usar una API externa en vez del modelo local.

use aion_kernel::traits::{GenerateRequest, LlmEngine, StreamChunk};
use aion_kernel::types::{Message, Role};
use aion_kernel::{AionError, Result};
use async_trait::async_trait;
use futures_util::StreamExt;

/// Cliente HTTP COMPARTIDO para todas las llamadas de chat externas. `reqwest::Client`
/// mantiene un pool de conexiones interno y está pensado para reutilizarse: clonarlo es
/// barato (Arc) y comparte el pool. Como `active_engine()` reconstruye el motor en CADA
/// turno, sin esto cada turno abría una conexión TLS nueva a la API (handshake extra). Sin
/// timeout global a propósito: una generación larga (streaming) no debe expirar.
fn shared_http() -> reqwest::Client {
    static HTTP: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    HTTP.get_or_init(reqwest::Client::new).clone()
}

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
            http: shared_http(),
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
        let msg = &v["choices"][0]["message"];
        let content = msg["content"].as_str().unwrap_or("").to_string();
        // Modelos de razonamiento (deepseek-reasoner/v4, Gemini thinking…) devuelven la
        // cadena de pensamiento en `reasoning_content`, aparte de `content`. La conservamos
        // como `thinking` (igual que OllamaEngine) en vez de descartarla.
        let thinking = msg["reasoning_content"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        Ok(Message {
            role: Role::Assistant,
            content,
            thinking,
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
        // tok/s REAL = tokens de respuesta / tiempo transcurrido. Antes iba hardcodeado a 0.0,
        // así que la UI mostraba siempre «0 tok/s» con cualquier API externa.
        let start = std::time::Instant::now();
        let tps = |tokens: u32| -> f32 {
            let secs = start.elapsed().as_secs_f32();
            if secs > 0.0 {
                tokens as f32 / secs
            } else {
                0.0
            }
        };
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
                        tokens_per_sec: tps(tokens),
                    });
                    return Ok(());
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    let delta = &v["choices"][0]["delta"];
                    // Modelos de razonamiento emiten su pensamiento en `reasoning_content`,
                    // separado de la respuesta. Lo transmitimos como Thinking (paridad con
                    // Ollama) en vez de descartarlo: antes el usuario veía un parón largo SIN
                    // nada en pantalla mientras el modelo «pensaba».
                    if let Some(r) = delta["reasoning_content"].as_str() {
                        if !r.is_empty() {
                            on_chunk(StreamChunk::Thinking {
                                text: r.to_string(),
                            });
                        }
                    }
                    if let Some(t) = delta["content"].as_str() {
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
            tokens_per_sec: tps(tokens),
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

/// Valida una API OpenAI-compatible listando sus modelos (`GET /models`) y los devuelve
/// ordenados. Sirve para el botón «Probar» de la UI: confirma de un golpe que la
/// `base_url` y la API key son correctas Y obtiene los modelos reales para que el usuario
/// elija uno del desplegable en vez de escribirlo a mano.
///
/// Usa un cliente con timeout propio (12 s): NO reutiliza el del motor, que no debe
/// expirar durante un streaming largo. Mapea los errores comunes (401/403/404) a
/// mensajes accionables en español.
pub async fn list_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    let base = base_url.trim_end_matches('/');
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| AionError::Llm(format!("no se pudo crear el cliente HTTP: {e}")))?;
    let resp = http
        .get(format!("{base}/models"))
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| AionError::Llm(format!("no se pudo contactar la API: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AionError::Llm(match status.as_u16() {
            401 | 403 => "API key inválida o sin permisos".to_string(),
            404 => "este endpoint no expone /models; escribe el modelo a mano".to_string(),
            429 => "la API respondió 429 (límite de uso); reintenta en unos segundos".to_string(),
            code => format!("la API devolvió {code}"),
        }));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AionError::Llm(format!("respuesta inválida de la API: {e}")))?;
    // Formato estándar OpenAI: { "data": [ { "id": "modelo-x", ... }, ... ] }.
    let mut ids: Vec<String> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str())
                // Filtra modelos que NO sirven para chat (ruido típico de Gemini: embeddings,
                // AQA, generación de imagen/vídeo). Si tras filtrar no queda nada, no filtramos
                // (fail-open: mejor una lista con ruido que una vacía).
                .filter(|id| {
                    let l = id.to_lowercase();
                    !(l.contains("embedding")
                        || l.contains("aqa")
                        || l.contains("imagen")
                        || l.contains("veo")
                        || l.contains("tts"))
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    // Fail-open: si el filtro dejó la lista vacía pero la API sí devolvió modelos, no filtres.
    if ids.is_empty() {
        ids = v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}
