//! DiscordTool — enviar mensajes y leer un canal de Discord desde AION.

use aion_orchestrator::{Tool, ToolCategory};
use async_trait::async_trait;

/// Herramienta de integración con Discord.
///
/// Variables de entorno:
/// - `AION_DISCORD_WEBHOOK_URL`: Webhook URL para enviar mensajes (para modo "send").
/// - `AION_DISCORD_CHANNEL_ID`: ID del canal para leer mensajes (para modo "read").
/// - `AION_DISCORD_TOKEN`: Bot token de Discord (para modo "read").
///
/// Modos de entrada:
/// - `"send ::: mensaje"` → envía el mensaje al webhook configurado (pide confirmación).
/// - `"read"` → obtiene los últimos 10 mensajes del canal configurado.
/// - `"status"` → muestra qué variables de entorno están configuradas.
pub struct DiscordTool;

impl DiscordTool {
    fn webhook_url() -> Option<String> {
        std::env::var("AION_DISCORD_WEBHOOK_URL").ok()
    }

    fn channel_id() -> Option<String> {
        std::env::var("AION_DISCORD_CHANNEL_ID").ok()
    }

    fn bot_token() -> Option<String> {
        std::env::var("AION_DISCORD_TOKEN").ok()
    }
}

#[async_trait]
impl Tool for DiscordTool {
    fn name(&self) -> &str {
        "discord"
    }

    fn description(&self) -> &str {
        "Integración con Discord. \
        Modos: «send ::: mensaje» para enviar al webhook (requiere AION_DISCORD_WEBHOOK_URL), \
        «read» para leer últimos 10 mensajes del canal (requiere AION_DISCORD_CHANNEL_ID + AION_DISCORD_TOKEN), \
        «status» para ver qué está configurado."
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::External
    }

    fn needs_confirm(&self, input: &str) -> Option<String> {
        let input = input.trim();
        if let Some(rest) = input.strip_prefix("send :::") {
            let msg = rest.trim();
            return Some(format!("Enviar a Discord: «{msg}»"));
        }
        // Comprobación menos estricta por si hay variaciones de espaciado
        if input.starts_with("send") && input.contains(":::") {
            if let Some((_, msg)) = input.split_once(":::") {
                return Some(format!("Enviar a Discord: «{}»", msg.trim()));
            }
        }
        None
    }

    async fn run(&self, input: &str) -> Result<String, String> {
        let input = input.trim();

        // Modo "status"
        if input.eq_ignore_ascii_case("status") {
            let webhook = if Self::webhook_url().is_some() {
                "AION_DISCORD_WEBHOOK_URL: configurado"
            } else {
                "AION_DISCORD_WEBHOOK_URL: NO configurado"
            };
            let channel = if Self::channel_id().is_some() {
                "AION_DISCORD_CHANNEL_ID: configurado"
            } else {
                "AION_DISCORD_CHANNEL_ID: NO configurado"
            };
            let token = if Self::bot_token().is_some() {
                "AION_DISCORD_TOKEN: configurado"
            } else {
                "AION_DISCORD_TOKEN: NO configurado"
            };
            return Ok(format!("{webhook}\n{channel}\n{token}"));
        }

        // Modo "read"
        if input.eq_ignore_ascii_case("read") {
            let channel_id = Self::channel_id().ok_or_else(|| {
                "AION_DISCORD_CHANNEL_ID no configurado. Configura el ID del canal Discord."
                    .to_string()
            })?;
            let token = Self::bot_token().ok_or_else(|| {
                "AION_DISCORD_TOKEN no configurado. Configura el bot token de Discord.".to_string()
            })?;

            let client = reqwest::Client::new();
            let url =
                format!("https://discord.com/api/v10/channels/{channel_id}/messages?limit=10");

            let resp = client
                .get(&url)
                .header("Authorization", format!("Bot {token}"))
                .header("Content-Type", "application/json")
                .send()
                .await
                .map_err(|e| format!("Error conectando a Discord API: {e}"))?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Discord API respondió con {status}: {body}"));
            }

            let messages: Vec<serde_json::Value> = resp
                .json()
                .await
                .map_err(|e| format!("Respuesta de Discord inválida: {e}"))?;

            if messages.is_empty() {
                return Ok("No hay mensajes recientes en el canal.".to_string());
            }

            let lines: Vec<String> = messages
                .iter()
                .map(|m| {
                    let author = m
                        .get("author")
                        .and_then(|a| a.get("username"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("?");
                    let content = m
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("[sin texto]");
                    let timestamp = m.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
                    format!("• [{timestamp}] {author}: {content}")
                })
                .collect();

            return Ok(format!(
                "Últimos {} mensajes en el canal:\n{}",
                lines.len(),
                lines.join("\n")
            ));
        }

        // Modo "send ::: mensaje"
        if input.contains(":::") {
            if let Some((prefix, message)) = input.split_once(":::") {
                if prefix.trim().eq_ignore_ascii_case("send") {
                    let message = message.trim();
                    if message.is_empty() {
                        return Err("El mensaje a enviar no puede estar vacío.".to_string());
                    }

                    let webhook_url = Self::webhook_url().ok_or_else(|| {
                        "AION_DISCORD_WEBHOOK_URL no configurado. Configura el webhook de Discord.".to_string()
                    })?;

                    let client = reqwest::Client::new();
                    let body = serde_json::json!({ "content": message });

                    let resp = client
                        .post(&webhook_url)
                        .json(&body)
                        .send()
                        .await
                        .map_err(|e| format!("Error enviando a Discord: {e}"))?;

                    let status = resp.status();
                    // Discord devuelve 204 No Content en éxito para webhooks
                    if status.is_success() || status.as_u16() == 204 {
                        return Ok(format!("Mensaje enviado a Discord: «{message}»"));
                    } else {
                        let body_text = resp.text().await.unwrap_or_default();
                        return Err(format!(
                            "Discord rechazó el mensaje con {status}: {body_text}"
                        ));
                    }
                }
            }
        }

        Err("Modo no reconocido. Usa «send ::: mensaje», «read» o «status».".to_string())
    }
}
