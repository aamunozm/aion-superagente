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
    /// **Hub WebSocket** del peer (p. ej. `wss://api.ceo-intelligence.com/api/v1/ws/a2a`). Cuando
    /// está, AION abre un canal SALIENTE persistente a él con `token` y recibe mensajes aunque esté
    /// fuera de la LAN (atraviesa NAT). Se rellena al pegar el «código de conexión» del peer.
    #[serde(default)]
    pub hub: String,
    /// Otros agentes conocidos (nombre + URL base, p. ej. http://192.168.1.20:8765). Modelo HTTP
    /// directo (LAN): coexiste con el hub WS (que es el camino para movilidad).
    #[serde(default)]
    pub peers: Vec<Peer>,
}

/// Decodifica un **código de conexión** del peer (base64url de `{"hub": "...", "token": "..."}`)
/// a `(hub, token)`. Es el atajo de un-pegado que genera CEO·Intelligence al registrar un agente.
pub fn decode_connect_code(code: &str) -> Option<(String, String)> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(code.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(code.trim()))
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let hub = v.get("hub")?.as_str()?.to_string();
    let token = v.get("token")?.as_str()?.to_string();
    if hub.is_empty() || token.is_empty() {
        return None;
    }
    Some((hub, token))
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
