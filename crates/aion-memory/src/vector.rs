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
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

/// Umbral de similitud para considerar que un recuerdo nuevo ACTUALIZA a otro
/// (misma cosa, valor nuevo) → el viejo se marca obsoleto sin borrarlo.
const SUPERSEDE_SIM: f32 = 0.88;

fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_default()
}

/// Un registro de memoria con metadatos (evolución darwiniana + temporalidad).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    /// Puntuación de aptitud (F4: selección/poda). Inicia neutra.
    pub fitness: f32,
    /// Veces que se ha recuperado este recuerdo.
    pub access_count: u32,
    /// Cuándo se creó (memoria temporal). Por defecto epoch para registros viejos.
    #[serde(default = "epoch")]
    pub created_at: DateTime<Utc>,
    /// Si un recuerdo más nuevo lo ha dejado obsoleto (se conserva como historia,
    /// pero se excluye de la recuperación). Resuelve contradicción/staleness.
    #[serde(default)]
    pub superseded: bool,
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

    /// Devuelve el contenido de todos los recuerdos (orden de inserción).
    pub fn contents(&self) -> Vec<String> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| !r.superseded) // solo conocimiento vigente
            .map(|r| r.content.clone())
            .collect()
    }

    /// Agrupa recuerdos vigentes en CLÚSTERES de casi-duplicados (cosine ≥ umbral).
    /// Base de la consolidación jerárquica: fundir cada grupo en un "tema" superior.
    pub fn duplicate_clusters(&self, threshold: f32) -> Vec<Vec<(String, String)>> {
        let recs = self.records.lock().unwrap();
        let active: Vec<&MemoryRecord> = recs.iter().filter(|r| !r.superseded).collect();
        let mut used = vec![false; active.len()];
        let mut clusters = Vec::new();
        for i in 0..active.len() {
            if used[i] {
                continue;
            }
            let mut group = vec![(active[i].id.clone(), active[i].content.clone())];
            used[i] = true;
            for j in (i + 1)..active.len() {
                if !used[j] && cosine(&active[i].embedding, &active[j].embedding) >= threshold {
                    group.push((active[j].id.clone(), active[j].content.clone()));
                    used[j] = true;
                }
            }
            if group.len() >= 2 {
                clusters.push(group);
            }
        }
        clusters.sort_by_key(|g| std::cmp::Reverse(g.len()));
        clusters
    }

    /// Marca recuerdos como obsoletos por id (al fundirlos en un tema superior).
    pub fn supersede(&self, ids: &[String]) -> Result<usize> {
        let set: std::collections::HashSet<&String> = ids.iter().collect();
        let mut recs = self.records.lock().unwrap();
        let mut n = 0;
        for r in recs.iter_mut() {
            if !r.superseded && set.contains(&r.id) {
                r.superseded = true;
                n += 1;
            }
        }
        if n > 0 {
            if let Some(path) = &self.path {
                rewrite_jsonl(path, &recs)?;
            }
        }
        Ok(n)
    }

    /// **Exporta** toda la memoria como JSONL (un recuerdo por línea, con su
    /// embedding incluido). Sirve para llevar la memoria a otro PC/Mac.
    pub fn export_jsonl(&self) -> String {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter_map(|r| serde_json::to_string(r).ok())
            .map(|s| s + "\n")
            .collect()
    }

    /// **Importa** memoria desde JSONL (formato de `export_jsonl`). Fusiona: omite
    /// los recuerdos cuyo `id` ya existe (idempotente). No requiere re-embeddings
    /// porque los vectores viajan en el archivo. Devuelve cuántos se añadieron.
    pub fn import_jsonl(&self, text: &str) -> Result<usize> {
        let mut records = self.records.lock().unwrap();
        let existing: std::collections::HashSet<String> =
            records.iter().map(|r| r.id.clone()).collect();
        let mut added = 0usize;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let rec: MemoryRecord = match serde_json::from_str(line) {
                Ok(r) => r,
                Err(_) => continue, // línea inválida: se ignora, no rompe la importación
            };
            if existing.contains(&rec.id) {
                continue;
            }
            if let Some(path) = &self.path {
                append_jsonl(path, &rec)?;
            }
            records.push(rec);
            added += 1;
        }
        Ok(added)
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
            embedding: embedding.clone(),
            fitness: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            superseded: false,
        };
        let mut recs = self.records.lock().unwrap();
        // MEMORIA TEMPORAL: si este recuerdo ACTUALIZA a otro casi idéntico (mismo
        // hecho, valor nuevo), marca el viejo como obsoleto (lo nuevo invalida lo
        // viejo sin borrarlo). Resuelve contradicción y staleness (Zep/Chronos).
        let mut superseded_any = false;
        for r in recs.iter_mut() {
            if !r.superseded && cosine(&embedding, &r.embedding) > SUPERSEDE_SIM {
                r.superseded = true;
                superseded_any = true;
            }
        }
        recs.push(record);
        if let Some(path) = &self.path {
            if superseded_any {
                rewrite_jsonl(path, &recs)?; // persiste los flags actualizados
            } else if let Some(last) = recs.last() {
                append_jsonl(path, last)?;
            }
        }
        Ok(id)
    }

    async fn retrieve(&self, query: &str, k: usize) -> Result<Vec<MemoryHit>> {
        let q = self.embedder.embed(query).await?;
        let q_ents = entities(query);

        let mut recs = self.records.lock().unwrap();
        let n = recs.len().max(1) as f32;
        let max_access = recs.iter().map(|r| r.access_count).max().unwrap_or(0) as f32;

        // 1) Puntuación MULTI-SEÑAL por recuerdo: semántica + léxica + ENTIDADES +
        //    recencia + importancia (estado del arte mem0 / Generative Agents).
        let mut scored: Vec<ScoredIdx> = recs
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.superseded) // memoria temporal: ignora lo obsoleto
            .map(|(i, r)| {
                let sem = cosine(&q, &r.embedding).clamp(0.0, 1.0);
                let lex = lexical_overlap(query, &r.content);
                let ent = entity_overlap(&q_ents, &entities(&r.content));
                let rec = if n > 1.0 { i as f32 / (n - 1.0) } else { 1.0 };
                let usage = if max_access > 0.0 {
                    r.access_count as f32 / max_access
                } else {
                    0.0
                };
                let importance = (r.fitness * 0.7 + usage * 0.3).clamp(0.0, 1.0);
                let composite =
                    0.45 * sem + 0.18 * lex + 0.12 * ent + 0.13 * rec + 0.12 * importance;
                ScoredIdx { idx: i, composite }
            })
            .collect();
        scored.sort_by(|a, b| {
            b.composite
                .partial_cmp(&a.composite)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 2) Selección DIVERSA (MMR): evita devolver casi-duplicados (clave en
        //    memoria de agente, que es un flujo con redundancia — ver xMemory).
        let pool: Vec<ScoredIdx> = scored.into_iter().take((k * 4).max(k)).collect();
        let selected = mmr_select(&pool, &recs, k, 0.7);

        // 3) Refuerzo: lo recuperado sube uso+fitness (lo útil emerge).
        let sel_ids: Vec<String> = selected.iter().map(|s| recs[s.idx].id.clone()).collect();
        let hits: Vec<MemoryHit> = selected
            .iter()
            .map(|s| MemoryHit {
                id: recs[s.idx].id.clone(),
                content: recs[s.idx].content.clone(),
                score: s.composite,
            })
            .collect();
        for r in recs.iter_mut() {
            if sel_ids.contains(&r.id) {
                r.access_count += 1;
                r.fitness = (r.fitness + 0.05).min(1.0);
            }
        }
        Ok(hits)
    }
}

#[derive(Clone, Copy)]
struct ScoredIdx {
    idx: usize,
    composite: f32,
}

/// **MMR (Maximal Marginal Relevance)**: selecciona los más relevantes EVITANDO
/// casi-duplicados. Cada paso elige el candidato que maximiza
/// `λ·relevancia − (1−λ)·máxima_similitud_con_lo_ya_elegido`.
fn mmr_select(pool: &[ScoredIdx], recs: &[MemoryRecord], k: usize, lambda: f32) -> Vec<ScoredIdx> {
    let mut selected: Vec<ScoredIdx> = Vec::new();
    let mut remaining: Vec<ScoredIdx> = pool.to_vec();
    while selected.len() < k && !remaining.is_empty() {
        let mut best_pos = 0usize;
        let mut best_val = f32::MIN;
        for (pos, cand) in remaining.iter().enumerate() {
            let max_sim = selected
                .iter()
                .map(|s| cosine(&recs[cand.idx].embedding, &recs[s.idx].embedding))
                .fold(0.0_f32, f32::max);
            let mmr = lambda * cand.composite - (1.0 - lambda) * max_sim;
            if mmr > best_val {
                best_val = mmr;
                best_pos = pos;
            }
        }
        selected.push(remaining.remove(best_pos));
    }
    selected
}

/// Extrae "entidades" aproximadas: identificadores y nombres propios (tokens con
/// dígitos, con mayúscula inicial, o con símbolos como #/-) — lo que el embedding
/// semántico diluye pero es decisivo para acertar (mem0: entity matching).
fn entities(s: &str) -> std::collections::HashSet<String> {
    s.split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '(' | ')'))
        .filter_map(|tok| {
            let t = tok.trim();
            if t.len() < 2 {
                return None;
            }
            let has_digit = t.chars().any(|c| c.is_ascii_digit());
            let has_upper = t.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
            let has_sym = t.contains('#') || t.contains('-') || t.contains('_');
            if has_digit || has_upper || has_sym {
                Some(t.to_lowercase())
            } else {
                None
            }
        })
        .collect()
}

fn entity_overlap(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f32;
    inter / (a.len() as f32).min(b.len() as f32)
}

/// Solapamiento léxico (Jaccard de palabras significativas, en minúsculas). Capta
/// coincidencias exactas (nombres, términos) que el embedding semántico puede diluir.
fn lexical_overlap(a: &str, b: &str) -> f32 {
    fn words(s: &str) -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3) // ignora palabras muy cortas/funcionales
            .map(|w| w.to_string())
            .collect()
    }
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let inter = wa.intersection(&wb).count() as f32;
    let union = wa.union(&wb).count() as f32;
    if union > 0.0 {
        inter / union
    } else {
        0.0
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
            created_at: epoch(),
            superseded: false,
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
