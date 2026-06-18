//! McpConsumerTool — cliente genérico JSON-RPC 2.0 para servidores MCP externos.

use aion_orchestrator::{Tool, ToolCategory};
use async_trait::async_trait;

/// Herramienta que delega la ejecución a un servidor MCP externo via JSON-RPC 2.0.
#[allow(dead_code)]
pub struct McpConsumerTool {
    pub tool_name: String,
    pub tool_description: String,
    pub mcp_server_url: String,
    pub input_schema: serde_json::Value,
}

impl McpConsumerTool {
    #[allow(dead_code)]
    pub fn new(
        tool_name: impl Into<String>,
        tool_description: impl Into<String>,
        mcp_server_url: impl Into<String>,
        input_schema: serde_json::Value,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            tool_description: tool_description.into(),
            mcp_server_url: mcp_server_url.into(),
            input_schema,
        }
    }
}

#[async_trait]
impl Tool for McpConsumerTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::External
    }

    fn schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    async fn run(&self, input: &str) -> Result<String, String> {
        let client = reqwest::Client::new();

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": self.tool_name,
                "arguments": {
                    "input": input
                }
            }
        });

        let resp = client
            .post(&self.mcp_server_url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Error de red al contactar servidor MCP: {e}"))?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Respuesta MCP no es JSON válido: {e}"))?;

        if !status.is_success() {
            return Err(format!("Servidor MCP devolvió estado {status}: {}", body));
        }

        // Extraer error JSON-RPC si existe
        if let Some(err) = body.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("error desconocido");
            return Err(format!("Error JSON-RPC del servidor MCP: {msg}"));
        }

        // Extraer result.content
        let result = body
            .get("result")
            .ok_or_else(|| "Respuesta MCP sin campo 'result'".to_string())?;

        // El protocolo MCP devuelve result.content como array de {type, text}
        if let Some(content_arr) = result.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<&str> = content_arr
                .iter()
                .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                .collect();
            if !texts.is_empty() {
                return Ok(texts.join("\n"));
            }
        }

        // Fallback: serializar el resultado completo
        Ok(result.to_string())
    }
}
