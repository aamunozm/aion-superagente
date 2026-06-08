//! Puente: expone la capacidad web (leer una URL) como herramienta del agente.

use aion_browser::WebClient;
use aion_orchestrator::Tool;
use async_trait::async_trait;
use std::sync::Arc;

/// Herramienta que descarga una URL y devuelve su texto legible.
pub struct WebTool {
    client: Arc<WebClient>,
}

impl WebTool {
    pub fn new(client: Arc<WebClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for WebTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Descarga una página web y devuelve su texto. Entrada: una URL http(s) completa."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        self.client
            .fetch_text(input.trim())
            .await
            .map_err(|e| e.to_string())
    }
}
