//! Servicios de compresión para optimización de tokens.
//!
//! Implementa el trait CompressorService. Inicialmente con versiones simples
//! (whitespace, keyword extraction). LLMLingua viene en Phase 2.

use crate::multilingual::CompressorService;
use aion_kernel::Result;
use std::collections::HashSet;

/// Compressor stub: extrae palabras clave (50-60% compresión simple).
/// Fase 1B: validar arquitectura. Fase 2: reemplazar por LLMLingua.
pub struct KeywordCompressor {
    // Stopwords en inglés (palabras sin información semántica)
    stopwords: HashSet<&'static str>,
}

impl KeywordCompressor {
    pub fn new() -> Self {
        let stopwords = vec![
            "the", "a", "an", "and", "or", "but", "is", "are", "am", "was", "were", "be", "been",
            "being", "have", "has", "had", "do", "does", "did", "will", "would", "could", "should",
            "may", "might", "must", "can", "in", "on", "at", "to", "for", "of", "with", "by",
            "from", "as", "if", "so", "up", "out", "about", "into", "through", "during", "before",
            "after", "above", "below", "between", "under", "again", "further", "then", "once",
            "here", "there", "when", "where", "why", "how", "all", "both", "each", "few", "more",
            "most", "other", "some", "such", "no", "nor", "not", "only", "own", "same", "so",
            "than", "too", "very", "s", "t", "can", "just", "don", "now",
        ]
        .into_iter()
        .collect();

        Self { stopwords }
    }

    fn keep_word(&self, word: &str) -> bool {
        let clean = word.to_lowercase();
        !self.stopwords.contains(clean.as_str()) && clean.len() > 2
    }
}

impl Default for KeywordCompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl CompressorService for KeywordCompressor {
    fn compress_to_english(&self, text: &str) -> Result<String> {
        // Extrae palabras no-stopword + números + símbolos (50-60% compresión)
        let compressed = text
            .split_whitespace()
            .filter(|word| {
                self.keep_word(word) || word.chars().any(|c| c.is_numeric() || !c.is_alphabetic())
            })
            .collect::<Vec<_>>()
            .join(" ");

        Ok(compressed)
    }

    fn compression_ratio(&self, original: &str, compressed: &str) -> f32 {
        let orig_words = original.split_whitespace().count().max(1);
        let comp_words = compressed.split_whitespace().count().max(1);
        orig_words as f32 / comp_words as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_compressor_removes_stopwords() {
        let compressor = KeywordCompressor::new();
        let text = "The quick brown fox jumps over the lazy dog";
        let compressed = compressor.compress_to_english(text).unwrap();

        // Debe mantener: quick, brown, fox, jumps, lazy, dog
        // Debe quitar: the, over
        assert!(compressed.contains("quick"));
        assert!(compressed.contains("brown"));
        assert!(compressed.contains("fox"));
        assert!(!compressed.contains("the"));
    }

    #[test]
    fn keyword_compressor_calculates_ratio() {
        let compressor = KeywordCompressor::new();
        let original = "The quick brown fox";
        let compressed = compressor.compress_to_english(original).unwrap();
        let ratio = compressor.compression_ratio(original, &compressed);

        // Ratio debe ser > 1 (compresión positiva)
        assert!(ratio > 1.0);
        println!("Compression ratio: {:.2}x", ratio);
    }
}
