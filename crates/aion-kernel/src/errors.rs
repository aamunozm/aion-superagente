//! Errores unificados del sistema AION.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, AionError>;

#[derive(Debug, Error)]
pub enum AionError {
    #[error("error del motor LLM: {0}")]
    Llm(String),

    #[error("error de memoria: {0}")]
    Memory(String),

    #[error("error de skill: {0}")]
    Skill(String),

    #[error("acción bloqueada por política de seguridad: {0}")]
    PolicyDenied(String),

    #[error("integridad del kernel comprometida: {0}")]
    KernelIntegrity(String),

    #[error("error de serialización: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("error de E/S: {0}")]
    Io(String),

    #[error("error interno: {0}")]
    Internal(String),
}
