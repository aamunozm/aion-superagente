//! Cola de ingesta en SEGUNDO PLANO para la biblioteca (Academias).
//!
//! Subir cientos de libros embebe mucho texto en CPU: bloquear la petición sería
//! inviable. La UI **encola** (guarda el archivo en staging + apunta un trabajo) y un
//! worker en el proceso del servidor los procesa de uno en uno, sin bloquear el chat.
//! Estado persistente en disco → sobrevive a reinicios (un trabajo «processing» a
//! medias se reintenta).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

/// Serializa el acceso al archivo de cola (un solo proceso, varias tareas).
static QLOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub domain: String,
    pub source: String,
    /// Archivo en staging con los bytes a ingerir.
    pub path: String,
    /// pending | processing | done | error
    pub status: String,
    #[serde(default)]
    pub passages: usize,
    #[serde(default)]
    pub error: String,
    /// Nº de veces que se ha tomado para procesar. Tope para no reintentar infinito un
    /// trabajo que falla siempre (p. ej. su archivo de staging ya no existe).
    #[serde(default)]
    pub attempts: u32,
}

fn queue_path() -> PathBuf {
    crate::app_data_dir().join("ingest_queue.json")
}

pub fn staging_dir() -> PathBuf {
    let d = crate::app_data_dir().join("library_staging");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn read() -> Vec<Job> {
    std::fs::read_to_string(queue_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write(jobs: &[Job]) {
    if let Ok(s) = serde_json::to_string(jobs) {
        let p = queue_path();
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// Encola un trabajo (el archivo ya está en staging). Devuelve su id.
pub fn enqueue(id: &str, domain: &str, source: &str, path: &str) {
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut jobs = read();
    // DEDUP POR ID: si ya hay un trabajo con este id (p. ej. el auto-modelo que se
    // re-siembra en cada arranque), reemplázalo en vez de ACUMULAR DUPLICADOS. Los ids
    // repetidos rompían take_next/update (solo tocaban el 1er match) → bucle infinito.
    jobs.retain(|j| j.id != id);
    // PODA: no dejes que la cola crezca sin límite con terminales viejos (done/error).
    if jobs.len() > 200 {
        jobs.retain(|j| j.status == "pending" || j.status == "processing");
    }
    jobs.push(Job {
        id: id.to_string(),
        domain: domain.to_string(),
        source: source.to_string(),
        path: path.to_string(),
        status: "pending".into(),
        passages: 0,
        error: String::new(),
        attempts: 0,
    });
    write(&jobs);
}

/// Toma el siguiente trabajo pendiente y lo marca «processing». (Reintenta también
/// los que quedaron «processing» por un reinicio.)
pub fn take_next() -> Option<Job> {
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut jobs = read();
    // TOPE DE REINTENTOS: un trabajo «processing» que ya se intentó demasiadas veces
    // (su archivo no existe / falla siempre) pasa a «error» y deja de re-cogerse. Sin
    // esto, un job que falla siempre se re-toma sin fin (bucle a cientos/seg).
    let mut changed = false;
    for j in jobs.iter_mut() {
        if j.status == "processing" && j.attempts >= 3 {
            j.status = "error".into();
            if j.error.is_empty() {
                j.error = "agotó reintentos".into();
            }
            changed = true;
        }
    }
    let pos = jobs
        .iter()
        .position(|j| j.status == "pending")
        .or_else(|| jobs.iter().position(|j| j.status == "processing"));
    let Some(pos) = pos else {
        if changed {
            write(&jobs);
        }
        return None;
    };
    jobs[pos].status = "processing".into();
    jobs[pos].attempts += 1;
    let job = jobs[pos].clone();
    write(&jobs);
    Some(job)
}

pub fn complete(id: &str, passages: usize) {
    update(id, |j| {
        j.status = "done".into();
        j.passages = passages;
    });
}

pub fn fail(id: &str, error: &str) {
    update(id, |j| {
        j.status = "error".into();
        j.error = error.to_string();
    });
}

fn update(id: &str, f: impl Fn(&mut Job)) {
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut jobs = read();
    if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
        f(j);
    }
    write(&jobs);
}

/// Resumen para la UI: conteos por estado + trabajos recientes.
pub fn snapshot() -> serde_json::Value {
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let jobs = read();
    let count = |s: &str| jobs.iter().filter(|j| j.status == s).count();
    let current = jobs
        .iter()
        .find(|j| j.status == "processing")
        .map(|j| j.source.clone());
    serde_json::json!({
        "pending": count("pending"),
        "processing": count("processing"),
        "done": count("done"),
        "error": count("error"),
        "current": current,
        "jobs": jobs,
    })
}

// ── Cache de ingesta incremental (SHA-256 por documento) ───────────────────
//
// Re-encolar un libro que no cambió es habitual (re-subidas masivas, sincronías).
// Guardamos el hash del archivo por "dominio::fuente": si coincide, el worker salta
// la ingesta entera (ni embeddings ni grafo). Archivo aparte de la cola, atómico.

fn cache_path() -> PathBuf {
    crate::app_data_dir().join("ingest_cache.json")
}

fn read_cache() -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_cache(map: &std::collections::HashMap<String, String>) {
    if let Ok(s) = serde_json::to_string(map) {
        crate::write_atomic(&cache_path(), &s);
    }
}

pub fn cached_sha(domain: &str, source: &str) -> Option<String> {
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_cache().get(&format!("{domain}::{source}")).cloned()
}

pub fn set_cached_sha(domain: &str, source: &str, sha: &str) {
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map = read_cache();
    map.insert(format!("{domain}::{source}"), sha.to_string());
    write_cache(&map);
}

pub fn clear_cached_sha(domain: &str, source: &str) {
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map = read_cache();
    if map.remove(&format!("{domain}::{source}")).is_some() {
        write_cache(&map);
    }
}

/// SHA-256 de un archivo (streaming, no carga el archivo entero en RAM).
pub fn sha256_file(path: &std::path::Path) -> Option<String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut f, &mut hasher).ok()?;
    Some(format!("{:x}", hasher.finalize()))
}

/// Limpia de la cola los trabajos terminados (done/error) y devuelve cuántos.
pub fn clear_finished() -> usize {
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut jobs = read();
    let before = jobs.len();
    jobs.retain(|j| j.status == "pending" || j.status == "processing");
    let removed = before - jobs.len();
    write(&jobs);
    removed
}
