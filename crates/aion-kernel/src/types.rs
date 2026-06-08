//! Tipos compartidos del dominio.

use serde::{Deserialize, Serialize};

/// Rol de un mensaje en una conversación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Un mensaje de conversación. `thinking` guarda el razonamiento (bloque `<think>`)
/// por separado del contenido final mostrado al usuario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            thinking: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            thinking: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            thinking: None,
        }
    }
}

/// Información identificativa del kernel para verificación de integridad.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub contract_version: u32,
}
