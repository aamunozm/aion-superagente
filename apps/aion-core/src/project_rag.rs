//! **RAG por PROYECTO sobre las fuentes** (estilo NotebookLM, pero el agente además ACTÚA).
//!
//! Reutiliza el MISMO motor de embeddings que la memoria de AION —BGE-M3 vía
//! [`aion_memory::OllamaEmbedder`] + [`aion_memory::cosine`]— pero con un índice **AISLADO por
//! proyecto**: un archivo `projects/<pid>/source_index.jsonl` por proyecto. Así la información de
//! un proyecto NUNCA se cruza con la de otro (archivos distintos), y queda SEPARADA de la memoria
//! de hechos de AION (no la contaminamos con texto crudo de documentos del usuario).
//!
//! Flujo: al cambiar las fuentes se (re)indexan TODAS (chunk → embedding); al chatear, se
//! recuperan los fragmentos más RELEVANTES a la pregunta (no el prefijo del documento), filtrando
//! a las fuentes ACTIVAS. Si aún no hay índice (Ollama caído o proyecto viejo), se cae con
//! elegancia al grounding clásico por prefijo ([`crate::projects::grounding`]).

use aion_memory::{cosine, OllamaEmbedder};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// Tamaño objetivo de cada fragmento (en caracteres). ~700 da granularidad fina sin trocear
/// demasiado; BGE-M3 admite hasta 8192 tokens, así que sobra margen.
const CHUNK_CHARS: usize = 700;
/// Fragmentos relevantes que se inyectan en el grounding por pregunta.
const TOP_K: usize = 8;
/// Umbral mínimo de similitud coseno para considerar un fragmento «relevante».
const MIN_SIM: f32 = 0.2;

#[derive(Serialize, Deserialize, Clone)]
struct Chunk {
    source_id: String,
    title: String,
    kind: String,
    text: String,
    #[serde(default)]
    embedding: Vec<f32>,
}

fn index_path(pid: &str) -> PathBuf {
    crate::projects::project_dir(pid).join("source_index.jsonl")
}

/// Parte un texto en fragmentos de ~`target` chars respetando saltos de línea; un bloque enorme
/// (p. ej. texto de un PDF sin saltos) se trocea por ventanas. Devuelve trozos no vacíos.
fn chunk_text(content: &str, target: usize) -> Vec<String> {
    let text = content.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for seg in text.split('\n') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        // Segmento gigantesco → ventanas de `target` chars.
        if seg.chars().count() > target * 2 {
            if !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let chars: Vec<char> = seg.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let end = (i + target).min(chars.len());
                out.push(chars[i..end].iter().collect());
                i = end;
            }
            continue;
        }
        if cur.chars().count() + seg.chars().count() + 1 > target && !cur.trim().is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(seg);
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// (Re)indexa TODAS las fuentes del proyecto (activas o no; el filtro de actividad se aplica al
/// recuperar). Devuelve cuántos fragmentos quedaron indexados. Si Ollama no responde a mitad,
/// aborta SIN tocar el índice anterior (deja el último bueno). Escritura atómica.
pub async fn reindex(pid: &str) -> usize {
    let srcs = crate::projects::sources(pid);
    let path = index_path(pid);
    if srcs.is_empty() {
        let _ = std::fs::remove_file(&path);
        return 0;
    }
    let embedder = OllamaEmbedder::default_local();
    let mut chunks: Vec<Chunk> = Vec::new();
    for src in &srcs {
        for piece in chunk_text(&src.content, CHUNK_CHARS) {
            match embedder.embed(&piece).await {
                Ok(embedding) => chunks.push(Chunk {
                    source_id: src.id.clone(),
                    title: src.title.clone(),
                    kind: src.kind.clone(),
                    text: piece,
                    embedding,
                }),
                // Embedder caído → no malogramos el índice existente.
                Err(_) => return 0,
            }
        }
    }
    let mut buf = String::new();
    for c in &chunks {
        if let Ok(line) = serde_json::to_string(c) {
            buf.push_str(&line);
            buf.push('\n');
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("jsonl.tmp");
    if std::fs::write(&tmp, &buf).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
    chunks.len()
}

/// (Re)indexa en SEGUNDO PLANO (no bloquea el endpoint que cambió las fuentes).
pub fn reindex_bg(pid: &str) {
    let pid = pid.to_string();
    tokio::spawn(async move {
        let _ = reindex(&pid).await;
    });
}

fn load_chunks(pid: &str) -> Vec<Chunk> {
    let Ok(text) = std::fs::read_to_string(index_path(pid)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<Chunk>(l).ok())
        .filter(|c| !c.embedding.is_empty())
        .collect()
}

/// Recupera los `k` fragmentos más relevantes a `query`, SOLO de las fuentes ACTIVAS. Devuelve
/// `(título, tipo, texto)`. Vacío si no hay índice o el embedder no responde.
async fn retrieve(pid: &str, query: &str, k: usize) -> Vec<(String, String, String)> {
    let chunks = load_chunks(pid);
    if chunks.is_empty() {
        return Vec::new();
    }
    // Filtro de ACTIVAS: el toggle no re-indexa; se respeta aquí.
    let active: HashSet<String> = crate::projects::sources(pid)
        .into_iter()
        .filter(|s| s.active)
        .map(|s| s.id)
        .collect();
    let embedder = OllamaEmbedder::default_local();
    let Ok(q) = embedder.embed(query).await else {
        return Vec::new();
    };
    let mut scored: Vec<(f32, &Chunk)> = chunks
        .iter()
        .filter(|c| active.contains(&c.source_id))
        .map(|c| (cosine(&q, &c.embedding), c))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(k)
        .filter(|(s, _)| *s > MIN_SIM)
        .map(|(_, c)| (c.title.clone(), c.kind.clone(), c.text.clone()))
        .collect()
}

/// Grounding del agente para un proyecto, **enfocado por la pregunta**: cabecera del proyecto +
/// los fragmentos de fuentes más relevantes a `query` (recuperación semántica). Si aún no hay
/// índice (proyecto viejo / primera vez) intenta construirlo; si sigue vacío, cae al grounding
/// clásico por prefijo. Aislado por proyecto: nunca mezcla fuentes de otros proyectos.
pub async fn grounding_for_query(pid: &str, query: &str) -> String {
    // Primera vez (sin índice) pero con fuentes → indexa ahora (coste único; luego persiste).
    if !index_path(pid).exists() && !crate::projects::sources(pid).is_empty() {
        let _ = reindex(pid).await;
    }
    let hits = retrieve(pid, query, TOP_K).await;
    if hits.is_empty() {
        // Sin recuperación útil → comportamiento anterior (prefijo de fuentes activas).
        return crate::projects::grounding(pid);
    }
    let mut s = crate::projects::header(pid);
    // Comentarios de Ariel sobre las fuentes (prioridad máxima), antes de los fragmentos.
    s.push_str(&crate::projects::source_notes_block(pid));
    s.push_str(
        "\nFRAGMENTOS RELEVANTES DE LAS FUENTES (recuperados por SIGNIFICADO para esta pregunta — \
         básate en ellos y cítalos por su título; si no bastan, dilo con franqueza):\n",
    );
    for (title, kind, text) in hits {
        s.push_str(&format!("- «{title}» [{kind}]: {text}\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_respeta_objetivo_y_trocea_bloques_enormes() {
        // Texto con saltos: se agrupa en ~target.
        let t = "uno dos tres\ncuatro cinco\nseis siete ocho";
        let c = chunk_text(t, 20);
        assert!(!c.is_empty());
        assert!(c.iter().all(|x| !x.trim().is_empty()));
        // Bloque gigante sin saltos: se trocea por ventanas.
        let big = "a".repeat(2000);
        let c2 = chunk_text(&big, 500);
        assert!(
            c2.len() >= 4,
            "un bloque de 2000 con target 500 da >=4 trozos"
        );
        assert!(c2.iter().all(|x| x.chars().count() <= 500));
    }

    #[test]
    fn chunk_vacio_es_vacio() {
        assert!(chunk_text("   \n  \n", 100).is_empty());
    }
}
