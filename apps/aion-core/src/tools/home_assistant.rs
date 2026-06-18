//! HomeAssistantTool — control del hogar via Home Assistant REST API.

use aion_orchestrator::{Tool, ToolCategory};
use async_trait::async_trait;

/// Herramienta que integra AION con Home Assistant.
///
/// Variables de entorno:
/// - `AION_HA_URL`: URL base de Home Assistant (por defecto `http://homeassistant.local:8123`)
/// - `AION_HA_TOKEN`: Long-lived access token de HA (requerido para llamadas reales)
///
/// Modos de entrada:
/// - `"estados"` → lista hasta 50 dispositivos con entity_id, estado y nombre amigable.
/// - `"domain.service ::: entity_id"` → llama al servicio HA indicado sobre la entidad.
pub struct HomeAssistantTool;

impl HomeAssistantTool {
    fn base_url() -> String {
        std::env::var("AION_HA_URL")
            .unwrap_or_else(|_| "http://homeassistant.local:8123".to_string())
    }

    fn token() -> Option<String> {
        std::env::var("AION_HA_TOKEN").ok()
    }

    fn client_with_auth() -> Result<reqwest::Client, String> {
        let token = Self::token().ok_or_else(|| {
            "AION_HA_TOKEN no configurado. Añade tu Long-lived Access Token de Home Assistant."
                .to_string()
        })?;

        let mut headers = reqwest::header::HeaderMap::new();
        let auth_value = format!("Bearer {token}");
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth_value)
                .map_err(|e| format!("Token inválido: {e}"))?,
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| format!("Error al construir cliente HTTP: {e}"))
    }
}

#[async_trait]
impl Tool for HomeAssistantTool {
    fn name(&self) -> &str {
        "home_assistant"
    }

    fn description(&self) -> &str {
        "Control del hogar via Home Assistant. \
        Entrada: «estados» para ver dispositivos (máx 50), \
        o «domain.service ::: entity_id» para ejecutar un servicio \
        (ej: «light.turn_on ::: light.sala»). \
        Requiere AION_HA_URL y AION_HA_TOKEN."
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::External
    }

    async fn run(&self, input: &str) -> Result<String, String> {
        let input = input.trim();
        let base = Self::base_url();
        let client = Self::client_with_auth()?;

        if input.eq_ignore_ascii_case("estados") {
            // GET /api/states → lista de entidades
            let url = format!("{base}/api/states");
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("Error conectando a Home Assistant: {e}"))?;

            if !resp.status().is_success() {
                return Err(format!(
                    "Home Assistant respondió con estado {}: comprueba URL y token.",
                    resp.status()
                ));
            }

            let states: Vec<serde_json::Value> = resp
                .json()
                .await
                .map_err(|e| format!("Respuesta HA inválida: {e}"))?;

            let lines: Vec<String> = states
                .iter()
                .take(50)
                .map(|s| {
                    let entity_id = s.get("entity_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let state = s.get("state").and_then(|v| v.as_str()).unwrap_or("?");
                    let friendly = s
                        .get("attributes")
                        .and_then(|a| a.get("friendly_name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(entity_id);
                    format!("• {entity_id} [{state}] — {friendly}")
                })
                .collect();

            let total = states.len();
            let shown = lines.len();
            let mut out = lines.join("\n");
            if total > shown {
                out.push_str(&format!("\n... y {} dispositivos más.", total - shown));
            }
            return Ok(out);
        }

        // Modo servicio: "domain.service ::: entity_id"
        if let Some((service_full, entity_id)) = input.split_once(":::") {
            let service_full = service_full.trim();
            let entity_id = entity_id.trim();

            let (domain, service) = service_full.split_once('.').ok_or_else(|| {
                format!(
                    "Formato de servicio inválido «{service_full}». Usa domain.service (ej: light.turn_on)"
                )
            })?;

            let url = format!("{base}/api/services/{domain}/{service}");
            let body = serde_json::json!({ "entity_id": entity_id });

            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("Error llamando al servicio HA: {e}"))?;

            let status = resp.status();
            if status.is_success() {
                Ok(format!(
                    "Servicio {domain}.{service} ejecutado sobre {entity_id}."
                ))
            } else {
                let text = resp.text().await.unwrap_or_default();
                Err(format!(
                    "Home Assistant devolvió {status} al llamar {domain}.{service}: {text}"
                ))
            }
        } else {
            Err(
                "Entrada no reconocida. Usa «estados» para listar dispositivos, \
                o «domain.service ::: entity_id» para ejecutar un servicio."
                    .to_string(),
            )
        }
    }

    fn needs_confirm(&self, input: &str) -> Option<String> {
        let input = input.trim();
        // Las acciones de escritura (cualquier servicio) piden confirmación
        if input.contains(":::") {
            if let Some((service_full, entity_id)) = input.split_once(":::") {
                return Some(format!(
                    "Ejecutar servicio «{}» sobre «{}» en Home Assistant.",
                    service_full.trim(),
                    entity_id.trim()
                ));
            }
        }
        None
    }
}
