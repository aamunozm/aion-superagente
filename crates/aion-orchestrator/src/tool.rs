//! Herramientas que el agente puede invocar, y su registro.
//!
//! En F3 las skills auto-generadas (WASM/Extism) se exponen como `Tool`. Aquí,
//! herramientas nativas de ejemplo (calculadora) que ya dan capacidades reales.

use crate::calc;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Una herramienta invocable por el agente.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn run(&self, input: &str) -> Result<String, String>;
}

/// Registro de herramientas disponibles para el agente.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Descripción para el prompt del agente.
    pub fn describe(&self) -> String {
        self.tools
            .values()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Calculadora aritmética determinista. Corrige la incapacidad del LLM para
/// la aritmética exacta delegando el cálculo a código.
pub struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }
    fn description(&self) -> &str {
        "Evalúa una expresión aritmética (+ - * / y paréntesis). Entrada: la expresión, p.ej. 47*89-1234"
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        calc::eval(input).map(|v| {
            if v.fract() == 0.0 {
                format!("{}", v as i64)
            } else {
                format!("{v}")
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn calculator_tool_runs() {
        let t = CalculatorTool;
        assert_eq!(t.run("47*89-1234").await.unwrap(), "2949");
    }

    #[test]
    fn registry_describes_tools() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        assert!(r.describe().contains("calculator"));
        assert!(r.get("calculator").is_some());
    }
}
