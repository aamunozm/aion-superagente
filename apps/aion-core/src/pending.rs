//! **Deudas conversacionales**: preguntas/tareas de Ariel que quedaron SIN resolver
//! (negativa honesta o corrección suya). La vida autónoma las retoma más tarde con
//! herramientas reales y, cuando consigue la respuesta, vuelve a Ariel con ella.
//! Es lo que separa a un asistente que "responde" de un compañero que se acuerda:
//! el fallo no se evapora — se convierte en la próxima cosa que AION hace por ti.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pending {
    pub id: String,
    /// Epoch de cuándo quedó la deuda.
    pub at: i64,
    /// La tarea/pregunta original de Ariel, tal cual la pidió.
    pub task: String,
    /// Por qué quedó pendiente («no pude responderla», «Ariel me corrigió»).
    pub why: String,
    /// Intentos de resolución en segundo plano (a la 4ª sin éxito se abandona:
    /// insistir sin cambiar nada no es vida, es bucle).
    #[serde(default)]
    pub attempts: u32,
    /// Epoch del último intento (respiración de 2 h entre intentos).
    #[serde(default)]
    pub last_try: i64,
    #[serde(default)]
    pub resolved: bool,
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("pending.jsonl")
}

pub fn all() -> Vec<Pending> {
    let Ok(txt) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    txt.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn save(items: &[Pending]) {
    let body: String = items
        .iter()
        .filter_map(|p| serde_json::to_string(p).ok())
        .map(|l| l + "\n")
        .collect();
    crate::write_atomic(&path(), &body);
}

/// Apunta una deuda nueva. Dedup léxico contra las abiertas: la misma pregunta
/// dos veces es UNA deuda, no dos. Las triviales (<8 chars) no son deudas.
pub fn push(task: &str, why: &str) {
    let t = task.trim();
    if t.chars().count() < 8 {
        return;
    }
    let mut items = all();
    if items
        .iter()
        .any(|p| !p.resolved && crate::serve::texts_similar(&p.task, t))
    {
        return;
    }
    items.push(Pending {
        id: uuid::Uuid::new_v4().to_string(),
        at: chrono::Utc::now().timestamp(),
        task: t.chars().take(400).collect(),
        why: why.trim().to_string(),
        attempts: 0,
        last_try: 0,
        resolved: false,
    });
    // Tope suave: las 30 más recientes — una lista infinita de deudas viejas
    // no es memoria, es culpa acumulada.
    let extra = items.len().saturating_sub(30);
    if extra > 0 {
        items.drain(..extra);
    }
    save(&items);
}

/// La deuda abierta más antigua que TOCA reintentar ahora (≥2 h desde el último
/// intento, <4 intentos). None = no hay nada que deber o aún no toca.
pub fn next_due() -> Option<Pending> {
    let now = chrono::Utc::now().timestamp();
    all()
        .into_iter()
        .filter(|p| !p.resolved && p.attempts < 4 && now - p.last_try > 2 * 3600)
        .min_by_key(|p| p.at)
}

pub fn note_attempt(id: &str) {
    let mut items = all();
    for p in items.iter_mut() {
        if p.id == id {
            p.attempts += 1;
            p.last_try = chrono::Utc::now().timestamp();
        }
    }
    save(&items);
}

pub fn resolve(id: &str) {
    let mut items = all();
    for p in items.iter_mut() {
        if p.id == id {
            p.resolved = true;
        }
    }
    save(&items);
}

/// Cuántas deudas siguen abiertas (para el estado interno / prompt).
pub fn open_count() -> usize {
    all()
        .iter()
        .filter(|p| !p.resolved && p.attempts < 4)
        .count()
}
