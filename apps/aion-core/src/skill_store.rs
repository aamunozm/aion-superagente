//! Almacén **persistente** de skills forjadas. Cuando AION se escribe una skill
//! nueva, se guarda aquí (WAT + manifiesto) y se vuelve a cargar en cada arranque.
//! Así su caja de herramientas CRECE con el tiempo: es mejor en lo que hace porque
//! acumula capacidades, no parte de cero en cada sesión.

use aion_skills::{SkillManifest, WasmSkillHost};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSkill {
    name: String,
    description: String,
    wat: String,
}

fn store_path() -> PathBuf {
    crate::app_data_dir().join("skills.jsonl")
}

/// Guarda una skill forjada (idempotente por nombre: reemplaza si ya existía).
pub fn save(name: &str, description: &str, wat: &str) -> std::io::Result<()> {
    let path = store_path();
    let mut skills = load_records();
    skills.retain(|s| s.name != name);
    skills.push(StoredSkill {
        name: name.to_string(),
        description: description.to_string(),
        wat: wat.to_string(),
    });
    let body: String = skills
        .iter()
        .filter_map(|s| serde_json::to_string(s).ok())
        .map(|s| s + "\n")
        .collect();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)
}

/// Carga TODAS las skills persistidas en el host (al arrancar). Devuelve cuántas.
pub fn load_all(host: &WasmSkillHost) -> usize {
    let mut n = 0;
    for s in load_records() {
        if host
            .register(
                SkillManifest {
                    name: s.name.clone(),
                    description: s.description.clone(),
                },
                s.wat.as_bytes(),
            )
            .is_ok()
        {
            n += 1;
        }
    }
    n
}

/// Nombres + descripciones de las skills persistidas (para mostrarlas al agente).
pub fn catalog() -> Vec<(String, String)> {
    load_records()
        .into_iter()
        .map(|s| (s.name, s.description))
        .collect()
}

fn load_records() -> Vec<StoredSkill> {
    match std::fs::read_to_string(store_path()) {
        Ok(t) => t
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
        Err(_) => vec![],
    }
}
