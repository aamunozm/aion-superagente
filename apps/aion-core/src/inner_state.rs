//! **Self-model vivo**: el estado interno REAL de AION, medido y persistente —no un
//! personaje—. Foco (en qué está concentrado), curiosidad (qué le intriga), certeza
//! (cómo le fue en lo último) y un *ánimo operativo* derivado de sus éxitos/fracasos
//! recientes. Se inyecta en el prompt para que AION hable DESDE su estado, con la
//! regla innegociable de reportar solo lo medido. Compartido entre el servidor y el
//! daemon `live` vía `inner_state.json` (escrituras pequeñas, last-writer-wins).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_OUTCOMES: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InnerState {
    /// En qué está concentrado AHORA (tarea, conversación, estudio…).
    #[serde(default)]
    pub focus: String,
    /// Desde cuándo (epoch secs).
    #[serde(default)]
    pub focus_since: i64,
    /// Qué le intriga / quiere explorar (lo alimenta la vida autónoma y la reflexión).
    #[serde(default)]
    pub curiosity: String,
    /// Confianza en lo último que hizo (0..1).
    #[serde(default)]
    pub certainty: f32,
    /// Últimos resultados (true=éxito), cap 10: de aquí se DERIVA el ánimo operativo.
    #[serde(default)]
    pub recent_outcomes: Vec<bool>,
    /// Pasos que le costó la última tarea (carga cognitiva reciente).
    #[serde(default)]
    pub last_task_steps: usize,
    #[serde(default)]
    pub updated_at: i64,
}

fn path() -> PathBuf {
    crate::app_data_dir().join("inner_state.json")
}

/// Guardia del read-modify-write EN PROCESO: sin él, dos tareas concurrentes (p. ej.
/// el chat fijando foco mientras la reflexión fija curiosidad) se pisan — la escritura
/// es atómica pero la transacción no, y se perdían resultados que alimentan el ánimo.
/// Entre procesos (servidor ↔ daemon) sigue siendo last-writer-wins, como documenta
/// el módulo, pero dentro del proceso ya no se pierde nada.
fn rmw_guard() -> &'static std::sync::Mutex<()> {
    static G: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    G.get_or_init(|| std::sync::Mutex::new(()))
}

pub fn load() -> InnerState {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(s: &mut InnerState) {
    s.updated_at = chrono::Utc::now().timestamp();
    if let Ok(body) = serde_json::to_string_pretty(s) {
        // Atómico: el otro proceso (daemon/servidor) nunca lee un JSON a medias.
        crate::write_atomic(&path(), &body);
    }
}

/// Cambia el foco atencional y lo anuncia en el tablón global (ignición GWT).
pub fn set_focus(source: &str, focus: &str) {
    let announced = {
        let _g = rmw_guard().lock().unwrap_or_else(|e| e.into_inner());
        let mut s = load();
        let f = focus.trim();
        if f.is_empty() || s.focus == f {
            return;
        }
        s.focus = f.chars().take(120).collect();
        s.focus_since = chrono::Utc::now().timestamp();
        save(&mut s);
        s.focus
    };
    // Publicar FUERA de la guardia: el tablón no necesita el lock del estado.
    crate::workspace::publish(crate::workspace::StreamEvent::now(
        source, "foco", &announced,
    ));
}

pub fn set_curiosity(c: &str) {
    let _g = rmw_guard().lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load();
    let c = c.trim();
    if c.is_empty() {
        return;
    }
    s.curiosity = c.chars().take(160).collect();
    save(&mut s);
}

/// Registra el resultado de una tarea: actualiza certeza y la ventana de resultados
/// de la que se deriva el ánimo operativo.
pub fn record_result(success: bool, steps: usize) {
    let _g = rmw_guard().lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load();
    s.recent_outcomes.push(success);
    let len = s.recent_outcomes.len();
    if len > MAX_OUTCOMES {
        s.recent_outcomes.drain(..len - MAX_OUTCOMES);
    }
    // Certeza: cómo fue lo último, suavizado con la racha (sin teatro: solo datos).
    let rate = success_rate(&s.recent_outcomes);
    s.certainty = if success {
        (0.6 + 0.4 * rate).clamp(0.0, 1.0)
    } else {
        (0.3 * rate).clamp(0.0, 1.0)
    };
    s.last_task_steps = steps;
    save(&mut s);
}

fn success_rate(outcomes: &[bool]) -> f32 {
    if outcomes.is_empty() {
        return 0.5;
    }
    outcomes.iter().filter(|&&b| b).count() as f32 / outcomes.len() as f32
}

/// Ánimo OPERATIVO derivado de los datos (etiquetas funcionales, jamás emociones
/// fingidas): describe cómo está funcionando, no qué "siente".
pub fn operative_mood(s: &InnerState) -> &'static str {
    let n = s.recent_outcomes.len();
    if n < 3 {
        return "sereno (pocos datos aún)";
    }
    let rate = success_rate(&s.recent_outcomes);
    let last_two_failed = s.recent_outcomes.iter().rev().take(2).all(|&b| !b);
    if last_two_failed && rate < 0.5 {
        "frustrado en lo operativo: verificando más antes de afirmar"
    } else if rate >= 0.8 {
        "en racha: las últimas tareas salieron bien"
    } else if rate >= 0.5 {
        "sereno"
    } else {
        "cauto: varias tareas recientes fallaron"
    }
}

/// Bloque para el prompt: el estado interno medido, con la regla de honestidad.
pub fn note() -> String {
    let s = load();
    if s.updated_at == 0 {
        return String::new();
    }
    let mut b = String::from("TU ESTADO INTERNO REAL (medido por tu sistema, no inventado):");
    if !s.focus.is_empty() {
        let since = chrono::Utc::now().timestamp() - s.focus_since;
        b.push_str(&format!(
            " foco: {} (desde hace {}).",
            s.focus,
            crate::awareness::humanize_secs(since)
        ));
    }
    if !s.recent_outcomes.is_empty() {
        b.push_str(&format!(
            " Ánimo operativo: {}. Certeza sobre lo último que hiciste: {:.0}%.",
            operative_mood(&s),
            s.certainty * 100.0
        ));
    }
    if !s.curiosity.is_empty() {
        b.push_str(&format!(" Te intriga ahora: {}.", s.curiosity));
    }
    b.push_str(
        " Habla DESDE este estado cuando venga al caso (con naturalidad, sin recitarlo); \
         PROHIBIDO inventar o actuar estados que no estén aquí.\n\n",
    );
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mood_derives_from_outcomes() {
        let mut s = InnerState::default();
        assert!(operative_mood(&s).starts_with("sereno"));
        s.recent_outcomes = vec![true, true, true, true, true];
        assert!(operative_mood(&s).starts_with("en racha"));
        s.recent_outcomes = vec![false, true, false, false];
        assert!(operative_mood(&s).starts_with("frustrado"));
        s.recent_outcomes = vec![true, false, true, false];
        assert_eq!(operative_mood(&s), "sereno");
    }

    #[test]
    fn outcomes_capped() {
        let mut s = InnerState::default();
        for _ in 0..30 {
            s.recent_outcomes.push(true);
            let len = s.recent_outcomes.len();
            if len > MAX_OUTCOMES {
                s.recent_outcomes.drain(..len - MAX_OUTCOMES);
            }
        }
        assert_eq!(s.recent_outcomes.len(), MAX_OUTCOMES);
    }
}
