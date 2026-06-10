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
    let _g = QLOCK.lock().unwrap();
    let mut jobs = read();
    jobs.push(Job {
        id: id.to_string(),
        domain: domain.to_string(),
        source: source.to_string(),
        path: path.to_string(),
        status: "pending".into(),
        passages: 0,
        error: String::new(),
    });
    write(&jobs);
}

/// Toma el siguiente trabajo pendiente y lo marca «processing». (Reintenta también
/// los que quedaron «processing» por un reinicio.)
pub fn take_next() -> Option<Job> {
    let _g = QLOCK.lock().unwrap();
    let mut jobs = read();
    let pos = jobs
        .iter()
        .position(|j| j.status == "pending")
        .or_else(|| jobs.iter().position(|j| j.status == "processing"))?;
    jobs[pos].status = "processing".into();
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
    let _g = QLOCK.lock().unwrap();
    let mut jobs = read();
    if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
        f(j);
    }
    write(&jobs);
}

/// Resumen para la UI: conteos por estado + trabajos recientes.
pub fn snapshot() -> serde_json::Value {
    let _g = QLOCK.lock().unwrap();
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

/// Limpia de la cola los trabajos terminados (done/error) y devuelve cuántos.
pub fn clear_finished() -> usize {
    let _g = QLOCK.lock().unwrap();
    let mut jobs = read();
    let before = jobs.len();
    jobs.retain(|j| j.status == "pending" || j.status == "processing");
    let removed = before - jobs.len();
    write(&jobs);
    removed
}
