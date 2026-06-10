//! **Academias de AION**: biblioteca de conocimiento por dominios.
//!
//! Ingesta documentos largos (libros, PDFs, notas) que la memoria personal no puede
//! representar: los **trocea** en pasajes, los **embebe con BGE-M3** (multilingüe y
//! cross-lingual: puedes preguntar en español sobre un libro en inglés/italiano) y los
//! recupera con **cita** (fuente + nº de fragmento). Es un almacén SEPARADO de la
//! memoria personal (`memory.jsonl`) para no mezclar libros con tus recuerdos.

use aion_memory::{cosine, OllamaEmbedder};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Tamaño de ventana (palabras) por pasaje y solape entre pasajes contiguos.
const CHUNK_WORDS: usize = 220;
const OVERLAP_WORDS: usize = 40;

/// Un pasaje indexado de un documento.
#[derive(Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub domain: String,
    pub source: String,
    pub idx: usize,
    pub content: String,
    pub embedding: Vec<f32>,
}

/// Un resultado de búsqueda con su puntuación y procedencia (para citar).
pub struct Passage {
    pub score: f32,
    pub domain: String,
    pub source: String,
    pub idx: usize,
    pub content: String,
}

/// Biblioteca de conocimiento persistente (JSONL).
pub struct Library {
    path: PathBuf,
    embedder: OllamaEmbedder,
    chunks: Vec<Chunk>,
}

impl Library {
    /// Abre (o crea) la biblioteca en la ruta dada.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let chunks = load(&path);
        Self {
            path,
            embedder: OllamaEmbedder::default_local(),
            chunks,
        }
    }

    pub fn total_chunks(&self) -> usize {
        self.chunks.len()
    }

    /// Lista de documentos: (dominio, fuente, nº de pasajes).
    pub fn documents(&self) -> Vec<(String, String, usize)> {
        use std::collections::BTreeMap;
        let mut map: BTreeMap<(String, String), usize> = BTreeMap::new();
        for c in &self.chunks {
            *map.entry((c.domain.clone(), c.source.clone())).or_insert(0) += 1;
        }
        map.into_iter().map(|((d, s), n)| (d, s, n)).collect()
    }

    /// Ingesta un archivo (.txt/.md/.pdf) en un dominio. Si el documento ya existía
    /// (mismo dominio+fuente) lo reemplaza. Devuelve cuántos pasajes se indexaron.
    pub async fn ingest_file(&mut self, domain: &str, file: &Path) -> Result<usize, String> {
        let source = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "documento".into());
        let text = extract_text(file)?;
        self.ingest_text(domain, &source, &text).await
    }

    /// Ingesta texto plano bajo (dominio, fuente). Reemplaza si ya existía.
    pub async fn ingest_text(
        &mut self,
        domain: &str,
        source: &str,
        text: &str,
    ) -> Result<usize, String> {
        let passages = chunk_text(text);
        if passages.is_empty() {
            return Err("el documento no tiene texto legible".into());
        }
        // Quita una versión previa del mismo documento (reingesta idempotente).
        self.chunks
            .retain(|c| !(c.domain == domain && c.source == source));

        let mut added = 0usize;
        for (idx, content) in passages.into_iter().enumerate() {
            let embedding = self
                .embedder
                .embed(&content)
                .await
                .map_err(|e| format!("fallo de embedding: {e}"))?;
            self.chunks.push(Chunk {
                id: format!("{domain}::{source}#{idx}"),
                domain: domain.to_string(),
                source: source.to_string(),
                idx,
                content,
                embedding,
            });
            added += 1;
        }
        self.persist()?;
        Ok(added)
    }

    /// Busca los `k` pasajes más relevantes (opcionalmente dentro de un dominio).
    /// Multilingüe: la consulta y los pasajes pueden estar en idiomas distintos.
    pub async fn search(
        &self,
        query: &str,
        k: usize,
        domain: Option<&str>,
    ) -> Result<Vec<Passage>, String> {
        let q = self
            .embedder
            .embed(query)
            .await
            .map_err(|e| format!("fallo de embedding: {e}"))?;
        let mut scored: Vec<Passage> = self
            .chunks
            .iter()
            .filter(|c| domain.is_none_or(|d| c.domain == d))
            .map(|c| Passage {
                score: cosine(&q, &c.embedding),
                domain: c.domain.clone(),
                source: c.source.clone(),
                idx: c.idx,
                content: c.content.clone(),
            })
            .collect();
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k.max(1));
        Ok(scored)
    }

    fn persist(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("jsonl.tmp");
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        for c in &self.chunks {
            if let Ok(line) = serde_json::to_string(c) {
                writeln!(f, "{line}").map_err(|e| e.to_string())?;
            }
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn load(path: &Path) -> Vec<Chunk> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Chunk>(l).ok())
        .collect()
}

/// Extrae texto de un archivo según su extensión (.txt/.md/.pdf).
fn extract_text(file: &Path) -> Result<String, String> {
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "txt" | "md" | "markdown" | "text" => {
            std::fs::read_to_string(file).map_err(|e| format!("no pude leer el archivo: {e}"))
        }
        "pdf" => pdf_extract::extract_text(file)
            .map_err(|e| format!("no pude extraer el texto del PDF: {e}")),
        other => Err(format!(
            "formato no soportado todavía: «.{other}» (por ahora: .txt, .md, .pdf)"
        )),
    }
}

/// Trocea texto en pasajes por ventana de palabras con solape (preserva contexto
/// entre fragmentos contiguos). Normaliza espacios en blanco.
fn chunk_text(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    if words.len() <= CHUNK_WORDS {
        return vec![words.join(" ")];
    }
    let step = CHUNK_WORDS - OVERLAP_WORDS;
    let mut out = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let end = (start + CHUNK_WORDS).min(words.len());
        out.push(words[start..end].join(" "));
        if end == words.len() {
            break;
        }
        start += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_overlap_and_cover() {
        let text = (0..500).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
        let chunks = chunk_text(&text);
        assert!(chunks.len() >= 2);
        // El primer pasaje empieza por "0" y cubre CHUNK_WORDS palabras.
        assert!(chunks[0].starts_with("0 1 2"));
    }

    #[test]
    fn short_text_one_chunk() {
        assert_eq!(chunk_text("hola mundo").len(), 1);
    }
}
