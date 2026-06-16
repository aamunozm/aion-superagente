//! **API keys opcionales (gratis) que el usuario añade desde Ajustes.** Local-first: por defecto
//! AION investiga 100% sin key; aquí Ariel puede ENCHUFAR claves gratuitas para reforzar puntos
//! concretos (hoy: GitHub, que sin token rate-limita la búsqueda de repos). Se guardan cifradas a
//! nivel de permisos (0600, owner-only) en el directorio de datos y se exponen al resto del sistema
//! vía variables de entorno (mismo patrón desacoplado que `AION_PROXY`): así `aion-browser` las lee
//! sin acoplarse a la config de `aion-core`. La clave NUNCA se devuelve al cliente (solo un flag).

use serde::{Deserialize, Serialize};

/// Claves persistidas. Campos por proveedor (extensible: añadir brave/tavily/youtube… cuando toque).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeys {
    /// Token personal de GitHub (PAT). Sin scope o `public_repo` basta: sube el rate-limit de la
    /// búsqueda y habilita endpoints que sin token devuelven vacío.
    #[serde(default)]
    pub github: String,
}

/// Proveedor soportado en la UI de Ajustes → APIs (metadatos para pintar la lista).
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

/// Registro de proveedores que la UI ofrece. Empezamos por GitHub; crecerá aquí.
pub const PROVIDERS: &[Provider] = &[Provider {
    id: "github",
    label: "GitHub",
    help: "Token personal (Settings → Developer settings → Personal access tokens). Sin scope o \
           solo public_repo basta. Sube el rate-limit de la búsqueda de repos y habilita más.",
}];

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("apikeys.json")
}

pub fn load() -> ApiKeys {
    match std::fs::read_to_string(path()) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
        Err(_) => ApiKeys::default(),
    }
}

/// Guarda con permisos de secreto (0600 + rename atómico): contiene tokens.
pub fn save(keys: &ApiKeys) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(keys)?;
    crate::write_atomic_secret(&path(), &json);
    Ok(())
}

/// Devuelve la clave guardada de un proveedor (vacío si no hay). Para uso interno del backend.
pub fn get(provider: &str) -> String {
    let k = load();
    match provider {
        "github" => k.github,
        _ => String::new(),
    }
}

/// Fija/borra la clave de un proveedor (clave vacía = borrar), persiste y refresca el entorno.
/// Devuelve `false` si el proveedor no está soportado.
pub fn set(provider: &str, key: &str) -> bool {
    let mut k = load();
    match provider {
        "github" => k.github = key.trim().to_string(),
        _ => return false,
    }
    let _ = save(&k);
    apply_to_env(&k);
    true
}

/// Vuelca las claves no vacías a variables de entorno del proceso, para que los consumidores
/// (p. ej. `aion-browser::search_github`) las lean igual que `AION_PROXY`. Idempotente.
pub fn apply_to_env(keys: &ApiKeys) {
    set_or_clear("AION_GITHUB_TOKEN", &keys.github);
}

fn set_or_clear(var: &str, val: &str) {
    if val.trim().is_empty() {
        std::env::remove_var(var);
    } else {
        std::env::set_var(var, val.trim());
    }
}

/// Al arrancar: carga del disco y publica en el entorno. Llamar una vez en `serve::run`.
pub fn init_env() {
    apply_to_env(&load());
}
