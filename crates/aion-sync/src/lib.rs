//! # aion-sync
//!
//! Sincronización CRDT (Automerge) cifrada E2E entre dispositivos.
//!
//! Estado: stub de F0. La implementación llega en su fase correspondiente
//! (ver docs/PRD y el plan maestro). Depende de `aion-kernel` para los contratos.

/// Marcador de versión del crate (placeholder hasta implementación).
pub const CRATE: &str = "aion-sync";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert!(!super::CRATE.is_empty());
    }
}
