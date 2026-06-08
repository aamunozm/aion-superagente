//! Puente F4.2: expone la memoria de largo plazo como herramienta del agente.
//! Permite que el agente ReAct **recuerde** (recupere recuerdos) durante su bucle.

use aion_kernel::traits::MemoryStore;
use aion_memory::VectorMemory;
use aion_orchestrator::Tool;
use async_trait::async_trait;
use std::sync::Arc;

/// Herramienta de recuperación sobre la memoria persistente de AION.
pub struct MemoryTool {
    memory: Arc<VectorMemory>,
    k: usize,
}

impl MemoryTool {
    pub fn new(memory: Arc<VectorMemory>, k: usize) -> Self {
        Self { memory, k }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory_search"
    }
    fn description(&self) -> &str {
        "Busca en la memoria de largo plazo de AION. Entrada: una consulta en texto. \
         Devuelve los recuerdos más relevantes."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let hits = self
            .memory
            .retrieve(input, self.k)
            .await
            .map_err(|e| e.to_string())?;
        if hits.is_empty() {
            return Ok("(sin recuerdos relevantes)".into());
        }
        Ok(hits
            .iter()
            .map(|h| format!("- {} (rel {:.2})", h.content, h.score))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
