//! # aion-memory
//!
//! Memoria de AION. Implementa [`aion_kernel::MemoryStore`].
//!
//! - F1 (actual): [`VectorMemory`] — embeddings vía Ollama (`nomic-embed-text`) +
//!   recuperación por similitud coseno en memoria. Port del prototipo `rag_demo.py`.
//! - F2: persistencia con **LanceDB** detrás del mismo trait (embebido, local-first).
//! - F4: campos de fitness/salience/access para la **memoria darwiniana** y la
//!   consolidación tipo "sueño".

pub mod compressor;
mod embedder;
pub mod multilingual;
mod vector;

pub use embedder::OllamaEmbedder;
pub use multilingual::{
    CompressorService, DocumentMetadata, Language, MultilingualDocument, MultilingualMemory,
    RetrievalResult,
};
pub use vector::{
    is_unknown_time, ConsolidationConfig, ConsolidationReport, MemoryRecord, VectorMemory,
};

/// Calcula la similitud coseno entre dos vectores. Devuelve 0.0 si las
/// dimensiones difieren o algún vector es nulo.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::cosine;

    #[test]
    fn cosine_identity_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    }

    #[test]
    fn cosine_dim_mismatch_is_zero() {
        assert_eq!(cosine(&[1.0, 2.0], &[1.0]), 0.0);
    }
}
