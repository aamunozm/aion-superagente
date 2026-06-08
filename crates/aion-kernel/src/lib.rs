//! # aion-kernel
//!
//! El **núcleo inmutable** de AION. Define los contratos (traits), tipos, eventos
//! y errores de los que dependen todos los demás crates. El bucle de auto-evolución
//! (F5) NUNCA puede modificar este crate: su hash se verifica al arranque.
//!
//! Principio: el kernel no conoce implementaciones concretas (Ollama, LanceDB,
//! Extism…), solo abstracciones. Esto permite intercambiar motores sin tocar el
//! resto del sistema (p. ej. `OllamaEngine` → `MistralRsEngine` en F2).

pub mod errors;
pub mod events;
pub mod traits;
pub mod types;

pub use errors::{AionError, Result};
pub use events::{AionEvent, EventBus};
pub use traits::{LlmEngine, MemoryStore, SkillHost};
pub use types::{KernelInfo, Message, Role};

/// Versión del contrato del kernel. Cambiarla es un evento mayor: implica que
/// las implementaciones deben re-validarse.
pub const KERNEL_CONTRACT_VERSION: u32 = 1;

/// Información identificativa del kernel, usada por la verificación de integridad
/// (el bucle evolutivo comprueba que el kernel no ha sido alterado).
pub fn kernel_info() -> KernelInfo {
    KernelInfo {
        name: "aion-kernel",
        version: env!("CARGO_PKG_VERSION"),
        contract_version: KERNEL_CONTRACT_VERSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_info_is_stable() {
        let info = kernel_info();
        assert_eq!(info.name, "aion-kernel");
        assert_eq!(info.contract_version, KERNEL_CONTRACT_VERSION);
    }
}
