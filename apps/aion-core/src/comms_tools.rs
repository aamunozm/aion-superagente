//! Herramientas de COMUNICACIÓN reales (macOS): Calendario, Contactos, Mensajes
//! (iMessage/SMS) y WhatsApp Web. Cada herramienta pasa por `comms::CommsPolicy`
//! (filtro de CON QUIÉN y QUÉ CANAL) antes de actuar; los envíos, además, devuelven
//! `needs_confirm` para que el bucle del agente pida el OK humano (HITL).
//!
//! Honestidad sobre los límites (no se finge nada):
//!   · Calendario/Contactos/envío de Mensajes: AppleScript nativo. Funciona; pide permiso
//!     de Automatización la primera vez.
//!   · Lectura de Mensajes: lee la base local `chat.db` con `sqlite3` (requiere Acceso a
//!     disco completo). Si falta el permiso, lo dice claramente, no inventa mensajes.
//!   · WhatsApp: se opera vía el navegador agéntico sobre web.whatsapp.com (requiere haber
//!     escaneado el QR una vez). NO hay llamadas de audio ni transcripción de notas de voz:
//!     eso no es automatizable hoy y AION no lo simula.

use crate::comms;
use aion_browser::{BrowserDriver, WebClient};
use aion_orchestrator::Tool;
use async_trait::async_trait;
use std::sync::Arc;

/// Ejecuta un AppleScript y devuelve stdout (o un error legible con la pista de permisos).
#[cfg(target_os = "macos")]
fn run_osa(script: &str) -> Result<String, String> {
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("no pude ejecutar osascript: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!(
            "AppleScript falló (¿falta permiso de Automatización en Ajustes del Sistema → \
             Privacidad y seguridad → Automatización → AION?): {err}"
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn run_osa(_script: &str) -> Result<String, String> {
    Err("las herramientas de comunicación nativas solo están disponibles en macOS".into())
}

/// Escapa una cadena para incrustarla entre comillas dobles en AppleScript.
fn osa_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ── Calendario ──────────────────────────────────────────────────────────────

/// Lista los próximos eventos del Calendario del Mac.
pub struct CalendarListTool;
impl CalendarListTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for CalendarListTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for CalendarListTool {
    fn name(&self) -> &str {
        "calendar_list"
    }
    fn description(&self) -> &str {
        "Mira la AGENDA del Mac: lista los próximos eventos del Calendario. Úsalo para «¿qué \
         tengo esta semana?», «¿qué hay mañana?». Entrada: número de días a mirar (por defecto 7)."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        comms::load().authorize("calendar", "", false)?;
        let days: i64 = input.trim().parse().unwrap_or(7).clamp(1, 60);
        let script = format!(
            "set out to \"\"\n\
             set t0 to current date\n\
             set t1 to t0 + ({days} * days)\n\
             tell application \"Calendar\"\n\
             repeat with c in calendars\n\
             try\n\
             set evs to (every event of c whose start date ≥ t0 and start date ≤ t1)\n\
             repeat with e in evs\n\
             set out to out & (summary of e) & \" — \" & (start date of e as string) & linefeed\n\
             end repeat\n\
             end try\n\
             end repeat\n\
             end tell\n\
             return out"
        );
        let res = run_osa(&script)?;
        if res.trim().is_empty() {
            Ok(format!("no hay eventos en los próximos {days} días."))
        } else {
            Ok(format!("Eventos en los próximos {days} días:\n{res}"))
        }
    }
}

/// Crea un evento en el Calendario. Pide confirmación humana (escribe en tu agenda).
pub struct CalendarCreateTool;
impl CalendarCreateTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for CalendarCreateTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for CalendarCreateTool {
    fn name(&self) -> &str {
        "calendar_create"
    }
    fn description(&self) -> &str {
        "Crea un EVENTO en el Calendario del Mac. Entrada: «Título ::: AAAA-MM-DD HH:MM ::: \
         duración_min». Ej: «Dentista ::: 2026-06-25 10:30 ::: 45». La duración es opcional \
         (por defecto 60 min)."
    }
    fn needs_confirm(&self, input: &str) -> Option<String> {
        Some(format!(
            "crear un evento en tu calendario: {}",
            input.trim()
        ))
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        comms::load().authorize("calendar", "", true)?;
        let parts: Vec<&str> = input.split(":::").map(|s| s.trim()).collect();
        if parts.len() < 2 {
            return Err("formato: «Título ::: AAAA-MM-DD HH:MM ::: duración_min»".into());
        }
        let title = parts[0];
        let when = parts[1];
        let dur: i64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
        // Parseo robusto e independiente del locale: extraemos los componentes y los
        // fijamos uno a uno sobre `current date` en AppleScript.
        let (date_part, time_part) = when.split_once(' ').unwrap_or((when, "09:00"));
        let dp: Vec<&str> = date_part.split('-').collect();
        let tp: Vec<&str> = time_part.split(':').collect();
        if dp.len() != 3 {
            return Err("la fecha debe ser AAAA-MM-DD".into());
        }
        let (y, mo, d) = (dp[0], dp[1], dp[2]);
        let hh = tp.first().copied().unwrap_or("9");
        let mm = tp.get(1).copied().unwrap_or("0");
        for (v, name) in [
            (y, "año"),
            (mo, "mes"),
            (d, "día"),
            (hh, "hora"),
            (mm, "min"),
        ] {
            if v.parse::<i64>().is_err() {
                return Err(format!("{name} inválido en la fecha/hora"));
            }
        }
        let script = format!(
            "set d to current date\n\
             set year of d to {y}\n\
             set month of d to {mo}\n\
             set day of d to {d}\n\
             set hours of d to {hh}\n\
             set minutes of d to {mm}\n\
             set seconds of d to 0\n\
             set d2 to d + ({dur} * minutes)\n\
             tell application \"Calendar\"\n\
             tell calendar 1\n\
             make new event with properties {{summary:\"{}\", start date:d, end date:d2}}\n\
             end tell\n\
             end tell\n\
             return \"ok\"",
            osa_escape(title)
        );
        run_osa(&script)?;
        Ok(format!(
            "evento «{title}» creado el {when} ({dur} min) en tu Calendario."
        ))
    }
}

// ── Contactos ─────────────────────────────────────────────────────────────

/// Busca en la app Contactos del Mac (resuelve nombre → teléfono/email).
pub struct ContactsSearchTool;
impl ContactsSearchTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for ContactsSearchTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for ContactsSearchTool {
    fn name(&self) -> &str {
        "contacts_search"
    }
    fn description(&self) -> &str {
        "Busca una persona en tus Contactos del Mac y devuelve sus teléfonos/emails. Útil para \
         resolver «escríbele a Mamá» → su número. Entrada: el nombre o parte del nombre."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        comms::load().authorize("contacts", "", false)?;
        let q = input.trim();
        if q.is_empty() {
            return Err("dime a quién buscar (un nombre).".into());
        }
        let script = format!(
            "set out to \"\"\n\
             tell application \"Contacts\"\n\
             set ppl to (every person whose name contains \"{}\")\n\
             repeat with p in ppl\n\
             set out to out & (name of p)\n\
             repeat with ph in phones of p\n\
             set out to out & \" | tel: \" & (value of ph)\n\
             end repeat\n\
             repeat with em in emails of p\n\
             set out to out & \" | email: \" & (value of em)\n\
             end repeat\n\
             set out to out & linefeed\n\
             end repeat\n\
             end tell\n\
             return out",
            osa_escape(q)
        );
        let res = run_osa(&script)?;
        if res.trim().is_empty() {
            Ok(format!("no encontré contactos que coincidan con «{q}»."))
        } else {
            Ok(format!("Contactos para «{q}»:\n{res}"))
        }
    }
}

// ── Mensajes (iMessage / SMS) ────────────────────────────────────────────────

/// Envía un iMessage/SMS. Filtra por contacto permitido + confirmación humana.
pub struct MessagesSendTool;
impl MessagesSendTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for MessagesSendTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for MessagesSendTool {
    fn name(&self) -> &str {
        "messages_send"
    }
    fn description(&self) -> &str {
        "Envía un mensaje por la app Mensajes (iMessage/SMS) del Mac. Entrada: «destinatario ::: \
         texto». El destinatario es un teléfono, email o el nombre de un contacto permitido. TÚ \
         redactas el texto. Pide confirmación antes de enviar."
    }
    fn needs_confirm(&self, input: &str) -> Option<String> {
        let (to, msg) = input.split_once(":::").unwrap_or((input, ""));
        Some(format!(
            "enviar un mensaje a {} por Mensajes: «{}»",
            to.trim(),
            msg.trim()
        ))
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let (to, msg) = match input.split_once(":::") {
            Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
            None => return Err("formato: «destinatario ::: texto»".into()),
        };
        if msg.is_empty() {
            return Err("falta el texto del mensaje.".into());
        }
        let pol = comms::load();
        let who = pol.authorize("imessage", &to, true)?;
        // Si el destinatario era un nombre de contacto, resuelve su handle desde la política.
        let handle = pol
            .find(&to)
            .map(|c| {
                if c.handle.is_empty() {
                    to.clone()
                } else {
                    c.handle.clone()
                }
            })
            .unwrap_or_else(|| to.clone());
        let script = format!(
            "tell application \"Messages\"\n\
             send \"{}\" to buddy \"{}\" of (1st service whose service type = iMessage)\n\
             end tell\n\
             return \"ok\"",
            osa_escape(&msg),
            osa_escape(&handle)
        );
        run_osa(&script)?;
        Ok(format!("mensaje enviado a {who} ({handle})."))
    }
}

/// Lee los mensajes recientes de la base local del Mac (chat.db). Requiere Acceso a disco completo.
pub struct MessagesReadTool;
impl MessagesReadTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for MessagesReadTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for MessagesReadTool {
    fn name(&self) -> &str {
        "messages_read"
    }
    fn description(&self) -> &str {
        "Lee los MENSAJES recientes (iMessage/SMS) del Mac. Entrada opcional: un teléfono/email \
         para filtrar por esa conversación; vacío = los más recientes. (Requiere Acceso a disco \
         completo para AION; si falta, lo aviso, no invento mensajes.)"
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let target = input.trim();
        let pol = comms::load();
        if target.is_empty() {
            pol.authorize("imessage", "", false)?;
        } else {
            pol.authorize("imessage", target, false)?;
        }
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var("HOME").map_err(|_| "no encuentro HOME".to_string())?;
            let db = format!("{home}/Library/Messages/chat.db");
            if !std::path::Path::new(&db).exists() {
                return Err("no encuentro la base de Mensajes (chat.db).".into());
            }
            // Fechas Apple: nanosegundos desde 2001-01-01. Convertimos a hora local.
            let mut where_clause = "message.text IS NOT NULL".to_string();
            if !target.is_empty() {
                where_clause.push_str(&format!(
                    " AND handle.id LIKE '%{}%'",
                    target.replace('\'', "")
                ));
            }
            let sql = format!(
                "SELECT datetime(message.date/1000000000 + 978307200,'unixepoch','localtime') AS d, \
                 CASE message.is_from_me WHEN 1 THEN 'yo' ELSE COALESCE(handle.id,'?') END AS quien, \
                 substr(message.text,1,300) \
                 FROM message LEFT JOIN handle ON message.handle_id = handle.ROWID \
                 WHERE {where_clause} ORDER BY message.date DESC LIMIT 15;"
            );
            let out = std::process::Command::new("sqlite3")
                .arg("-separator")
                .arg(" | ")
                .arg(&db)
                .arg(&sql)
                .output()
                .map_err(|e| format!("no pude consultar chat.db: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                if err.contains("authorization denied") || err.contains("unable to open") {
                    return Err(
                        "no tengo Acceso a disco completo para leer tus mensajes. Actívalo en \
                         Ajustes del Sistema → Privacidad y seguridad → Acceso a disco completo → AION."
                            .into(),
                    );
                }
                return Err(format!("no pude leer los mensajes: {}", err.trim()));
            }
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if text.is_empty() {
                return Ok("no hay mensajes recientes que coincidan.".into());
            }
            // Orden cronológico para leerlo natural (la consulta vino DESC).
            let mut lines: Vec<&str> = text.lines().collect();
            lines.reverse();
            Ok(format!("Mensajes recientes:\n{}", lines.join("\n")))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("la lectura de Mensajes solo está disponible en macOS.".into())
        }
    }
}

// ── WhatsApp (vía navegador agéntico) ─────────────────────────────────────────

/// Abre WhatsApp Web (en el navegador agéntico) en la conversación de un contacto,
/// opcionalmente con un mensaje preescrito. Luego el agente usa browser_read/click/type.
pub struct WhatsAppOpenTool {
    driver: Arc<dyn BrowserDriver>,
    #[allow(dead_code)]
    web: Arc<WebClient>,
}
impl WhatsAppOpenTool {
    pub fn new(driver: Arc<dyn BrowserDriver>, web: Arc<WebClient>) -> Self {
        Self { driver, web }
    }
}
#[async_trait]
impl Tool for WhatsAppOpenTool {
    fn name(&self) -> &str {
        "whatsapp_open"
    }
    fn description(&self) -> &str {
        "Abre WhatsApp Web en el navegador para una conversación. Entrada: «contacto» para abrir \
         su chat, o «contacto ::: mensaje» para abrirlo con el texto preparado (luego confirmas el \
         envío y pulsas enviar con browser_click). Requiere haber iniciado sesión (QR) una vez."
    }
    fn needs_confirm(&self, input: &str) -> Option<String> {
        if let Some((to, msg)) = input.split_once(":::") {
            Some(format!(
                "preparar un WhatsApp para {} con el texto: «{}»",
                to.trim(),
                msg.trim()
            ))
        } else {
            None // solo abrir/leer no requiere confirmación
        }
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let (to, msg) = match input.split_once(":::") {
            Some((a, b)) => (a.trim().to_string(), Some(b.trim().to_string())),
            None => (input.trim().to_string(), None),
        };
        if to.is_empty() {
            return Err("dime con qué contacto abro WhatsApp.".into());
        }
        let pol = comms::load();
        let who = pol.authorize("whatsapp", &to, msg.is_some())?;
        // Resuelve el teléfono (dígitos) del contacto si lo tenemos guardado.
        let phone: String = pol
            .find(&to)
            .map(|c| c.handle.clone())
            .unwrap_or_else(|| to.clone())
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        let url = match (&msg, phone.is_empty()) {
            (Some(m), false) => format!(
                "https://web.whatsapp.com/send?phone={phone}&text={}",
                urlencoding(m)
            ),
            (None, false) => format!("https://web.whatsapp.com/send?phone={phone}"),
            _ => "https://web.whatsapp.com/".to_string(),
        };
        let snap = tokio::time::timeout(std::time::Duration::from_secs(25), async {
            self.driver.open(&url).await.map_err(|e| e.to_string())?;
            self.driver.snapshot().await.map_err(|e| e.to_string())
        })
        .await;
        match snap {
            Ok(Ok(s)) => Ok(format!(
                "WhatsApp Web abierto para {who}. Usa browser_read para ver el chat y \
                 browser_click/browser_type para responder.\n[{}] {}",
                s.view.title, s.view.url
            )),
            Ok(Err(e)) => Err(format!(
                "no pude abrir WhatsApp Web ({e}). ¿Has iniciado sesión con el QR?"
            )),
            Err(_) => Err("WhatsApp Web no respondió en 25s.".into()),
        }
    }
}

/// Codificación de URL mínima para el texto del mensaje (sin dependencias extra).
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding_escapes() {
        assert_eq!(urlencoding("hola mundo"), "hola%20mundo");
        assert_eq!(urlencoding("á?"), "%C3%A1%3F");
    }

    #[test]
    fn osa_escape_quotes() {
        assert_eq!(osa_escape("di \"hola\""), "di \\\"hola\\\"");
    }
}
