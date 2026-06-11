//! **A2A (Agent-to-Agent)** — comunicación entre AION y otros agentes. Config local
//! sencilla: activar, un token compartido (secreto) y una lista de pares (nombre+url).
//! Cada mensaje lleva la IDENTIDAD única de este AION ([[identity]]). El mensaje del
//! otro agente se trata como CONTENIDO EXTERNO no confiable (anti-inyección).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Peer {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Si AION acepta/inicia conversaciones con otros agentes.
    #[serde(default)]
    pub enabled: bool,
    /// Secreto compartido: ambos agentes deben tener el mismo para hablarse.
    #[serde(default)]
    pub token: String,
    /// Otros agentes conocidos (nombre + URL base, p. ej. http://192.168.1.20:8765).
    #[serde(default)]
    pub peers: Vec<Peer>,
}

fn path() -> PathBuf {
    crate::app_data_dir().join("a2a.json")
}

pub fn load() -> Config {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(c: &Config) {
    if let Some(p) = path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(b) = serde_json::to_string_pretty(c) {
        let _ = std::fs::write(path(), b);
    }
}
