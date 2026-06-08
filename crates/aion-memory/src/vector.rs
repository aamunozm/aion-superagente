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

/// Configuración del ciclo de consolidación ("sueño") darwiniano.
#[derive(Debug, Clone)]
pub struct ConsolidationConfig {
    /// Similitud coseno por encima de la cual dos recuerdos se fusionan.
    pub merge_threshold: f32,
    /// Aptitud por debajo de la cual un recuerdo nunca accedido se poda.
    pub prune_floor: f32,
    /// Factor de decaimiento de aptitud aplicado a todos (olvido gradual).
    pub decay: f32,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            merge_threshold: 0.95,
            prune_floor: 0.15,
            decay: 0.9,
        }
    }
}

/// Resultado de un ciclo de consolidación.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidationReport {
    pub before: usize,
    pub merged: usize,
    pub pruned: usize,
    pub after: usize,
}

impl VectorMemory {
    /// Ciclo de "sueño" (consolidación darwiniana):
    /// 1) decae la aptitud de todos (presión de olvido);
    /// 2) **fusiona** recuerdos casi-duplicados (suma accesos, conserva el mejor);
    /// 3) **poda** los de baja aptitud nunca accedidos.
    ///
    /// Conservador por diseño: si es persistente, guarda un snapshot `.bak`
    /// antes de reescribir — nunca destruye sin copia de seguridad.
    pub fn consolidate(&self, cfg: &ConsolidationConfig) -> Result<ConsolidationReport> {
        let mut recs = self.records.lock().unwrap();
        let before = recs.len();

        // 1) Decaimiento de aptitud.
        for r in recs.iter_mut() {
            r.fitness *= cfg.decay;
        }

        // 2) Fusión de casi-duplicados (greedy contra los ya conservados).
        let mut kept: Vec<MemoryRecord> = Vec::with_capacity(recs.len());
        let mut merged = 0usize;
        for r in recs.drain(..) {
            if let Some(k) = kept
                .iter_mut()
                .find(|k| cosine(&k.embedding, &r.embedding) >= cfg.merge_threshold)
            {
                k.access_count += r.access_count;
                k.fitness = k.fitness.max(r.fitness);
                merged += 1;
            } else {
                kept.push(r);
            }
        }

        // 3) Poda: fuera los de aptitud baja que nunca se usaron.
        let after_merge = kept.len();
        kept.retain(|r| r.fitness >= cfg.prune_floor || r.access_count > 0);
        let pruned = after_merge - kept.len();

        *recs = kept;
        let after = recs.len();
        let snapshot = recs.clone();
        drop(recs);

        // Persistencia: snapshot + reescritura completa.
        if let Some(path) = &self.path {
            if path.exists() {
                let bak = path.with_extension("jsonl.bak");
                let _ = std::fs::copy(path, &bak);
            }
            rewrite_jsonl(path, &snapshot)?;
        }

        Ok(ConsolidationReport {
            before,
            merged,
            pruned,
            after,
        })
    }
}

fn rewrite_jsonl(path: &PathBuf, records: &[MemoryRecord]) -> Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| AionError::Memory(e.to_string()))?;
        }
    }
    let mut buf = String::new();
    for r in records {
        buf.push_str(&serde_json::to_string(r)?);
        buf.push('\n');
    }
    std::fs::write(path, buf).map_err(|e| AionError::Memory(e.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(emb: Vec<f32>, fitness: f32, access: u32) -> MemoryRecord {
        MemoryRecord {
            id: Uuid::new_v4().to_string(),
            content: format!("emb{emb:?}"),
            embedding: emb,
            fitness,
            access_count: access,
        }
    }

    #[test]
    fn consolidation_merges_duplicates_and_prunes_weak() {
        let mem = VectorMemory::default_local();
        {
            let mut r = mem.records.lock().unwrap();
            r.push(rec(vec![1.0, 0.0, 0.0], 0.5, 1)); // usado
            r.push(rec(vec![1.0, 0.0, 0.0], 0.5, 0)); // casi-dup → se fusiona
            r.push(rec(vec![0.0, 1.0, 0.0], 0.05, 0)); // débil y sin uso → poda
        }
        let report = mem.consolidate(&ConsolidationConfig::default()).unwrap();
        assert_eq!(report.before, 3);
        assert_eq!(report.merged, 1);
        assert_eq!(report.pruned, 1);
        assert_eq!(report.after, 1);
        assert_eq!(mem.len(), 1);
    }

    #[test]
    fn consolidation_keeps_accessed_memories() {
        let mem = VectorMemory::default_local();
        {
            let mut r = mem.records.lock().unwrap();
            r.push(rec(vec![0.0, 0.0, 1.0], 0.01, 5)); // aptitud baja pero MUY usada
        }
        let report = mem.consolidate(&ConsolidationConfig::default()).unwrap();
        assert_eq!(report.pruned, 0); // no se poda lo que se usa
        assert_eq!(mem.len(), 1);
    }
}
