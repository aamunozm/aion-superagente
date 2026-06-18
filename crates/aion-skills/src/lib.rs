//! # aion-skills
//!
//! Host de **skills WASM** de AION. Ejecuta código (potencialmente auto-generado)
//! en un sandbox **deny-all**: el módulo WASM no recibe ninguna función del host,
//! por lo que por construcción no puede acceder a disco, red ni al sistema.
//! Además se limita el cómputo con *fuel* para frenar bucles infinitos.
//!
//! Esto convierte el requisito más peligroso (que el agente ejecute código que
//! se escribe a sí mismo) en algo seguro: el radio de daño está acotado por el
//! sandbox. wasmtime es la base sobre la que se construye Extism; en F5 se añade
//! la capa de capabilities (conceder red/FS explícitamente a skills de confianza).

mod host;
pub mod skillbook;

pub use host::{SkillManifest, WasmSkillHost, SUM_TO_WAT};
pub use skillbook::{Procedure, ProcedureStep, SkillBook};
