//! **Proyectos de AION** — un espacio de trabajo por proyecto (inspirado en
//! NotebookLM): cada proyecto agrupa **Fuentes** (conocimiento), **Chat/Agente**
//! (con foco) y **Studio** (salidas generadas). A diferencia de NotebookLM, el
//! agente no solo resume: ACTÚA dentro del foco del proyecto.
//!
//! Persistencia local en JSON bajo `app_data_dir()/projects/`:
//!
//! - `index.json` → lista de proyectos
//! - `<id>/sources.json` → fuentes del proyecto
//! - `<id>/studio.json` → salidas de Studio del proyecto

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub desc: String,
    /// Emoji o icono representativo (decorativo).
    #[serde(default)]
    pub icon: String,
    pub created: String,
    #[serde(default)]
    pub updated: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub title: String,
    /// "nota" | "texto" | "web" | "archivo".
    pub kind: String,
    #[serde(default)]
    pub content: String,
    /// Si está ACTIVA, se usa para anclar (grounding) el chat/agente del proyecto.
    #[serde(default = "yes")]
    pub active: bool,
    pub created: String,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub id: String,
    /// "informe" | "resumen" | "mapa".
    pub kind: String,
    pub title: String,
    pub content: String,
    pub created: String,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn base() -> PathBuf {
    crate::app_data_dir().join("projects")
}
fn index_path() -> PathBuf {
    base().join("index.json")
}
fn sources_path(pid: &str) -> PathBuf {
    base().join(pid).join("sources.json")
}
fn studio_path(pid: &str) -> PathBuf {
    base().join(pid).join("studio.json")
}

fn read_vec<T: DeserializeOwned>(path: &PathBuf) -> Vec<T> {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}
fn write_vec<T: Serialize>(path: &PathBuf, v: &[T]) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(body) = serde_json::to_string_pretty(v) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

// ── Proyectos ───────────────────────────────────────────────────────────────

pub fn list() -> Vec<Project> {
    read_vec(&index_path())
}

pub fn get(id: &str) -> Option<Project> {
    list().into_iter().find(|p| p.id == id)
}

pub fn create(name: &str, desc: &str, icon: &str) -> Project {
    let p = Project {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        desc: desc.trim().to_string(),
        icon: icon.trim().to_string(),
        created: now(),
        updated: now(),
    };
    let mut all = list();
    all.insert(0, p.clone());
    write_vec(&index_path(), &all);
    p
}

pub fn remove(id: &str) {
    let all: Vec<Project> = list().into_iter().filter(|p| p.id != id).collect();
    write_vec(&index_path(), &all);
    let _ = std::fs::remove_dir_all(base().join(id));
}

fn touch(id: &str) {
    let mut all = list();
    if let Some(p) = all.iter_mut().find(|p| p.id == id) {
        p.updated = now();
        write_vec(&index_path(), &all);
    }
}

// ── Fuentes ─────────────────────────────────────────────────────────────────

pub fn sources(pid: &str) -> Vec<Source> {
    read_vec(&sources_path(pid))
}

pub fn add_source(pid: &str, title: &str, kind: &str, content: &str) -> Source {
    let s = Source {
        id: uuid::Uuid::new_v4().to_string(),
        title: title.trim().to_string(),
        kind: kind.trim().to_string(),
        content: content.trim().to_string(),
        active: true,
        created: now(),
    };
    let mut all = sources(pid);
    all.insert(0, s.clone());
    write_vec(&sources_path(pid), &all);
    touch(pid);
    s
}

pub fn toggle_source(pid: &str, sid: &str, active: bool) {
    let mut all = sources(pid);
    if let Some(s) = all.iter_mut().find(|s| s.id == sid) {
        s.active = active;
        write_vec(&sources_path(pid), &all);
    }
}

pub fn remove_source(pid: &str, sid: &str) {
    let all: Vec<Source> = sources(pid).into_iter().filter(|s| s.id != sid).collect();
    write_vec(&sources_path(pid), &all);
}

// ── Studio (salidas) ──────────────────────────────────────────────────────────

pub fn outputs(pid: &str) -> Vec<Output> {
    read_vec(&studio_path(pid))
}

pub fn add_output(pid: &str, kind: &str, title: &str, content: &str) -> Output {
    let o = Output {
        id: uuid::Uuid::new_v4().to_string(),
        kind: kind.trim().to_string(),
        title: title.trim().to_string(),
        content: content.trim().to_string(),
        created: now(),
    };
    let mut all = outputs(pid);
    all.insert(0, o.clone());
    write_vec(&studio_path(pid), &all);
    touch(pid);
    o
}

pub fn remove_output(pid: &str, oid: &str) {
    let all: Vec<Output> = outputs(pid).into_iter().filter(|o| o.id != oid).collect();
    write_vec(&studio_path(pid), &all);
}

/// Contexto de anclaje (grounding) del proyecto para el chat/agente: el objetivo
/// más el texto de las fuentes ACTIVAS (recortado). Así el agente responde con
/// foco en el material del proyecto, no en general.
pub fn grounding(pid: &str) -> String {
    let Some(p) = get(pid) else {
        return String::new();
    };
    let mut s = format!("PROYECTO EN FOCO: «{}».", p.name);
    if !p.desc.is_empty() {
        s.push_str(&format!(" Objetivo: {}.", p.desc));
    }
    let active: Vec<Source> = sources(pid).into_iter().filter(|s| s.active).collect();
    if !active.is_empty() {
        s.push_str(
            "\nFUENTES DEL PROYECTO (basa tus respuestas en ellas y cítalas por su título):\n",
        );
        // Presupuesto de caracteres para no inflar el prompt.
        let per = (8000 / active.len().max(1)).clamp(200, 2000);
        for src in active {
            let body: String = src.content.chars().take(per).collect();
            s.push_str(&format!("- «{}» [{}]: {}\n", src.title, src.kind, body));
        }
    }
    s
}
