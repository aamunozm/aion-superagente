//! **Compactación EN para el puente MCP con Claude Code.**
//!
//! AION es local-first: el chat con Gemma corre on-device y sus tokens son *gratis*.
//! Pero cuando un agente externo (Claude Code) consulta la memoria de AION vía MCP
//! (`aion_memory_search`, `aion_brief`…), ese texto entra en el contexto de un modelo
//! de **pago por token**. La memoria de AION está en español, y el español cuesta
//! ~40% más tokens que el mismo hecho en inglés (medido con tiktoken sobre recuerdos
//! reales). Ese 40% es el único ahorro que importa, y solo aquí.
//!
//! **Diseño** (el idioma se ata al CONSUMIDOR, no al almacenamiento):
//! - La memoria se guarda y se sirve a Gemma SIEMPRE en español (íntegra, sin tocar).
//! - Solo el puente MCP recibe una versión **inglesa** equivalente.
//! - La traduce **Gemma local** (gratis), fiel y literal — NO un quita-stopwords.
//! - Se **precomputa y cachea** por hash de contenido; nunca se traduce en caliente
//!   dentro de la llamada MCP (eso metería latencia de Gemma a cada búsqueda).
//! - **Fail-open absoluto**: si no hay traducción cacheada, se sirve el español
//!   original y se dispara la traducción en segundo plano para la próxima vez.
//!
//! Nunca corrompe ni bloquea: en el peor caso, Claude Code ve español (lo entiende
//! igual), solo paga unos tokens de más hasta que la caché se calienta.

use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Caché persistente { hash_contenido → versión inglesa }. Se carga del disco una vez.
fn cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let map = std::fs::read_to_string(cache_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Mutex::new(map)
    })
}

fn cache_path() -> std::path::PathBuf {
    crate::app_data_dir().join("mcp_compact_en.json")
}

/// Clave estable: SHA-256 del contenido, hex truncado (colisión despreciable).
fn key(content: &str) -> String {
    let mut h = Sha256::new();
    h.update(content.as_bytes());
    let d = h.finalize();
    hex16(&d[..8])
}

fn hex16(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Separa una etiqueta de procedencia inicial `[hecho] …` del cuerpo. La etiqueta es
/// estructura (no se traduce); el cuerpo sí. Devuelve `(Some("[hecho]"), "…")`.
fn split_tag(s: &str) -> (Option<&str>, &str) {
    let s = s.trim_start();
    if s.starts_with('[') {
        if let Some(close) = s.find(']') {
            let tag = &s[..=close]; // "[hecho]"
            let body = s[close + 1..].trim_start();
            return (Some(tag), body);
        }
    }
    (None, s)
}

/// Versión cacheada en inglés para este contenido, si existe. Instantáneo, fail-open.
pub fn english_for(content: &str) -> Option<String> {
    cache().lock().ok()?.get(&key(content)).cloned()
}

/// Sirve la versión óptima para el puente MCP: inglés si está cacheado; si no, devuelve
/// el español original ESTA vez y calienta la traducción en segundo plano.
pub fn compact_for_bridge(content: &str) -> String {
    if let Some(en) = english_for(content) {
        return en;
    }
    let owned = content.to_string();
    tokio::spawn(async move {
        let _ = ensure_english(&owned).await;
    });
    content.to_string()
}

/// Motor LOCAL (Ollama/Gemma) forzado — la traducción debe ser GRATIS aunque el motor
/// activo del usuario sea una API externa de pago.
fn local_engine() -> Arc<dyn LlmEngine> {
    let cfg = crate::provider::load();
    let model = if cfg.local_model.is_empty() {
        "gemma4-reason".to_string()
    } else {
        cfg.local_model
    };
    Arc::new(aion_llm::OllamaEngine::new(
        aion_llm::OllamaEngine::base_url_from_env(),
        &model,
    ))
}

/// Traduce+compacta el contenido a inglés con Gemma local y lo cachea. Idempotente:
/// si ya está, no rehace. Devuelve la versión inglesa (o `None` si se salta/falla).
pub async fn ensure_english(content: &str) -> Option<String> {
    let k = key(content);
    if let Some(en) = cache().lock().ok().and_then(|c| c.get(&k).cloned()) {
        return Some(en);
    }
    let trimmed = content.trim();
    // Saltar lo trivial y lo que NO tiene señal de español (ya está en inglés/código →
    // traducirlo no ahorra nada). Gate sesgado a traducir: ver `has_spanish_signal`.
    if trimmed.chars().count() < 40 {
        return None;
    }
    if !crate::language_detector::has_spanish_signal(trimmed) {
        return None;
    }
    let (tag, body) = split_tag(trimmed);
    if body.chars().count() < 20 {
        return None;
    }
    let engine = local_engine();
    let req = GenerateRequest {
        messages: vec![Message::user(format!(
            "Translate the following Spanish note into clear, faithful English. \
             Preserve EVERY fact, name, number, path and identifier exactly as-is. \
             Be concise but omit nothing. Output ONLY the English translation, with no \
             preamble, quotes or notes.\n\n{body}"
        ))],
        think: false,
        temperature: Some(0.1),
        max_tokens: Some(220),
    };
    let en = engine.generate(req).await.ok()?.content.trim().to_string();
    // Sanidad: una traducción vacía o sospechosamente corta no reemplaza al original.
    if en.is_empty() || en.chars().count() < body.chars().count() / 5 {
        return None;
    }
    let full = match tag {
        Some(t) => format!("{t} {en}"),
        None => en,
    };
    if let Ok(mut c) = cache().lock() {
        c.insert(k, full.clone());
        let n = c.len();
        persist(&c);
        tracing::debug!(
            cached = n,
            "mcp_compact: recuerdo traducido a inglés y cacheado"
        );
    }
    Some(full)
}

/// Persiste la caché de forma atómica (tmp + rename) — un crash nunca deja JSON a medias.
fn persist(map: &HashMap<String, String>) {
    if let Ok(json) = serde_json::to_string(map) {
        crate::write_atomic(&cache_path(), &json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_distinct() {
        assert_eq!(key("hola mundo"), key("hola mundo"));
        assert_ne!(key("hola mundo"), key("hola mundos"));
    }

    #[test]
    fn split_tag_extracts_provenance() {
        let (tag, body) = split_tag("[hecho] Ariel usa Rust");
        assert_eq!(tag, Some("[hecho]"));
        assert_eq!(body, "Ariel usa Rust");
    }

    #[test]
    fn split_tag_handles_no_tag() {
        let (tag, body) = split_tag("sin etiqueta aquí");
        assert_eq!(tag, None);
        assert_eq!(body, "sin etiqueta aquí");
    }

    #[test]
    fn english_for_miss_is_none() {
        assert!(english_for("contenido jamás cacheado xyzzy 9f3a").is_none());
    }
}
