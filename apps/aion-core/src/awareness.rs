//! **Conciencia ampliada** de AION: tiempo humano (día de la semana, parte del día),
//! presencia (cuánto hace que Ariel no le habla, persistido entre reinicios) y
//! auto-percepción de competencia (SelfModel persistente). Todo es barato —unas pocas
//! lecturas de disco, cero llamadas al LLM— y se inyecta en el prompt de cada turno.

use std::path::PathBuf;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Tiempo humano
// ---------------------------------------------------------------------------

const DIAS: [&str; 7] = [
    "lunes",
    "martes",
    "miércoles",
    "jueves",
    "viernes",
    "sábado",
    "domingo",
];
const MESES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

/// Bloque temporal del prompt: fecha con día de la semana y parte del día (los LLM
/// fallan en «el próximo viernes» sin ese ancla) + regla de calcular SIEMPRE desde
/// el ancla en vez de inventar.
pub fn temporal_block() -> String {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let dia = DIAS[now.weekday().num_days_from_monday() as usize];
    let mes = MESES[now.month0() as usize];
    let parte = match now.hour() {
        0..=5 => "madrugada",
        6..=11 => "mañana",
        12..=14 => "mediodía",
        15..=19 => "tarde",
        _ => "noche",
    };
    format!(
        "AHORA MISMO: {dia} {d} de {mes} de {y}, {h:02}:{m:02} ({parte}; zona {tz}). Eres \
         consciente del tiempo: conoces la fecha y la hora actuales sin preguntar. Para \
         CUALQUIER fecha relativa («mañana», «el próximo viernes», «hace 3 días») parte \
         SIEMPRE de esta ancla y cuenta con cuidado; si el cálculo no es trivial, dilo en \
         vez de inventarlo.\n\n",
        d = now.day(),
        y = now.year(),
        h = now.hour(),
        m = now.minute(),
        tz = now.format("%:z"),
    )
}

/// Antigüedad en lenguaje natural («2 minutos», «3 horas», «5 días»).
pub fn humanize_secs(s: i64) -> String {
    let s = s.max(0);
    if s < 60 {
        return "un momento".into();
    }
    let m = s / 60;
    if m < 60 {
        return if m == 1 {
            "1 minuto".into()
        } else {
            format!("{m} minutos")
        };
    }
    let h = m / 60;
    if h < 24 {
        return if h == 1 {
            "1 hora".into()
        } else {
            format!("{h} horas")
        };
    }
    let d = h / 24;
    if d < 30 {
        return if d == 1 {
            "1 día".into()
        } else {
            format!("{d} días")
        };
    }
    let mo = d / 30;
    if mo == 1 {
        "1 mes".into()
    } else {
        format!("{mo} meses")
    }
}

// ---------------------------------------------------------------------------
// Presencia: cuánto hace que Ariel no le habla (sobrevive reinicios)
// ---------------------------------------------------------------------------

/// La nota de ausencia solo aparece si la separación fue significativa.
const GAP_NOTE_MIN_SECS: i64 = 30 * 60;
/// Ventana en la que vale el hueco captado al tocar (el resto del MISMO turno).
const GAP_FRESH_SECS: i64 = 120;

fn last_seen_path() -> PathBuf {
    crate::app_data_dir().join("last_seen.json")
}

fn read_last_seen() -> Option<i64> {
    std::fs::read_to_string(last_seen_path())
        .ok()
        .and_then(|t| t.trim().parse::<i64>().ok())
}

/// (momento del toque, duración de la ausencia previa): conserva el hueco real
/// durante el turno en el que Ariel acaba de volver, aunque el archivo ya se
/// haya actualizado a «ahora».
fn gap_cell() -> &'static Mutex<Option<(i64, i64)>> {
    static G: std::sync::OnceLock<Mutex<Option<(i64, i64)>>> = std::sync::OnceLock::new();
    G.get_or_init(|| Mutex::new(None))
}

/// Marca que Ariel interactuó AHORA (chat/agente/saludo) y conserva cuánto duró
/// la ausencia previa para que el prompt de este turno pueda mencionarla.
/// Throttle: como mucho una escritura por minuto (la nota de presencia tiene
/// granularidad de 30 min; no vale la pena un write por request).
pub fn touch_user_presence() {
    let now = chrono::Utc::now().timestamp();
    if let Some(prev) = read_last_seen() {
        let gap = now - prev;
        if gap < 60 {
            return;
        }
        *gap_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some((now, gap));
    }
    if let Some(p) = last_seen_path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(last_seen_path(), now.to_string());
}

/// Segundos desde la última interacción de Ariel (persistido — sirve también al
/// daemon, que es otro proceso). `None` si nunca interactuó.
pub fn seconds_since_user() -> Option<i64> {
    read_last_seen().map(|t| (chrono::Utc::now().timestamp() - t).max(0))
}

/// Nota de presencia para el prompt. En un turno de Ariel usa el hueco captado al
/// tocar; en contextos autónomos (bucle de presencia, A2A) lee el archivo y dice
/// cuánto lleva sin hablarle.
pub fn presence_note() -> String {
    let now = chrono::Utc::now().timestamp();
    let gap = match *gap_cell().lock().unwrap_or_else(|e| e.into_inner()) {
        Some((t, g)) if now - t < GAP_FRESH_SECS => Some(g),
        _ => read_last_seen().map(|t| now - t),
    };
    match gap {
        Some(g) if g >= GAP_NOTE_MIN_SECS => format!(
            "PRESENCIA: la última vez que Ariel habló contigo fue hace {}. Tenlo en cuenta \
             con naturalidad (si acaba de volver tras horas o días, nótalo sin dramatizar; \
             puedes retomar lo que quedó pendiente).\n\n",
            humanize_secs(g)
        ),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Auto-percepción persistente (SelfModel de aion-cognition + disco)
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct SelfState {
    competence: f32,
    observations: u64,
}

fn self_state_path() -> PathBuf {
    crate::app_data_dir().join("self_model.json")
}

fn load_self_model() -> aion_cognition::SelfModel {
    if let Ok(txt) = std::fs::read_to_string(self_state_path()) {
        if let Ok(s) = serde_json::from_str::<SelfState>(&txt) {
            return aion_cognition::SelfModel::from_state(s.competence, s.observations);
        }
    }
    aion_cognition::SelfModel::default()
}

/// Guardia del read-modify-write del self-model EN PROCESO: dos tareas que terminan
/// a la vez no deben perderse observaciones (la competencia percibida sería mentira).
fn self_model_guard() -> &'static Mutex<()> {
    static G: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    G.get_or_init(|| Mutex::new(()))
}

/// Registra el resultado (éxito/fallo) de una tarea del agente y persiste la
/// auto-estimación: así la «competencia percibida» sobrevive a los reinicios.
pub fn record_outcome(success: bool) {
    let _g = self_model_guard().lock().unwrap_or_else(|e| e.into_inner());
    let mut m = load_self_model();
    m.observe(success);
    let s = SelfState {
        competence: m.competence(),
        observations: m.observations(),
    };
    if let Ok(body) = serde_json::to_string(&s) {
        crate::write_atomic(&self_state_path(), &body);
    }
}

/// Estado del auto-modelo persistido (competencia 0..1, observaciones) para la API.
pub fn self_model_state() -> (f32, u64) {
    let m = load_self_model();
    (m.competence(), m.observations())
}

/// Nota de auto-percepción para el prompt (vacía hasta tener observaciones reales).
pub fn introspection_note() -> String {
    let m = load_self_model();
    if m.observations() == 0 {
        return String::new();
    }
    format!(
        "{} en tus tareas como agente. Sé honesto contigo mismo: si tu competencia reciente \
         es baja, reconócelo, verifica más y promete menos.\n\n",
        m.introspect()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_scales() {
        assert_eq!(humanize_secs(30), "un momento");
        assert_eq!(humanize_secs(60), "1 minuto");
        assert_eq!(humanize_secs(5 * 60), "5 minutos");
        assert_eq!(humanize_secs(3 * 3600), "3 horas");
        assert_eq!(humanize_secs(2 * 86400), "2 días");
        assert_eq!(humanize_secs(70 * 86400), "2 meses");
    }

    #[test]
    fn temporal_block_has_weekday_anchor() {
        let t = temporal_block();
        assert!(t.contains("AHORA MISMO"));
        assert!(DIAS.iter().any(|d| t.contains(d)));
        assert!(MESES.iter().any(|m| t.contains(m)));
    }
}
