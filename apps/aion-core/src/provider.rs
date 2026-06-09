//! Configuración del proveedor de LLM elegido en el onboarding: modelo LOCAL
//! (Ollama) o una API externa OpenAI-compatible (OpenRouter, Groq, Google…).
//! Persistido en el directorio de datos para que la elección sobreviva.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// "local" (Ollama) o "external" (API OpenAI-compatible).
    pub kind: String,
    /// Modelo a usar (p. ej. "gemma4-reason", "gemma3:4b", o "llama-3.3-70b-versatile").
    pub model: String,
    /// Base URL para API externa (p. ej. https://api.groq.com/openai/v1).
    #[serde(default)]
    pub base_url: String,
    /// API key para la API externa (se guarda local).
    #[serde(default)]
    pub api_key: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: "local".into(),
            model: "gemma4-reason".into(),
            base_url: String::new(),
            api_key: String::new(),
        }
    }
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("provider.json")
}

pub fn load() -> ProviderConfig {
    match std::fs::read_to_string(path()) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
        Err(_) => ProviderConfig::default(),
    }
}

pub fn save(cfg: &ProviderConfig) -> std::io::Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(cfg)?)?;
    std::fs::rename(&tmp, &p)
}
