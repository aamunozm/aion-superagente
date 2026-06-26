//! CalendarListTool / CalendarCreateTool — integración con el Calendario de macOS via osascript.

use aion_orchestrator::{Tool, ToolCategory};
use async_trait::async_trait;
use tokio::process::Command;

// ─── CalendarListTool ─────────────────────────────────────────────────────────

/// Lista eventos del Calendario de macOS para los próximos N días.
/// Entrada: número de días (por defecto 7).
pub struct CalendarListTool;

#[async_trait]
impl Tool for CalendarListTool {
    fn name(&self) -> &str {
        "calendar_list"
    }

    fn description(&self) -> &str {
        "Lista los eventos del Calendario de macOS. \
        Entrada: número de días a consultar (por defecto 7). \
        Devuelve título, fecha/hora de inicio y fin de cada evento."
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::External
    }

    async fn run(&self, input: &str) -> Result<String, String> {
        let days: u32 = input.trim().parse().unwrap_or(7).clamp(1, 90);

        // AppleScript que obtiene eventos del Calendario en los próximos N días
        let script = format!(
            r#"
set now to current date
set endDate to now + ({days} * days)
set output to ""
tell application "Calendar"
    set allCalendars to every calendar
    repeat with cal in allCalendars
        set calName to name of cal
        set evts to (every event of cal whose start date >= now and start date <= endDate)
        repeat with evt in evts
            set evtTitle to summary of evt
            set evtStart to start date of evt
            set evtEnd to end date of evt
            set output to output & calName & "|" & evtTitle & "|" & (evtStart as string) & "|" & (evtEnd as string) & linefeed
        end repeat
    end repeat
end tell
return output
"#,
            days = days
        );

        let out = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
            .map_err(|e| format!("Error ejecutando osascript: {e}"))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("osascript falló: {stderr}"));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stdout = stdout.trim();

        if stdout.is_empty() {
            return Ok(format!(
                "No hay eventos en el Calendario para los próximos {days} días."
            ));
        }

        // Parsear líneas: "Calendario|Título|Inicio|Fin"
        let mut events: Vec<String> = Vec::new();
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() == 4 {
                events.push(format!(
                    "• [{}] {} — {} → {}",
                    parts[0], parts[1], parts[2], parts[3]
                ));
            } else if !line.trim().is_empty() {
                events.push(format!("• {line}"));
            }
        }

        if events.is_empty() {
            Ok(format!(
                "No hay eventos en el Calendario para los próximos {days} días."
            ))
        } else {
            Ok(format!(
                "Eventos para los próximos {days} días ({} total):\n{}",
                events.len(),
                events.join("\n")
            ))
        }
    }
}

// ─── CalendarCreateTool ───────────────────────────────────────────────────────

/// Crea un evento en el Calendario de macOS.
/// Entrada: "Título ::: YYYY-MM-DD HH:MM [::: duración_minutos]"
/// La duración por defecto es 60 minutos.
pub struct CalendarCreateTool;

#[async_trait]
impl Tool for CalendarCreateTool {
    fn name(&self) -> &str {
        "calendar_create"
    }

    fn description(&self) -> &str {
        "Crea un evento en el Calendario de macOS. \
        Entrada: «Título ::: YYYY-MM-DD HH:MM» o \
        «Título ::: YYYY-MM-DD HH:MM ::: duración_minutos». \
        Duración por defecto: 60 minutos. \
        Ejemplo: «Reunión equipo ::: 2026-07-01 10:00 ::: 90»"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::External
    }

    fn needs_confirm(&self, input: &str) -> Option<String> {
        let parts: Vec<&str> = input.splitn(3, ":::").collect();
        let title = parts.first().map(|s| s.trim()).unwrap_or("(sin título)");
        let when = parts.get(1).map(|s| s.trim()).unwrap_or("(sin fecha)");
        let duration = parts
            .get(2)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("60");
        Some(format!(
            "Crear evento «{title}» el {when} con duración {duration} minutos en el Calendario de macOS."
        ))
    }

    async fn run(&self, input: &str) -> Result<String, String> {
        let parts: Vec<&str> = input.splitn(3, ":::").collect();
        if parts.len() < 2 {
            return Err(
                "Formato incorrecto. Usa: «Título ::: YYYY-MM-DD HH:MM [::: duración_minutos]»"
                    .to_string(),
            );
        }

        let title = parts[0].trim();
        let datetime_str = parts[1].trim();
        let duration_minutes: u32 = parts
            .get(2)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(60);

        // Parsear "YYYY-MM-DD HH:MM"
        let dt_parts: Vec<&str> = datetime_str.splitn(2, ' ').collect();
        if dt_parts.len() < 2 {
            return Err(format!(
                "Formato de fecha/hora inválido «{datetime_str}». Usa YYYY-MM-DD HH:MM"
            ));
        }
        let date_part = dt_parts[0]; // YYYY-MM-DD
        let time_part = dt_parts[1]; // HH:MM

        let date_components: Vec<&str> = date_part.split('-').collect();
        let time_components: Vec<&str> = time_part.split(':').collect();

        if date_components.len() < 3 || time_components.len() < 2 {
            return Err(format!("Fecha u hora mal formateada: «{datetime_str}»"));
        }

        let year = date_components[0];
        let month = date_components[1];
        let day = date_components[2];
        let hour = time_components[0];
        let minute = time_components[1];

        // Escapar el título para AppleScript
        let safe_title = title.replace('\\', "\\\\").replace('"', "\\\"");

        let script = format!(
            r#"
tell application "Calendar"
    set startDate to date "{month}/{day}/{year} {hour}:{minute}:00"
    set endDate to startDate + ({duration_minutes} * minutes)
    set newEvent to make new event at end of events of first calendar with properties {{summary:"{safe_title}", start date:startDate, end date:endDate}}
    return "OK:" & (summary of newEvent) & "|" & (start date of newEvent as string)
end tell
"#,
            month = month,
            day = day,
            year = year,
            hour = hour,
            minute = minute,
            duration_minutes = duration_minutes,
            safe_title = safe_title
        );

        let out = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
            .map_err(|e| format!("Error ejecutando osascript: {e}"))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("No se pudo crear el evento: {stderr}"));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stdout = stdout.trim();

        if let Some(info) = stdout.strip_prefix("OK:") {
            Ok(format!("Evento creado: {info}"))
        } else {
            Ok(format!("Evento creado. Respuesta: {stdout}"))
        }
    }
}
