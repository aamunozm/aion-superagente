//! Rutinas: tareas que AION ejecuta SOLO en un horario diario, reportando el resultado a la
//! Bandeja. Ejecución desatendida con AUTO-APROBACIÓN (Ariel autoriza al crear la rutina); las
//! acciones peligrosas siguen bloqueadas en el runner (ver `run_routine_task` en serve.rs).
//! Persistido como JSON en el directorio de datos.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routine {
    #[serde(default)]
    pub id: String,
    pub title: String,
    /// La tarea en lenguaje natural que el agente ejecuta (puede mencionar una skill).
    pub prompt: String,
    /// Hora local diaria "HH:MM" a la que se ejecuta.
    pub time: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// "YYYY-MM-DD" del último día que corrió (evita repetir el mismo día).
    #[serde(default)]
    pub last_run: String,
}
fn yes() -> bool {
    true
}

fn path() -> PathBuf {
    crate::app_data_dir().join("routines.json")
}

pub fn all() -> Vec<Routine> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_all(rs: &[Routine]) -> std::io::Result<()> {
    std::fs::write(path(), serde_json::to_string_pretty(rs)?)
}

/// Crea (genera id) o actualiza una rutina por id.
pub fn upsert(mut r: Routine) -> std::io::Result<Routine> {
    let mut rs = all();
    if r.id.trim().is_empty() {
        r.id = uuid::Uuid::new_v4().to_string();
    }
    match rs.iter_mut().find(|x| x.id == r.id) {
        Some(e) => *e = r.clone(),
        None => rs.push(r.clone()),
    }
    save_all(&rs)?;
    Ok(r)
}

pub fn remove(id: &str) -> std::io::Result<()> {
    let mut rs = all();
    rs.retain(|x| x.id != id);
    save_all(&rs)
}

/// Marca la fecha del último día en que corrió (idempotencia diaria del planificador).
pub fn mark_ran(id: &str, date: &str) {
    let mut rs = all();
    if let Some(x) = rs.iter_mut().find(|x| x.id == id) {
        x.last_run = date.to_string();
    }
    let _ = save_all(&rs);
}
