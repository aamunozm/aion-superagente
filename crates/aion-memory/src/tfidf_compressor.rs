//! Compressor basado en TF-IDF — selecciona palabras más relevantes.
//!
//! Fase 2: Compresión más sofisticada que KeywordCompressor (fase 1).
//! Aproxima LLMLingua sin necesidad de modelo: mantiene ~20-30% de tokens
//! más relevantes (5-12x compresión según budget).
//!
//! Algoritmo:
//! 1. Calcular TF-IDF para cada palabra (frecuencia × rareza)
//! 2. Filtrar stopwords
//! 3. Mantener top-K% tokens por score
//! 4. Reconstruir texto preservando orden

use crate::multilingual::CompressorService;
use aion_kernel::Result;
use std::collections::HashMap;

/// Compressor TF-IDF — mantiene palabras más relevantes según importancia.
pub struct TfidfCompressor {
    target_ratio: f32, // 0.3 = mantener 30% tokens (3.3x compresión)
}

impl TfidfCompressor {
    /// Crear compressor con ratio de compresión objetivo.
    /// - 0.2 = ~5x compresión
    /// - 0.3 = ~3.3x compresión
    /// - 0.4 = ~2.5x compresión
    pub fn new(target_ratio: f32) -> Self {
        let target_ratio = target_ratio.clamp(0.1, 0.8);
        Self { target_ratio }
    }

    fn tokenize(text: &str) -> Vec<(usize, String)> {
        text.split_whitespace()
            .enumerate()
            .map(|(i, w)| (i, w.to_lowercase()))
            .collect()
    }

    fn is_stopword(word: &str) -> bool {
        matches!(
            word,
            "the"
                | "a"
                | "an"
                | "and"
                | "or"
                | "but"
                | "is"
                | "are"
                | "am"
                | "was"
                | "were"
                | "be"
                | "been"
                | "have"
                | "has"
                | "had"
                | "do"
                | "does"
                | "did"
                | "will"
                | "would"
                | "could"
                | "should"
                | "may"
                | "might"
                | "must"
                | "can"
                | "in"
                | "on"
                | "at"
                | "to"
                | "for"
                | "of"
                | "with"
                | "by"
                | "from"
                | "as"
                | "if"
                | "so"
                | "up"
                | "out"
                | "about"
                | "into"
                | "through"
                | "during"
                | "before"
                | "after"
                | "above"
                | "below"
                | "between"
                | "under"
                | "again"
                | "further"
                | "then"
                | "once"
                | "here"
                | "there"
                | "when"
                | "where"
                | "why"
                | "how"
                | "all"
                | "both"
                | "each"
                | "few"
                | "more"
                | "most"
                | "other"
                | "some"
                | "such"
                | "no"
                | "nor"
                | "not"
                | "only"
                | "own"
                | "same"
                | "than"
                | "too"
                | "very"
                | "just"
                | "don"
                | "now"
                | "your"
                | "it"
                | "its"
                | "they"
                | "them"
                | "this"
                | "that"
                | "these"
                | "those"
        )
    }

    fn calculate_tfidf(tokens: &[(usize, String)]) -> HashMap<String, f32> {
        let total_words = tokens.len() as f32;

        // Frecuencia de cada palabra
        let mut term_freq: HashMap<String, f32> = HashMap::new();
        for (_pos, word) in tokens {
            *term_freq.entry(word.clone()).or_insert(0.0) += 1.0;
        }

        // TF-IDF: TF × (log2(N / DF))
        // DF = número de documentos donde aparece (aquí: 1)
        // Aproximamos: IDF ≈ frecuencia inversa + longitud palabra
        let mut tfidf: HashMap<String, f32> = HashMap::new();
        for (word, freq) in term_freq.iter() {
            let tf = freq / total_words; // Frecuencia relativa
            let word_len = word.len() as f32;

            // IDF: palabras más largas tienden a ser más específicas
            let idf = (1.0 + (total_words / freq).log2()) * (word_len / 10.0).max(0.5);

            let score = tf * idf;
            tfidf.insert(word.clone(), score);
        }

        tfidf
    }
}

impl Default for TfidfCompressor {
    fn default() -> Self {
        Self::new(0.25) // ~4x compresión por defecto
    }
}

impl CompressorService for TfidfCompressor {
    fn compress_to_english(&self, text: &str) -> Result<String> {
        let tokens = Self::tokenize(text);
        if tokens.is_empty() {
            return Ok(text.to_string());
        }

        let tfidf = Self::calculate_tfidf(&tokens);

        // Seleccionar tokens top-K% por score
        let keep_count = (tokens.len() as f32 * self.target_ratio).max(1.0) as usize;
        let mut scored_tokens: Vec<_> = tokens
            .iter()
            .enumerate()
            .map(|(idx, (pos, word))| {
                let score = tfidf.get(word).copied().unwrap_or(0.1);
                // Bonus por stopword (mantener para coherencia)
                let is_stop = Self::is_stopword(word);
                let adjusted_score = if is_stop { score * 0.3 } else { score };
                (idx, *pos, word.clone(), adjusted_score)
            })
            .collect();

        // Ordenar por score, tomar top-K
        scored_tokens.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        let kept_indices: std::collections::HashSet<_> =
            scored_tokens.iter().take(keep_count).map(|t| t.1).collect();

        // Reconstruir en orden original
        let compressed = tokens
            .iter()
            .filter(|(pos, _)| kept_indices.contains(pos))
            .map(|(_, word)| word.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        Ok(compressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tfidf_compressor_basic() {
        let compressor = TfidfCompressor::new(0.3);
        let text = "The quick brown fox jumps over the lazy dog very quickly";
        let compressed = compressor.compress_to_english(text).unwrap();

        // Debe ser más corto
        assert!(
            compressed.len() < text.len(),
            "compressed='{}', original='{}'",
            compressed,
            text
        );

        // Debe mantener palabras clave (quick, fox, jumps, lazy, dog)
        assert!(compressed.contains("fox") || compressed.contains("quick"));
    }

    #[test]
    fn tfidf_compressor_ratio() {
        let compressor = TfidfCompressor::new(0.25);
        let text =
            "Artificial intelligence and machine learning are transforming the technology industry";
        let compressed = compressor.compress_to_english(text).unwrap();

        let orig_words = text.split_whitespace().count();
        let comp_words = compressed.split_whitespace().count();

        println!("Original: {} words", orig_words);
        println!("Compressed: {} words", comp_words);
        println!(
            "Ratio: {:.2}x",
            orig_words as f32 / comp_words.max(1) as f32
        );

        // Debe comprimir
        assert!(comp_words < orig_words);
    }

    #[test]
    fn tfidf_compressor_empty() {
        let compressor = TfidfCompressor::new(0.3);
        let result = compressor.compress_to_english("");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }
}
