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
    /// Modelo LOCAL ligero, OPCIONAL, para tareas de FONDO (traducción del puente MCP,
    /// resúmenes, extracción de lecciones). Independiente del modelo de chat: en un equipo
    /// potente puedes chatear con uno grande y hacer el trabajo de fondo con uno de 1-3B;
    /// en uno modesto, todo ligero. Vacío → las tareas de fondo usan `local_model`.
    #[serde(default)]
    pub utility_model: String,
    /// Modelo LOCAL especializado en TRADUCCIÓN, OPCIONAL (p. ej. "translategemma:12b" o
    /// "gemmax2"). Solo lo usa la compactación ES/IT→EN del puente MCP (`mcp_compact`). Un
    /// modelo afinado para traducción interpreta mejor la INTENCIÓN que el genérico (ver
    /// auditoría de interpretación). Vacío → cae a `utility_model`/`local_model`. El override
    /// por env `AION_TRANSLATION_MODEL` tiene prioridad (para probar sin tocar el archivo).
    #[serde(default)]
    pub translation_model: String,
    /// Runtime de inferencia LOCAL: "ollama" (por defecto). Costura para futuros motores
    /// (MLX, mistral.rs…). Vacío se trata como "ollama". Ver crate::local_runtime.
    #[serde(default)]
    pub runtime: String,
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
            utility_model: String::new(),
            translation_model: String::new(),
            runtime: "ollama".into(),
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
    /// Modelo LOCAL para tareas de fondo: el utilitario ligero si está configurado, si no
    /// el local normal. Cadena vacía si no hay ninguno (las tareas de fondo se saltan).
    #[allow(dead_code)]
    pub fn background_model(&self) -> String {
        let util = self.utility_model.trim();
        if !util.is_empty() {
            return util.to_string();
        }
        self.local_model.trim().to_string()
    }

    /// Modelo LOCAL para TRADUCIR (puente MCP). Prioridad: env `AION_TRANSLATION_MODEL` >
    /// `translation_model` configurado > `background_model()`. Así un traductor especializado
    /// (TranslateGemma/GemmaX2) es un cambio de CONFIG, no de código, con fallback seguro: si
    /// no hay nada especializado, traduce con el de fondo de siempre (fail-open conservado).
    #[allow(dead_code)]
    pub fn translation_model(&self) -> String {
        if let Ok(env) = std::env::var("AION_TRANSLATION_MODEL") {
            let env = env.trim();
            if !env.is_empty() {
                return env.to_string();
            }
        }
        let tm = self.translation_model.trim();
        if !tm.is_empty() {
            return tm.to_string();
        }
        self.background_model()
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
    // El modelo utilitario es independiente del motor activo: si la UI no lo reenvía,
    // se conserva el previo (no se pierde al alternar local↔API).
    if out.utility_model.trim().is_empty() {
        out.utility_model = prev.utility_model.clone();
    }
    // El modelo de traducción también es independiente del motor activo: se conserva.
    if out.translation_model.trim().is_empty() {
        out.translation_model = prev.translation_model.clone();
    }
    // Igual con el runtime local elegido.
    if out.runtime.trim().is_empty() {
        out.runtime = prev.runtime.clone();
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
