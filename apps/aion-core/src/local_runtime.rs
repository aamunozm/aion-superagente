//! **Abstracción del runtime de inferencia LOCAL.**
//!
//! AION es local-first, pero el *motor* local no debe ser una pieza fija: hoy es Ollama,
//! mañana podría ser MLX, mistral.rs o llama.cpp directo — según el equipo y lo que vaya
//! mejor. Este trait es la costura: el resto del sistema pide "asegura el runtime local" y
//! "apágalo", sin saber CUÁL es. Para añadir un motor nuevo basta una implementación más y
//! una rama en [`active`]; nada más cambia.
//!
//! Complementa a [`aion_kernel::traits::LlmEngine`]: aquel abstrae cómo se *invoca* el
//! modelo (generar texto); este abstrae cómo se *arranca y supervisa* el proceso local que
//! lo sirve.

use async_trait::async_trait;

/// Ciclo de vida de un runtime de inferencia local. Implementaciones: [`OllamaRuntime`].
#[async_trait]
pub trait LocalRuntime: Send + Sync {
    /// Nombre legible (para logs y diagnóstico).
    fn name(&self) -> &'static str;

    /// Garantiza que el runtime esté sirviendo. Idempotente (si ya hay uno, lo reutiliza) y
    /// *fail-open* (si no puede, no paniquea). Devuelve `true` si quedó disponible.
    async fn ensure_running(&self) -> bool;

    /// Termina el runtime SOLO si lo lanzamos nosotros; uno externo del usuario no se toca.
    fn shutdown(&self);
}

/// Selecciona el runtime local activo según `provider.runtime`. Hoy solo Ollama; cuando
/// llegue MLX/mistral.rs se añaden aquí — el resto del sistema no se entera.
fn active() -> Box<dyn LocalRuntime> {
    match crate::provider::load().runtime.trim() {
        // "mlx" => Box::new(crate::mlx_runtime::MlxRuntime),
        "ollama" | "" => Box::new(crate::ollama_runtime::OllamaRuntime),
        other => {
            tracing::warn!(runtime = other, "runtime local desconocido; uso Ollama");
            Box::new(crate::ollama_runtime::OllamaRuntime)
        }
    }
}

/// Asegura el runtime local activo (lo que el resto del sistema llama al arrancar).
pub async fn ensure() -> bool {
    let rt = active();
    let ok = rt.ensure_running().await;
    if !ok {
        tracing::warn!(runtime = rt.name(), "el runtime local no quedó disponible");
    }
    ok
}

/// Apaga el runtime local activo (en el cierre limpio de AION).
pub fn shutdown() {
    active().shutdown();
}
