//! Puente F3.2: expone una skill WASM (sandbox) como herramienta del agente.
//!
//! Conecta el `SkillHost` (aion-skills) con el `Tool` del orquestador, de modo
//! que el agente ReAct puede invocar código WASM acotado dentro de su bucle.

use aion_kernel::traits::SkillHost;
use aion_orchestrator::Tool;
use aion_skills::WasmSkillHost;
use async_trait::async_trait;
use std::sync::Arc;

/// Herramienta del agente respaldada por una skill WASM en sandbox.
pub struct SkillTool {
    host: Arc<WasmSkillHost>,
    name: String,
    description: String,
}

impl SkillTool {
    pub fn new(
        host: Arc<WasmSkillHost>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            host,
            name: name.into(),
            description: description.into(),
        }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let trimmed = input.trim();
        // Acepta un entero ("50") o JSON ({"n":50}).
        let value: serde_json::Value = match trimmed.parse::<i64>() {
            Ok(n) => serde_json::json!(n),
            Err(_) => serde_json::from_str(trimmed)
                .map_err(|_| format!("entrada inválida para skill: '{trimmed}'"))?,
        };
        let out = self
            .host
            .invoke(&self.name, value)
            .await
            .map_err(|e| e.to_string())?;
        Ok(out.output["result"].to_string())
    }
}
