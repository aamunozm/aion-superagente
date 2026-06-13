//! **Arranque y supervisión del Ollama EMBEBIDO de AION.**
//!
//! AION es local-first y *autocontenido*: trae su propio binario de Ollama en
//! `…/AION.app/Contents/Resources/ollama-runtime/ollama` y NO debe depender de ninguna
//! instalación externa (la app del cask, un `brew install`…) que pueda desaparecer. Sin
//! esto, AION podía quedar "vivo pero sin IA": el servidor HTTP arriba pero sin Ollama
//! detrás, y por tanto sin chat, sin embeddings (BGE-M3) y sin la compactación EN del
//! puente MCP — todo lo cual necesita el modelo local.
//!
//! Este módulo cierra ese hueco: al arrancar `serve`, garantiza que haya un Ollama
//! escuchando. **Idempotente y respetuoso**: si ya hay uno (el del usuario o uno previo),
//! lo reutiliza y no lanza otro; solo si NO hay, lanza el embebido como proceso hijo y lo
//! termina al cerrar AION limpiamente. *Fail-open*: si no encuentra el binario o no
//! arranca a tiempo, AION sigue sirviendo (lo que dependa del modelo degradará con
//! elegancia, no se cae).

use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// El hijo que LANZAMOS nosotros (si lo hicimos). Solo a este lo terminamos al cerrar:
/// un Ollama externo del usuario jamás se toca.
static SPAWNED: Mutex<Option<Child>> = Mutex::new(None);

/// `host:port` donde escucha Ollama. Override con `OLLAMA_HOST`.
fn host() -> String {
    std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "127.0.0.1:11434".to_string())
}

/// Resuelve la ruta del binario Ollama embebido. Orden: override explícito → relativo al
/// ejecutable (bundle .app y dev) → ruta conocida de la app instalada.
fn embedded_binary() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("AION_OLLAMA_BIN") {
        let pb = std::path::PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Bundle macOS: …/Contents/MacOS/aion-core → …/Contents/Resources/ollama-runtime/ollama
            let bundled = dir.join("../Resources/ollama-runtime/ollama");
            if bundled.exists() {
                return Some(bundled);
            }
            // Junto al ejecutable (empaquetados alternativos / dev).
            let sibling = dir.join("ollama-runtime/ollama");
            if sibling.exists() {
                return Some(sibling);
            }
        }
    }
    let known =
        std::path::PathBuf::from("/Applications/AION.app/Contents/Resources/ollama-runtime/ollama");
    if known.exists() {
        return Some(known);
    }
    None
}

/// ¿Responde ya un servidor Ollama en el host configurado?
async fn is_up() -> bool {
    let url = format!("http://{}/api/tags", host());
    reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Asegura que haya un Ollama escuchando. Idempotente: reutiliza el existente; solo lanza
/// el embebido si hace falta. Devuelve `true` si quedó disponible. Nunca paniquea.
pub async fn ensure_running() -> bool {
    if is_up().await {
        tracing::info!("Ollama ya responde — reutilizo el servidor existente");
        return true;
    }
    let Some(bin) = embedded_binary() else {
        tracing::warn!(
            "no encontré el binario Ollama embebido (ni AION_OLLAMA_BIN ni en el bundle); \
             la IA local no arrancará — funciones que dependen del modelo degradarán"
        );
        return false;
    };
    // Que `run_models_ensure` (y cualquier `ollama` que invoquemos) use ESTE mismo binario,
    // no el `ollama` del PATH (que en este equipo era un symlink roto).
    std::env::set_var("AION_OLLAMA_BIN", &bin);

    tracing::info!(bin = %bin.display(), "lanzando Ollama embebido");
    let spawn = Command::new(&bin)
        .arg("serve")
        .env("OLLAMA_HOST", host())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match spawn {
        Ok(child) => *SPAWNED.lock().unwrap() = Some(child),
        Err(e) => {
            tracing::error!(error = %e, "no pude lanzar Ollama embebido");
            return false;
        }
    }

    // Health-check hasta ~30 s (la primera vez tarda en bindear el puerto).
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if is_up().await {
            tracing::info!("Ollama embebido arriba y respondiendo");
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    tracing::error!("el Ollama embebido no respondió en 30 s");
    false
}

/// Termina el Ollama que lanzamos NOSOTROS (si lo hicimos). No toca un Ollama externo del
/// usuario. Se llama en el apagado limpio de AION.
pub fn shutdown() {
    if let Some(mut child) = SPAWNED.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
        tracing::info!("Ollama embebido detenido (lo habíamos lanzado nosotros)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_defaults_to_local() {
        // Sin OLLAMA_HOST en el entorno de test, debe ser el puerto estándar local.
        if std::env::var("OLLAMA_HOST").is_err() {
            assert_eq!(host(), "127.0.0.1:11434");
        }
    }
}
