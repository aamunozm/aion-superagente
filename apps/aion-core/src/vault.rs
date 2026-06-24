//! **Bóveda de SECRETOS GENERALES** (claves de API, datos bancarios, tokens, contraseñas sueltas)
//! con NOMBRE arbitrario — respaldada por el **Llavero de macOS** (cifrado por el SO, ligado a tu
//! sesión, en Apple Silicon protegido por el Secure Enclave). Igual patrón que [[credentials]]
//! (host/usuario/contraseña de webs) pero para secretos nombrados libremente.
//!
//! **Garantía de privacidad (el porqué de este módulo):** el VALOR vive SOLO en el Llavero; un
//! índice local (`vault_index.json`, 0600) guarda únicamente nombre + nota + fecha, NUNCA el valor.
//! Ni el LLM (local Gemma o externo) ni el puente MCP a Claude Code reciben jamás el valor: solo
//! `/api/vault/get` (local, bajo acción explícita del usuario) lo devuelve. Es el lugar CORRECTO
//! para datos bancarios/claves: no tocan `memory.jsonl`, no se embeben, no se sirven a ningún LLM.
//! Complementa la redacción (que captura secretos que se cuelan en la memoria por accidente): la
//! bóveda es donde los pones a propósito para que nunca lleguen ahí.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Servicio del Llavero (distinto de "AION-vault" de credentials.rs, que es host/usuario/pass).
const SERVICE: &str = "AION-secrets";

#[derive(Serialize, Deserialize, Clone)]
pub struct SecretMeta {
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub created_at: String,
}

fn index_path() -> PathBuf {
    crate::app_data_dir().join("vault_index.json")
}

/// Normaliza el nombre del secreto: sin caracteres de control, recortado, acotado a 128.
pub fn normalize_name(input: &str) -> String {
    input
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect::<String>()
        .trim()
        .to_string()
}

fn read_index() -> Vec<SecretMeta> {
    std::fs::read_to_string(index_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_index(list: &[SecretMeta]) {
    if let Ok(s) = serde_json::to_string_pretty(list) {
        // El índice revela QUÉ secretos guardas (no el valor) → 0600.
        crate::write_atomic_secret(&index_path(), &s);
    }
}

/// Guarda (o actualiza) un secreto. El valor va al Llavero; el índice solo registra
/// nombre + nota (+ fecha la primera vez). Devuelve Err si el backend seguro no está disponible.
pub fn set(name: &str, value: &str, note: &str) -> Result<(), String> {
    let name = normalize_name(name);
    if name.is_empty() {
        return Err("nombre vacío".into());
    }
    if value.is_empty() {
        return Err("valor vacío".into());
    }
    store_secret(&name, value)?;
    let mut idx = read_index();
    let created_at = idx
        .iter()
        .find(|m| m.name == name)
        .map(|m| m.created_at.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    idx.retain(|m| m.name != name);
    idx.push(SecretMeta {
        name,
        note: note.trim().chars().take(200).collect(),
        created_at,
    });
    write_index(&idx);
    Ok(())
}

/// Recupera el VALOR de un secreto. **Solo backend / acción local explícita** — jamás al LLM.
pub fn get(name: &str) -> Option<String> {
    load_secret(&normalize_name(name))
}

/// Lista los secretos guardados (nombre + nota + fecha). NUNCA incluye el valor.
pub fn list() -> Vec<SecretMeta> {
    let mut v = read_index();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// Elimina un secreto (Llavero + índice).
pub fn remove(name: &str) -> Result<(), String> {
    let name = normalize_name(name);
    delete_secret(&name)?;
    let mut idx = read_index();
    idx.retain(|m| m.name != name);
    write_index(&idx);
    Ok(())
}

// ── Backend de secretos: Llavero de macOS (cifrado por el SO) ───────────────

#[cfg(target_os = "macos")]
fn store_secret(name: &str, secret: &str) -> Result<(), String> {
    let out = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-U", // actualiza si ya existe
            "-s",
            SERVICE,
            "-a",
            name,
            "-w",
            secret,
        ])
        .output()
        .map_err(|e| format!("no pude acceder al Llavero: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(target_os = "macos")]
fn load_secret(name: &str) -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", SERVICE, "-a", name, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn delete_secret(name: &str) -> Result<(), String> {
    let _ = std::process::Command::new("security")
        .args(["delete-generic-password", "-s", SERVICE, "-a", name])
        .output();
    Ok(())
}

// ── Fallback (otros SO): aún no soportado de forma SEGURA ────────────────────
#[cfg(not(target_os = "macos"))]
fn store_secret(_name: &str, _secret: &str) -> Result<(), String> {
    Err("la bóveda segura solo está disponible en macOS por ahora".into())
}
#[cfg(not(target_os = "macos"))]
fn load_secret(_name: &str) -> Option<String> {
    None
}
#[cfg(not(target_os = "macos"))]
fn delete_secret(_name: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_name_strips_control_and_caps() {
        assert_eq!(normalize_name("  banco-santander \n"), "banco-santander");
        assert_eq!(normalize_name(&"x".repeat(200)).len(), 128);
        assert_eq!(normalize_name("   "), "");
    }
}
