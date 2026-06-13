//! Configuración del proveedor de LLM elegido en el onboarding: modelo LOCAL
//! (Ollama) o una API externa OpenAI-compatible (OpenRouter, Groq, Google…).
//! Persistido en el directorio de datos para que la elección sobreviva.
//!
//! `model`/`base_url`/`api_key` describen el motor ACTIVO (lo que lee `build_engine`
//! y el resto del sistema). Además recordamos `local_model` y `ext_model` para poder
//! ALTERNAR local↔API desde el chat sin perder la otra configuración: las credenciales
//! externas se conservan aunque el motor activo sea local (`build_engine` las ignora
//! mientras `kind == "local"`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// "local" (Ollama) o "external" (API OpenAI-compatible).
    pub kind: String,
    /// Modelo del motor ACTIVO (p. ej. "gemma4-reason" o "gemini-2.5-flash").
    pub model: String,
    /// Base URL para API externa (p. ej. https://api.deepseek.com).
    #[serde(default)]
    pub base_url: String,
    /// API key para la API externa (se guarda local, 0600).
    #[serde(default)]
    pub api_key: String,
    /// Último modelo LOCAL usado (recordado para alternar de vuelta a Ollama).
    #[serde(default)]
    pub local_model: String,
    /// Último modelo EXTERNO usado (recordado para alternar de vuelta a la API).
    #[serde(default)]
    pub ext_model: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: "local".into(),
            model: "gemma4-reason".into(),
            base_url: String::new(),
            api_key: String::new(),
            local_model: "gemma4-reason".into(),
            ext_model: String::new(),
        }
    }
}

impl ProviderConfig {
    /// ¿Hay una API externa configurada (recordada), aunque el motor activo sea local?
    pub fn has_external(&self) -> bool {
        !self.base_url.is_empty() && !self.api_key.is_empty() && !self.ext_model.is_empty()
    }
    /// ¿Hay un modelo local recordado al que volver?
    pub fn has_local(&self) -> bool {
        !self.local_model.is_empty()
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

/// Fusiona una config entrante (de la UI) con la guardada, conservando lo que la UI
/// no reenvía: la API key (que nunca vuelve al cliente) y la configuración del motor
/// NO activo (para poder alternar). Esto es lo que hace seguro pasar de local↔API.
pub fn merge(incoming: ProviderConfig) -> ProviderConfig {
    let prev = load();
    let mut out = incoming;
    if out.kind == "external" {
        // La key solo se conserva si apunta al MISMO endpoint y la UI no la reescribió.
        if out.api_key.is_empty() && out.base_url == prev.base_url {
            out.api_key = prev.api_key.clone();
        }
        // El modelo activo es el externo → recuérdalo; conserva el local de antes.
        if !out.model.is_empty() {
            out.ext_model = out.model.clone();
        }
        out.local_model = if !prev.local_model.is_empty() {
            prev.local_model.clone()
        } else if prev.kind == "local" {
            prev.model.clone()
        } else {
            prev.local_model.clone()
        };
    } else {
        // Motor local activo: recuerda el modelo local…
        out.local_model = out.model.clone();
        // …y CONSERVA las credenciales externas (ignoradas mientras kind=local).
        if out.base_url.is_empty() {
            out.base_url = prev.base_url.clone();
        }
        if out.api_key.is_empty() {
            out.api_key = prev.api_key.clone();
        }
        out.ext_model = if !prev.ext_model.is_empty() {
            prev.ext_model.clone()
        } else if prev.kind == "external" {
            prev.model.clone()
        } else {
            prev.ext_model.clone()
        };
    }
    out
}

pub fn save(cfg: &ProviderConfig) -> std::io::Result<()> {
    // Contiene la API key de la API externa: se escribe con permisos 0600 (owner-only)
    // y rename atómico, igual que el resto de secretos. Nunca world-readable.
    let json = serde_json::to_string_pretty(cfg)?;
    crate::write_atomic_secret(&path(), &json);
    Ok(())
}
