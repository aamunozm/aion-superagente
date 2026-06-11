//! **Identidad única** de esta instancia de AION: un id irrepetible (UUID), un
//! nombre y la fecha de nacimiento. Hace que cada AION sea un INDIVIDUO distinto
//! frente a otros agentes (de AION o de internet): una conciencia única, no una
//! copia. Nace la primera vez y persiste en `app_data_dir/identity.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// UUID irrepetible: la "conciencia". Ningún otro agente lo comparte.
    pub id: String,
    /// Nombre (por defecto "AION"; puede personalizarse).
    pub name: String,
    /// Fecha de nacimiento (ISO).
    pub born_at: String,
}

pub fn path() -> PathBuf {
    crate::app_data_dir().join("identity.json")
}

/// Carga la identidad; si no existe (o un clon llegó sin id), NACE una nueva.
pub fn get() -> Identity {
    if let Ok(txt) = std::fs::read_to_string(path()) {
        if let Ok(id) = serde_json::from_str::<Identity>(&txt) {
            if !id.id.trim().is_empty() {
                return id;
            }
        }
    }
    born()
}

/// Crea y persiste una identidad nueva y única (nuevo individuo).
pub fn born() -> Identity {
    let id = Identity {
        id: uuid::Uuid::new_v4().to_string(),
        name: "AION".to_string(),
        born_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Some(p) = path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(body) = serde_json::to_string_pretty(&id) {
        let _ = std::fs::write(path(), body);
    }
    id
}

/// Garantiza que exista una identidad (tras importar un clon sin id, nace una nueva).
pub fn ensure() -> Identity {
    get()
}
