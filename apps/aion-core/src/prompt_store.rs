//! Almacén PERSISTENTE y auto-editable de prompts (modos). Permite que AION
//! **refine sus propias instrucciones** (estilo OPRO/DSPy) y que esas mejoras
//! sobrevivan a reinicios, con historial por versiones para poder revertir.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PromptOverride {
    task: String,
    instruction: String,
    version: u32,
}

fn store_path() -> PathBuf {
    crate::app_data_dir().join("prompts.jsonl")
}

/// Devuelve la instrucción optimizada vigente para una tarea, si existe.
pub fn current(task: &str) -> Option<String> {
    load()
        .into_iter()
        .filter(|p| p.task == task)
        .max_by_key(|p| p.version)
        .map(|p| p.instruction)
}

/// Versión vigente (0 si no hay override).
pub fn current_version(task: &str) -> u32 {
    load()
        .into_iter()
        .filter(|p| p.task == task)
        .map(|p| p.version)
        .max()
        .unwrap_or(0)
}

/// Guarda una instrucción mejorada como una NUEVA versión (conserva el historial
/// → se puede revertir). Append-only.
pub fn save_new_version(task: &str, instruction: &str) -> std::io::Result<u32> {
    use std::io::Write;
    let version = current_version(task) + 1;
    let rec = PromptOverride {
        task: task.to_string(),
        instruction: instruction.to_string(),
        version,
    };
    let path = store_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(f, "{}", serde_json::to_string(&rec)?)?;
    Ok(version)
}

fn load() -> Vec<PromptOverride> {
    match std::fs::read_to_string(store_path()) {
        Ok(t) => t
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
        Err(_) => vec![],
    }
}
