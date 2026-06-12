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
/// Rango de similitud para crear una ARISTA asociativa (relacionados, no idénticos):
/// base del grafo de memoria que conecta recuerdos entre chats distintos (GAAMA).
const LINK_SIM_MIN: f32 = 0.62;
const MAX_LINKS: usize = 6;

fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_default()
}

/// Estima la **importancia** de un recuerdo (0..1) por señales deterministas —cero
/// latencia, sin LLM—: lo etiquetado como aprendizaje/reflexión, las preferencias y
/// decisiones del usuario, y los datos de identidad pesan más que un comentario de
/// paso. Inspirado en la puntuación de importancia de Generative Agents, pero barata.
pub fn estimate_importance(content: &str) -> f32 {
    let t = content.to_lowercase();
    let mut score: f32 = 0.4; // base
                              // Conocimiento que AION se forjó a sí mismo: vale más que charla.
    if t.starts_with("[aprendizaje]") || t.starts_with("[reflexión]") {
        score += 0.3;
    }
    // Preferencias, decisiones e identidad del usuario: lo más valioso de recordar.
    const HEAVY: [&str; 14] = [
        "prefiero",
        "me gusta",
        "odio",
        "no me gusta",
        "siempre",
        "nunca",
        "importante",
        "recuerda que",
        "decidimos",
        "mi objetivo",
        "mi nombre",
        "vivo en",
        "trabajo en",
        "no quiero",
    ];
    if HEAVY.iter().any(|k| t.contains(k)) {
        score += 0.25;
    }
    // Algo de sustancia (no un «ok»): premia el contenido con cuerpo, satura pronto.
    score += (content.chars().count() as f32 / 600.0).min(0.15);
    score.clamp(0.0, 1.0)
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
    /// CUÁNDO dejó de ser válido (bi-temporal): junto a `created_at` permite responder
    /// «esto lo creías hasta el martes». `None` mientras sigue vigente.
    #[serde(default)]
    pub superseded_at: Option<DateTime<Utc>>,
    /// Importancia del recuerdo (0..1): cuánto MERECE recordarse, estimada al guardar.
    /// Las decisiones, preferencias e identidad pesan más que un comentario de paso.
    #[serde(default = "default_importance")]
    pub importance: f32,
    /// ARISTAS asociativas: ids de recuerdos relacionados (grafo de memoria). Permite
    /// recordar por asociación entre chats distintos (GAAMA).
    #[serde(default)]
    pub links: Vec<String>,
    /// PROCEDENCIA: quién escribió el recuerdo. `""` = el propio AION;
    /// `"claude-code"` = un agente externo conectado. Permite cuarentena suave
    /// (marcar lo externo al inyectarlo en prompts) sin separar almacenes.
    #[serde(default)]
    pub origin: String,
}

fn default_importance() -> f32 {
    0.5
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

    /// Últimos `n` recuerdos vigentes con su fecha de creación (del más antiguo al
    /// más reciente), para que el agente sitúe en el tiempo lo que ha vivido
    /// («hace 2 horas estudié…») en vez de un pasado plano.
    pub fn recent_with_time(&self, n: usize) -> Vec<(String, DateTime<Utc>)> {
        let recs = self.records.lock().unwrap();
        let mut v: Vec<(String, DateTime<Utc>)> = recs
            .iter()
            .filter(|r| !r.superseded)
            .rev()
            .take(n)
            .map(|r| (r.content.clone(), r.created_at))
            .collect();
        v.reverse();
        v
    }

    /// Últimos `n` recuerdos vigentes como (id, contenido), para capas que puentean
    /// la memoria con otras estructuras (p. ej. el grafo de conocimiento).
    pub fn recent_with_ids(&self, n: usize) -> Vec<(String, String)> {
        let recs = self.records.lock().unwrap();
        recs.iter()
            .filter(|r| !r.superseded)
            .rev()
            .take(n)
            .map(|r| (r.id.clone(), r.content.clone()))
            .collect()
    }

    /// **Qué cambió** desde `since` (bi-temporal): lo que AION aprendió de nuevo y lo
    /// que dejó de ser válido en esa ventana. Permite responder «¿qué ha cambiado desde
    /// la semana pasada?» sin un grafo completo.
    pub fn changes_since(&self, since: DateTime<Utc>) -> (Vec<String>, Vec<String>) {
        let recs = self.records.lock().unwrap();
        let nuevos: Vec<String> = recs
            .iter()
            .filter(|r| !r.superseded && r.created_at >= since)
            .map(|r| r.content.clone())
            .collect();
        let obsoletos: Vec<String> = recs
            .iter()
            .filter(|r| r.superseded && r.superseded_at.map(|t| t >= since).unwrap_or(false))
            .map(|r| r.content.clone())
            .collect();
        (nuevos, obsoletos)
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

    /// **Reindexa** la memoria si el modelo de embeddings cambió: re-embebe todo
    /// recuerdo cuya dimensión no coincida con la del modelo actual (p. ej. al pasar
    /// de nomic 768-dim a BGE-M3 1024-dim). Sin esto, los vectores viejos y nuevos no
    /// son comparables y la recuperación se rompe. Idempotente y barato si no hay nada
    /// que migrar (solo un embed de sondeo). Devuelve cuántos recuerdos se reindexaron.
    pub async fn reindex_if_needed(&self) -> Result<usize> {
        // Dimensión objetivo del modelo actual (sondeo).
        let probe = self.embedder.embed("dimension probe").await?;
        let target = probe.len();
        if target == 0 {
            return Ok(0);
        }
        // Recoge los que necesitan re-embeber (sin mantener el lock cruzando await).
        let stale: Vec<(String, String)> = {
            let recs = self.records.lock().unwrap();
            recs.iter()
                .filter(|r| r.embedding.len() != target)
                .map(|r| (r.id.clone(), r.content.clone()))
                .collect()
        };
        if stale.is_empty() {
            return Ok(0);
        }
        let mut new_vecs: Vec<(String, Vec<f32>)> = Vec::with_capacity(stale.len());
        for (id, content) in &stale {
            let v = self.embedder.embed(content).await?;
            new_vecs.push((id.clone(), v));
        }
        let mut recs = self.records.lock().unwrap();
        let map: std::collections::HashMap<String, Vec<f32>> = new_vecs.into_iter().collect();
        let mut n = 0;
        for r in recs.iter_mut() {
            if let Some(v) = map.get(&r.id) {
                r.embedding = v.clone();
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

    /// Guarda un recuerdo declarando su PROCEDENCIA y un techo de importancia.
    /// Para escrituras de agentes externos (p. ej. Claude Code): el `origin` queda
    /// en el registro y `max_importance` impide que contenido externo supersedee
    /// preferencias/decisiones del usuario (ver lógica de supersede más abajo).
    pub async fn store_with_origin(
        &self,
        content: &str,
        origin: &str,
        max_importance: f32,
    ) -> Result<String> {
        let embedding = self.embedder.embed(content).await?;
        let id = Uuid::new_v4().to_string();
        let mut record = MemoryRecord {
            id: id.clone(),
            content: content.to_string(),
            embedding: embedding.clone(),
            fitness: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            superseded: false,
            superseded_at: None,
            importance: estimate_importance(content).min(max_importance),
            links: Vec::new(),
            origin: origin.to_string(),
        };
        let now = Utc::now();
        let mut recs = self.records.lock().unwrap();
        // MEMORIA TEMPORAL: si actualiza a otro casi idéntico, marca el viejo obsoleto.
        // GRAFO ASOCIATIVO: si está RELACIONADO (sin ser idéntico), crea una arista
        // bidireccional → recuerdos de chats distintos quedan conectados (GAAMA).
        let mut dirty = false;
        let mut sims: Vec<(usize, f32)> = Vec::new();
        for (i, r) in recs.iter_mut().enumerate() {
            if r.superseded {
                continue;
            }
            let sim = cosine(&embedding, &r.embedding);
            // Supersede CONSCIENTE DE IMPORTANCIA: un comentario de paso no puede
            // invalidar una preferencia/decisión del usuario sobre el mismo tema
            // (con BGE-M3, paráfrasis del mismo tópico superan 0.88 con facilidad).
            // Solo actualiza si el recuerdo nuevo pesa al menos tanto como el viejo
            // (tolerancia 0.1); si no, quedan ENLAZADOS como relacionados.
            if sim > SUPERSEDE_SIM && record.importance + 0.1 >= r.importance {
                r.superseded = true;
                r.superseded_at = Some(now); // bi-temporal: cuándo dejó de ser válido
                dirty = true;
            } else if sim >= LINK_SIM_MIN {
                sims.push((i, sim));
            }
        }
        // Conecta con los más relacionados (top MAX_LINKS), arista bidireccional.
        sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (i, _) in sims.into_iter().take(MAX_LINKS) {
            let other_id = recs[i].id.clone();
            if !recs[i].links.contains(&id) {
                recs[i].links.push(id.clone());
                dirty = true;
            }
            if !record.links.contains(&other_id) {
                record.links.push(other_id);
            }
        }
        recs.push(record);
        if let Some(path) = &self.path {
            if dirty {
                rewrite_jsonl(path, &recs)?; // persiste flags + aristas actualizadas
            } else if let Some(last) = recs.last() {
                append_jsonl(path, last)?;
            }
        }
        Ok(id)
    }
}

#[async_trait]
impl MemoryStore for VectorMemory {
    async fn store(&self, content: &str) -> Result<String> {
        self.store_with_origin(content, "", 1.0).await
    }

    async fn retrieve(&self, query: &str, k: usize) -> Result<Vec<MemoryHit>> {
        let q = self.embedder.embed(query).await?;
        let q_ents = entities(query);

        let mut recs = self.records.lock().unwrap();
        let max_access = recs.iter().map(|r| r.access_count).max().unwrap_or(0) as f32;
        let now = Utc::now();

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
                // Recencia REAL (Generative Agents): decay exponencial por edad con
                // semivida de 7 días — antes era el índice ordinal, que premiaba la
                // posición en el archivo y no el tiempo.
                let age_days = (now - r.created_at).num_seconds().max(0) as f32 / 86_400.0;
                let rec = 0.5_f32.powf(age_days / 7.0);
                let usage = if max_access > 0.0 {
                    r.access_count as f32 / max_access
                } else {
                    0.0
                };
                // Importancia: lo estimado al guardar (preferencias/decisiones/identidad)
                // reforzado por el uso real y la aptitud acumulada.
                let importance =
                    (r.importance * 0.6 + r.fitness * 0.25 + usage * 0.15).clamp(0.0, 1.0);
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

impl VectorMemory {
    /// **Recuperación ASOCIATIVA** (GAAMA): recupera los más relevantes y luego
    /// recorre el grafo de aristas `hops` saltos para traer recuerdos relacionados
    /// — incluso de OTROS chats — que el match directo no encontraría.
    pub async fn retrieve_associative(
        &self,
        query: &str,
        k: usize,
        hops: usize,
    ) -> Result<Vec<MemoryHit>> {
        let base = self.retrieve(query, k).await?;
        let mut result = base.clone();
        let mut seen: std::collections::HashSet<String> =
            base.iter().map(|h| h.id.clone()).collect();

        let recs = self.records.lock().unwrap();
        let by_id: std::collections::HashMap<&str, &MemoryRecord> =
            recs.iter().map(|r| (r.id.as_str(), r)).collect();

        let mut frontier: Vec<String> = base.iter().map(|h| h.id.clone()).collect();
        let mut decay = 0.6_f32;
        for _ in 0..hops {
            let mut next = Vec::new();
            for id in &frontier {
                // Vecinos por aristas SALIENTES y ENTRANTES (grafo no-dirigido):
                // garantiza la asociación sin importar quién creó la arista.
                let mut neighbors: Vec<String> = Vec::new();
                if let Some(r) = by_id.get(id.as_str()) {
                    neighbors.extend(r.links.iter().cloned());
                }
                for r in recs.iter() {
                    if r.links.iter().any(|l| l == id) {
                        neighbors.push(r.id.clone());
                    }
                }
                for lid in neighbors {
                    if seen.contains(&lid) {
                        continue;
                    }
                    if let Some(lr) = by_id.get(lid.as_str()) {
                        if lr.superseded {
                            continue;
                        }
                        seen.insert(lid.clone());
                        result.push(MemoryHit {
                            id: lid.clone(),
                            content: lr.content.clone(),
                            score: decay,
                        });
                        next.push(lid);
                    }
                }
            }
            frontier = next;
            decay *= 0.7;
        }
        Ok(result)
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
    s.split(|c: char| {
        c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '(' | ')')
    })
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
            superseded_at: None,
            importance: 0.5,
            links: Vec::new(),
            origin: String::new(),
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

    #[test]
    fn importance_weights_preferences_and_learnings() {
        let casual = estimate_importance("hoy hizo sol");
        let pref = estimate_importance("prefiero que me hables en español, siempre");
        let learn = estimate_importance("[aprendizaje] usar la herramienta files para contar");
        assert!(pref > casual, "una preferencia pesa más que un comentario");
        assert!(learn > casual, "una lección pesa más que un comentario");
        assert!((0.0..=1.0).contains(&pref));
    }

    #[test]
    fn changes_since_separates_new_and_obsolete() {
        let mem = VectorMemory::default_local();
        let t0 = epoch();
        {
            let mut r = mem.records.lock().unwrap();
            let mut viejo = rec(vec![1.0, 0.0], 0.5, 0);
            viejo.content = "creo X".into();
            viejo.superseded = true;
            viejo.superseded_at = Some(Utc::now());
            r.push(viejo);
            let mut nuevo = rec(vec![0.0, 1.0], 0.5, 0);
            nuevo.content = "ahora creo Y".into();
            nuevo.created_at = Utc::now();
            r.push(nuevo);
        }
        let (nuevos, obsoletos) = mem.changes_since(t0);
        assert!(nuevos.iter().any(|c| c.contains("Y")));
        assert!(obsoletos.iter().any(|c| c.contains("X")));
    }
}
