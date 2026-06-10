//! **Bóveda de credenciales** — almacenamiento SEGURO de usuario/contraseña por sitio.
//!
//! Diseño de seguridad:
//! - En macOS se guardan en el **Llavero** del sistema (cifrado por el SO, ligado a tu
//!   sesión) vía el binario `security`. Nunca en texto plano en disco.
//! - El **LLM nunca recibe** las credenciales: solo el backend llama a `get()` para
//!   inyectarlas en el formulario; no hay ninguna herramienta que devuelva el valor.
//! - Un índice aparte (`credentials_index.json`) guarda SOLO los hosts y usuarios (sin
//!   contraseñas) para poder listarlos en Ajustes.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SERVICE: &str = "AION-vault";

#[derive(Serialize, Deserialize, Clone)]
pub struct CredMeta {
    pub host: String,
    pub user: String,
}

fn index_path() -> PathBuf {
    crate::app_data_dir().join("credentials_index.json")
}

/// Normaliza el host (sin esquema, sin ruta, en minúsculas) para una clave estable.
pub fn normalize_host(input: &str) -> String {
    let s = input.trim().to_lowercase();
    let s = s.split_once("://").map(|(_, r)| r).unwrap_or(s.as_str());
    let s = s.split(['/', '?', '#']).next().unwrap_or(s);
    s.trim_start_matches("www.").to_string()
}

fn read_index() -> Vec<CredMeta> {
    std::fs::read_to_string(index_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_index(list: &[CredMeta]) {
    if let Ok(s) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(index_path(), s);
    }
}

/// Guarda (o actualiza) las credenciales de un host. El secreto va al Llavero; el
/// índice solo registra host+usuario (sin contraseña).
pub fn set(host: &str, user: &str, pass: &str) -> Result<(), String> {
    let host = normalize_host(host);
    let secret = serde_json::json!({ "u": user, "p": pass }).to_string();
    store_secret(&host, &secret)?;
    let mut idx = read_index();
    idx.retain(|c| c.host != host);
    idx.push(CredMeta {
        host,
        user: user.to_string(),
    });
    write_index(&idx);
    Ok(())
}

/// Recupera (usuario, contraseña) de un host. **Solo backend** — jamás se expone al LLM.
pub fn get(host: &str) -> Option<(String, String)> {
    let host = normalize_host(host);
    let secret = load_secret(&host)?;
    let v: serde_json::Value = serde_json::from_str(&secret).ok()?;
    Some((
        v["u"].as_str().unwrap_or("").to_string(),
        v["p"].as_str().unwrap_or("").to_string(),
    ))
}

/// Lista los sitios guardados (host + usuario). NUNCA incluye contraseñas.
pub fn list() -> Vec<CredMeta> {
    read_index()
}

/// Elimina las credenciales de un host (Llavero + índice).
pub fn remove(host: &str) -> Result<(), String> {
    let host = normalize_host(host);
    delete_secret(&host)?;
    let mut idx = read_index();
    idx.retain(|c| c.host != host);
    write_index(&idx);
    Ok(())
}

// ── Backend de secretos: Llavero de macOS (cifrado por el SO) ───────────────

#[cfg(target_os = "macos")]
fn store_secret(host: &str, secret: &str) -> Result<(), String> {
    // -U actualiza si ya existe. El secreto va por stdin-equivalente (-w <valor>).
    let out = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            SERVICE,
            "-a",
            host,
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
fn load_secret(host: &str) -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", SERVICE, "-a", host, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn delete_secret(host: &str) -> Result<(), String> {
    let _ = std::process::Command::new("security")
        .args(["delete-generic-password", "-s", SERVICE, "-a", host])
        .output();
    Ok(())
}

// ── Fallback (otros SO): aún no soportado de forma SEGURA ────────────────────
// (Windows Credential Manager / Secret Service quedan para más adelante; no
// guardamos contraseñas en texto plano.)

#[cfg(not(target_os = "macos"))]
fn store_secret(_host: &str, _secret: &str) -> Result<(), String> {
    Err("la bóveda segura solo está disponible en macOS por ahora".into())
}
#[cfg(not(target_os = "macos"))]
fn load_secret(_host: &str) -> Option<String> {
    None
}
#[cfg(not(target_os = "macos"))]
fn delete_secret(_host: &str) -> Result<(), String> {
    Ok(())
}
