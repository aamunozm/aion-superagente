//! Almacén vectorial en memoria (F1). Implementa [`MemoryStore`].
//!
//! En F2 se sustituye la persistencia por LanceDB (mismo trait). Los campos
//! `fitness`, `salience` y `access_count` preparan la memoria darwiniana (F4).

use crate::cosine;
use crate::embedder::OllamaEmbedder;
use aion_kernel::traits::{MemoryHit, MemoryStore};
use aion_kernel::Result;
use async_trait::async_trait;
use std::sync::Mutex;
use uuid::Uuid;

/// Un registro de memoria con metadatos para la futura evolución darwiniana.
#[derive(Debug, Clone)]
pub struct MemoryRecord {
    pub id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    /// Puntuación de aptitud (F4: selección/poda). Inicia neutra.
    pub fitness: f32,
    /// Veces que se ha recuperado este recuerdo.
    pub access_count: u32,
}

/// Memoria vectorial: embeddings + recuperación por coseno.
pub struct VectorMemory {
    embedder: OllamaEmbedder,
    records: Mutex<Vec<MemoryRecord>>,
}

impl VectorMemory {
    pub fn new(embedder: OllamaEmbedder) -> Self {
        Self {
            embedder,
            records: Mutex::new(Vec::new()),
        }
    }

    pub fn default_local() -> Self {
        Self::new(OllamaEmbedder::default_local())
    }

    /// Número de recuerdos almacenados.
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
        self.records.lock().unwrap().push(record);
        Ok(id)
    }

    async fn retrieve(&self, query: &str, k: usize) -> Result<Vec<MemoryHit>> {
        let q = self.embedder.embed(query).await?;
        let mut scored: Vec<(f32, MemoryHit)> = {
            let mut recs = self.records.lock().unwrap();
            let mut out = Vec::with_capacity(recs.len());
            for r in recs.iter_mut() {
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
            // refuerza fitness/acceso de los mejores tras seleccionar (abajo)
            out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            // marca acceso de los top-k
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
