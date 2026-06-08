//! Almacén vectorial. Implementa [`MemoryStore`].
//!
//! Soporta dos modos tras el mismo trait:
//! - **efímero** (`new`/`default_local`): solo en RAM.
//! - **persistente** (`persistent`): además escribe cada recuerdo a un archivo
//!   JSONL local (carga al arrancar) → AION recuerda entre sesiones.
//!
//! La recuperación es coseno lineal (suficiente para miles de recuerdos). En F2+
//! el backend ANN se sustituye por LanceDB embebido detrás de este mismo trait.
//! Los campos `fitness`/`access_count` preparan la memoria darwiniana (F4).

use crate::cosine;
use crate::embedder::OllamaEmbedder;
use aion_kernel::traits::{MemoryHit, MemoryStore};
use aion_kernel::{AionError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

/// Un registro de memoria con metadatos para la futura evolución darwiniana.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    /// Puntuación de aptitud (F4: selección/poda). Inicia neutra.
    pub fitness: f32,
    /// Veces que se ha recuperado este recuerdo.
    pub access_count: u32,
}

/// Memoria vectorial: embeddings + recuperación por coseno, con persistencia opcional.
pub struct VectorMemory {
    embedder: OllamaEmbedder,
    records: Mutex<Vec<MemoryRecord>>,
    path: Option<PathBuf>,
}

impl VectorMemory {
    /// Memoria efímera (solo RAM).
    pub fn new(embedder: OllamaEmbedder) -> Self {
        Self {
            embedder,
            records: Mutex::new(Vec::new()),
            path: None,
        }
    }

    /// Memoria efímera con valores por defecto (localhost + nomic).
    pub fn default_local() -> Self {
        Self::new(OllamaEmbedder::default_local())
    }

    /// Memoria persistente: carga los recuerdos previos del archivo JSONL si existe.
    pub fn persistent(embedder: OllamaEmbedder, path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let records = load_jsonl(&path)?;
        Ok(Self {
            embedder,
            records: Mutex::new(records),
            path: Some(path),
        })
    }

    /// Persistente con valores por defecto.
    pub fn persistent_local(path: impl Into<PathBuf>) -> Result<Self> {
        Self::persistent(OllamaEmbedder::default_local(), path)
    }

    pub fn len(&self) -> usize {
        self.records.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait]
impl MemoryStore for VectorMemory {
    async fn store(&self, content: &str) -> Result<String> {
        let embedding = self.embedder.embed(content).await?;
        let id = Uuid::new_v4().to_string();
        let record = MemoryRecord {
            id: id.clone(),
            content: content.to_string(),
            embedding,
            fitness: 0.5,
            access_count: 0,
        };
        if let Some(path) = &self.path {
            append_jsonl(path, &record)?;
        }
        self.records.lock().unwrap().push(record);
        Ok(id)
    }

    async fn retrieve(&self, query: &str, k: usize) -> Result<Vec<MemoryHit>> {
        let q = self.embedder.embed(query).await?;
        let mut scored: Vec<(f32, MemoryHit)> = {
            let mut recs = self.records.lock().unwrap();
            let mut out = Vec::with_capacity(recs.len());
            for r in recs.iter() {
                let score = cosine(&q, &r.embedding);
                out.push((
                    score,
                    MemoryHit {
                        id: r.id.clone(),
                        content: r.content.clone(),
                        score,
                    },
                ));
            }
            out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let top_ids: Vec<String> = out.iter().take(k).map(|(_, h)| h.id.clone()).collect();
            for r in recs.iter_mut() {
                if top_ids.contains(&r.id) {
                    r.access_count += 1;
                    r.fitness = (r.fitness + 0.05).min(1.0);
                }
            }
            out
        };
        scored.truncate(k);
        Ok(scored.into_iter().map(|(_, h)| h).collect())
    }
}

fn load_jsonl(path: &PathBuf) -> Result<Vec<MemoryRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path).map_err(|e| AionError::Memory(e.to_string()))?;
    let mut out = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| AionError::Memory(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<MemoryRecord>(&line) {
            out.push(rec);
        }
    }
    Ok(out)
}

fn append_jsonl(path: &PathBuf, record: &MemoryRecord) -> Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| AionError::Memory(e.to_string()))?;
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| AionError::Memory(e.to_string()))?;
    let line = serde_json::to_string(record)?;
    writeln!(file, "{line}").map_err(|e| AionError::Memory(e.to_string()))?;
    Ok(())
}
