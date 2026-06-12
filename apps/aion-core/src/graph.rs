//! **Grafo de conocimiento de AION**: conceptos conectados sobre la Biblioteca y la memoria.
//!
//! Diseño "lazy" (LazyGraphRAG/LightRAG, jun 2026): la ingesta extrae conceptos de forma
//! DETERMINISTA (RAKE-like trilingüe es/en/it, sin LLM — los modelos locales 7-12B no
//! producen extracción estructurada fiable), las aristas nacen por co-ocurrencia
//! (`Extracted`) o similitud de embeddings (`Inferred`), y el LLM solo interviene en
//! idle/sueño para tipar relaciones y resumir comunidades. Los nodos NO duplican
//! contenido: puentean a los pasajes de la Biblioteca (`chunk_ids`) y a los recuerdos
//! (`memory_ids`). Persistencia JSONL (`graph.jsonl`) con escritura atómica.

use aion_memory::{cosine, OllamaEmbedder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Conceptos máximos extraídos por pasaje.
const TOP_CONCEPTS_PER_CHUNK: usize = 10;
/// Longitud máxima del label de un nodo (seguridad: un doc malicioso no puede
/// inyectar texto largo con forma de instrucción vía el grounding).
const MAX_LABEL_CHARS: usize = 64;
/// Frecuencia mínima (menciones en chunks del documento) para CREAR un nodo nuevo.
/// Conceptos de una sola aparición son casi siempre ruido de extracción.
const MIN_DOC_FREQ: u32 = 2;
/// Similitud de embedding a partir de la cual dos conceptos se FUNDEN como alias.
const MERGE_SIM: f32 = 0.92;
/// Zona dudosa: nodo propio + arista `Ambiguous` (la resuelve el refinador en idle).
const AMBIG_SIM: f32 = 0.84;
/// Similitud mínima para una arista `Inferred` entre conceptos de documentos distintos.
const INFER_SIM: f32 = 0.78;
/// Vecinos `Inferred` máximos por concepto nuevo.
const INFER_TOP: usize = 4;
/// Decaimiento de score por salto en la expansión multi-hop (espejo de la memoria).
const HOP_DECAY: f32 = 0.7;
/// Cap blando de nodos: por encima, la poda de sueño actúa y la ingesta avisa.
pub const SOFT_NODE_CAP: usize = 20_000;

// ---------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Concept,
    Source,
}

/// Etiqueta de confianza de una arista (patrón graphify): qué tan respaldada está.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Evidencia explícita: co-ocurrencia en un mismo pasaje.
    Extracted,
    /// Deducida por similitud de embeddings (sin co-ocurrencia textual).
    Inferred,
    /// Dudosa (p. ej. posible duplicado no fundido). El refinador idle la resuelve.
    Ambiguous,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// "c:<concepto normalizado>" o "s:<dominio>::<fuente>".
    pub id: String,
    pub kind: NodeKind,
    /// Forma de superficie (saneada, ≤64 chars).
    pub label: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// BGE-M3 1024-dim. Vacío en nodos `Source`.
    #[serde(default)]
    pub embedding: Vec<f32>,
    /// PUENTE a la Biblioteca: ids de pasaje "{dominio}::{fuente}#{idx}".
    #[serde(default)]
    pub chunk_ids: Vec<String>,
    /// PUENTE a la memoria personal: UUIDs de `MemoryRecord`.
    #[serde(default)]
    pub memory_ids: Vec<String>,
    /// Menciones (derivada: nº de chunks donde aparece).
    pub freq: u32,
    #[serde(default)]
    pub community: Option<u32>,
    /// Perfil corto estilo LightRAG (lo escribe el refinador en idle).
    #[serde(default)]
    pub summary: String,
    /// Epoch segundos (patrón `workspace::StreamEvent`).
    pub created_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Ids de nodo; NO dirigida, canónica `a < b`.
    pub a: String,
    pub b: String,
    /// "co-ocurre" al nacer; el refinador idle la tipa (causa, parte-de, …).
    pub rel: String,
    pub weight: f32,
    pub confidence: Confidence,
    /// Documentos "dominio::fuente" que la respaldan (vacío en `Inferred`).
    #[serde(default)]
    pub sources: Vec<String>,
    pub created_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Community {
    pub id: u32,
    pub label: String,
    pub summary: String,
    #[serde(default)]
    pub summary_embedding: Vec<f32>,
    pub size: usize,
    /// Aristas intra-comunidad / aristas totales de sus nodos (regla graphify).
    pub cohesion: f32,
}

/// Una línea de `graph.jsonl`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Row {
    Node(GraphNode),
    Edge(GraphEdge),
    Community(Community),
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct UpsertStats {
    pub concepts_new: usize,
    pub concepts_updated: usize,
    pub edges_extracted: usize,
    pub edges_inferred: usize,
    pub edges_ambiguous: usize,
}

/// Resultado del nivel LOCAL de consulta: un pasaje puenteado con su score y por
/// qué conceptos se llegó a él (para citar el camino).
pub struct LocalHit {
    pub chunk_id: String,
    pub score: f32,
    pub via: Vec<String>,
}

// ---------------------------------------------------------------------------
// Embedder inyectable (para tests sin red)
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait ConceptEmbedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
}

#[async_trait::async_trait]
impl ConceptEmbedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        OllamaEmbedder::embed(self, text)
            .await
            .map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Grafo
// ---------------------------------------------------------------------------

pub struct KnowledgeGraph {
    path: PathBuf,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    communities: Vec<Community>,
    /// id → índice en `nodes`.
    by_id: HashMap<String, usize>,
    /// índice de nodo → [(índice de arista, índice de vecino)].
    adj: HashMap<usize, Vec<(usize, usize)>>,
}

impl KnowledgeGraph {
    /// Abre (o crea) el grafo en la ruta dada. Carga todo en RAM (estilo `Library`).
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut g = Self {
            path,
            nodes: Vec::new(),
            edges: Vec::new(),
            communities: Vec::new(),
            by_id: HashMap::new(),
            adj: HashMap::new(),
        };
        if let Ok(text) = std::fs::read_to_string(&g.path) {
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                match serde_json::from_str::<Row>(line) {
                    Ok(Row::Node(n)) => g.nodes.push(n),
                    Ok(Row::Edge(e)) => g.edges.push(e),
                    Ok(Row::Community(c)) => g.communities.push(c),
                    Err(_) => {}
                }
            }
        }
        g.rebuild_index();
        g
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// (Usado por los tests; la API pública de conteo es `stats()`.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn nodes(&self) -> &[GraphNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[GraphEdge] {
        &self.edges
    }

    pub fn stats(&self) -> serde_json::Value {
        let concepts = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Concept)
            .count();
        let sources = self.nodes.len() - concepts;
        let by_conf = |c: Confidence| self.edges.iter().filter(|e| e.confidence == c).count();
        serde_json::json!({
            "nodes": self.nodes.len(),
            "concepts": concepts,
            "sources": sources,
            "edges": self.edges.len(),
            "extracted": by_conf(Confidence::Extracted),
            "inferred": by_conf(Confidence::Inferred),
            "ambiguous": by_conf(Confidence::Ambiguous),
            "typed": self.edges.iter().filter(|e| e.rel != "co-ocurre" && e.rel != "relacionado").count(),
            "communities": self.communities.len(),
        })
    }

    /// Ingesta incremental de un documento: borra lo que el doc aportó antes y
    /// re-extrae SOLO ese doc. Embebe únicamente conceptos NUEVOS únicos (sublineal
    /// con el corpus). `chunks` = (chunk_id "{dominio}::{fuente}#{idx}", contenido).
    pub async fn upsert_document(
        &mut self,
        domain: &str,
        source: &str,
        chunks: &[(String, String)],
        embedder: &dyn ConceptEmbedder,
    ) -> Result<UpsertStats, String> {
        let doc = format!("{domain}::{source}");
        self.remove_doc_inplace(domain, source);
        let now = chrono::Utc::now().timestamp();
        let mut stats = UpsertStats::default();

        // 1) Extracción determinista por chunk (sin LLM).
        let mut per_chunk: Vec<(String, Vec<ExtractedConcept>)> = Vec::new();
        let mut doc_freq: HashMap<String, u32> = HashMap::new();
        let mut surface: HashMap<String, String> = HashMap::new();
        for (chunk_id, content) in chunks {
            let cs = extract_concepts(content);
            for c in &cs {
                *doc_freq.entry(c.norm.clone()).or_insert(0) += c.count;
                surface
                    .entry(c.norm.clone())
                    .or_insert_with(|| c.label.clone());
            }
            per_chunk.push((chunk_id.clone(), cs));
        }

        // 2) Conceptos que entran: frecuencia mínima en el doc, o nodo ya existente
        //    (un concepto ya consolidado suma evidencia aunque aquí aparezca una vez).
        let kept: HashSet<String> = doc_freq
            .iter()
            .filter(|(norm, &f)| f >= MIN_DOC_FREQ || self.by_id.contains_key(&format!("c:{norm}")))
            .map(|(norm, _)| norm.clone())
            .collect();

        // 3) Alta/actualización de nodos. `resolved` mapea norma → id de nodo final
        //    (puede diferir del propio si se fundió como alias de otro).
        let mut resolved: HashMap<String, String> = HashMap::new();
        let mut new_ids: Vec<String> = Vec::new();
        for norm in &kept {
            let id = format!("c:{norm}");
            if let Some(&idx) = self.by_id.get(&id) {
                resolved.insert(norm.clone(), id);
                stats.concepts_updated += 1;
                let _ = idx;
                continue;
            }
            // ¿Alias ya fundido en otro nodo?
            if let Some(owner) = self.alias_owner(norm) {
                resolved.insert(norm.clone(), owner);
                stats.concepts_updated += 1;
                continue;
            }
            // Nodo potencialmente nuevo: embeber y deduplicar por embedding (LightRAG).
            let label = sanitize_label(surface.get(norm).unwrap_or(norm));
            let emb = embedder.embed(norm).await?;
            let mut best: Option<(f32, usize)> = None;
            for (i, n) in self.nodes.iter().enumerate() {
                if n.kind != NodeKind::Concept || n.embedding.is_empty() {
                    continue;
                }
                let s = cosine(&emb, &n.embedding);
                if best.is_none_or(|(bs, _)| s > bs) {
                    best = Some((s, i));
                }
            }
            match best {
                // Mismo concepto con otra superficie → alias, sin nodo nuevo.
                Some((s, i)) if s >= MERGE_SIM => {
                    let owner = self.nodes[i].id.clone();
                    if !self.nodes[i].aliases.iter().any(|a| a == norm) {
                        self.nodes[i].aliases.push(norm.clone());
                    }
                    resolved.insert(norm.clone(), owner);
                    stats.concepts_updated += 1;
                }
                // Zona dudosa → nodo propio + arista Ambiguous (la resuelve el idle).
                Some((s, i)) if s >= AMBIG_SIM => {
                    let other = self.nodes[i].id.clone();
                    self.push_node(GraphNode {
                        id: id.clone(),
                        kind: NodeKind::Concept,
                        label,
                        aliases: Vec::new(),
                        embedding: emb,
                        chunk_ids: Vec::new(),
                        memory_ids: Vec::new(),
                        freq: 0,
                        community: None,
                        summary: String::new(),
                        created_at: now,
                    });
                    self.push_edge(GraphEdge {
                        a: id.clone().min(other.clone()),
                        b: id.clone().max(other),
                        rel: "co-ocurre".into(),
                        weight: s,
                        confidence: Confidence::Ambiguous,
                        sources: vec![doc.clone()],
                        created_at: now,
                    });
                    resolved.insert(norm.clone(), id.clone());
                    new_ids.push(id);
                    stats.concepts_new += 1;
                    stats.edges_ambiguous += 1;
                }
                _ => {
                    self.push_node(GraphNode {
                        id: id.clone(),
                        kind: NodeKind::Concept,
                        label,
                        aliases: Vec::new(),
                        embedding: emb,
                        chunk_ids: Vec::new(),
                        memory_ids: Vec::new(),
                        freq: 0,
                        community: None,
                        summary: String::new(),
                        created_at: now,
                    });
                    resolved.insert(norm.clone(), id.clone());
                    new_ids.push(id);
                    stats.concepts_new += 1;
                }
            }
        }

        // 4) Puentes a chunks + co-ocurrencia (aristas Extracted).
        let mut co: HashMap<(String, String), u32> = HashMap::new();
        for (chunk_id, cs) in &per_chunk {
            let mut ids: Vec<String> = cs
                .iter()
                .filter_map(|c| resolved.get(&c.norm).cloned())
                .collect();
            ids.sort();
            ids.dedup();
            for nid in &ids {
                if let Some(&i) = self.by_id.get(nid) {
                    if !self.nodes[i].chunk_ids.iter().any(|c| c == chunk_id) {
                        self.nodes[i].chunk_ids.push(chunk_id.clone());
                    }
                }
            }
            for x in 0..ids.len() {
                for y in (x + 1)..ids.len() {
                    *co.entry((ids[x].clone(), ids[y].clone())).or_insert(0) += 1;
                }
            }
        }
        let max_co = co.values().copied().max().unwrap_or(1) as f32;
        for ((a, b), n) in co {
            let w = n as f32 / max_co;
            if let Some(e) = self
                .edges
                .iter_mut()
                .find(|e| e.a == a && e.b == b && e.confidence != Confidence::Ambiguous)
            {
                e.weight = e.weight.max(w);
                e.confidence = Confidence::Extracted;
                if !e.sources.iter().any(|s| s == &doc) {
                    e.sources.push(doc.clone());
                }
            } else {
                self.push_edge(GraphEdge {
                    a,
                    b,
                    rel: "co-ocurre".into(),
                    weight: w,
                    confidence: Confidence::Extracted,
                    sources: vec![doc.clone()],
                    created_at: now,
                });
                stats.edges_extracted += 1;
            }
        }

        // 5) Aristas Inferred: conceptos NUEVOS ↔ corpus existente por embedding.
        for nid in &new_ids {
            let Some(&ni) = self.by_id.get(nid) else {
                continue;
            };
            let emb = self.nodes[ni].embedding.clone();
            let mut sims: Vec<(f32, String)> = self
                .nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Concept && n.id != *nid && !n.embedding.is_empty())
                .map(|n| (cosine(&emb, &n.embedding), n.id.clone()))
                .filter(|(s, _)| *s >= INFER_SIM && *s < MERGE_SIM)
                .collect();
            sims.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            for (s, other) in sims.into_iter().take(INFER_TOP) {
                let (a, b) = if *nid < other {
                    (nid.clone(), other)
                } else {
                    (other, nid.clone())
                };
                if self.edges.iter().any(|e| e.a == a && e.b == b) {
                    continue;
                }
                self.push_edge(GraphEdge {
                    a,
                    b,
                    rel: "relacionado".into(),
                    weight: s,
                    confidence: Confidence::Inferred,
                    sources: Vec::new(),
                    created_at: now,
                });
                stats.edges_inferred += 1;
            }
        }

        // 6) Nodo Source del documento + frecuencias derivadas.
        let sid = format!("s:{doc}");
        self.push_node(GraphNode {
            id: sid,
            kind: NodeKind::Source,
            label: sanitize_label(source),
            aliases: Vec::new(),
            embedding: Vec::new(),
            chunk_ids: chunks.iter().map(|(id, _)| id.clone()).collect(),
            memory_ids: Vec::new(),
            freq: chunks.len() as u32,
            community: None,
            summary: String::new(),
            created_at: now,
        });
        self.recompute_freq();
        if self.nodes.len() > SOFT_NODE_CAP {
            tracing::warn!(
                nodes = self.nodes.len(),
                "grafo por encima del cap blando; la poda de sueño actuará"
            );
        }
        self.persist()?;
        Ok(stats)
    }

    /// Elimina lo que un documento aportó al grafo. Devuelve nodos eliminados.
    pub fn remove_document(&mut self, domain: &str, source: &str) -> Result<usize, String> {
        let removed = self.remove_doc_inplace(domain, source);
        if removed > 0 {
            self.persist()?;
        }
        Ok(removed)
    }

    fn remove_doc_inplace(&mut self, domain: &str, source: &str) -> usize {
        let doc = format!("{domain}::{source}");
        let prefix = format!("{doc}#");
        let sid = format!("s:{doc}");
        for n in &mut self.nodes {
            n.chunk_ids.retain(|c| !c.starts_with(&prefix));
        }
        for e in &mut self.edges {
            e.sources.retain(|s| s != &doc);
        }
        // Mueren: el nodo Source del doc y los conceptos que se quedan sin evidencia.
        let dead: HashSet<String> = self
            .nodes
            .iter()
            .filter(|n| {
                n.id == sid
                    || (n.kind == NodeKind::Concept
                        && n.chunk_ids.is_empty()
                        && n.memory_ids.is_empty())
            })
            .map(|n| n.id.clone())
            .collect();
        if dead.is_empty()
            && !self
                .edges
                .iter()
                .any(|e| e.confidence == Confidence::Extracted && e.sources.is_empty())
        {
            return 0;
        }
        let before = self.nodes.len();
        self.nodes.retain(|n| !dead.contains(&n.id));
        self.edges.retain(|e| {
            // Una Extracted sin documentos que la respalden ya no tiene evidencia.
            let sin_evidencia = e.confidence == Confidence::Extracted && e.sources.is_empty();
            !(dead.contains(&e.a) || dead.contains(&e.b) || sin_evidencia)
        });
        self.recompute_freq();
        self.rebuild_index();
        before - self.nodes.len()
    }

    /// Nivel LOCAL de consulta (<1 ms en RAM): conceptos semilla por
    /// `0.7·coseno + 0.3·solape léxico`, expansión multi-hop con decaimiento, y
    /// pasajes puenteados puntuados por el mejor concepto que llega a ellos.
    pub fn local_candidates(
        &self,
        q_emb: &[f32],
        q_text: &str,
        k: usize,
        hops: usize,
    ) -> Vec<LocalHit> {
        let q_words: HashSet<String> = q_text
            .to_lowercase()
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() >= 3 && !is_stopword(w))
            .collect();
        let mut scored: Vec<(f32, usize)> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == NodeKind::Concept && !n.embedding.is_empty())
            .map(|(i, n)| {
                let sem = cosine(q_emb, &n.embedding);
                let cw: Vec<&str> = n.label.split_whitespace().collect();
                let lex = if cw.is_empty() || q_words.is_empty() {
                    0.0
                } else {
                    cw.iter()
                        .filter(|w| q_words.contains(&w.to_lowercase()))
                        .count() as f32
                        / cw.len() as f32
                };
                (0.7 * sem + 0.3 * lex, i)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k.max(1));

        // Expansión multi-hop: score del vecino = score origen × decay × peso de arista.
        let mut best: HashMap<usize, (f32, Vec<String>)> = HashMap::new();
        for &(s, i) in &scored {
            best.insert(i, (s, vec![self.nodes[i].label.clone()]));
        }
        let mut frontier: Vec<usize> = scored.iter().map(|&(_, i)| i).collect();
        for _ in 0..hops {
            let mut next = Vec::new();
            for &i in &frontier {
                let (base, via) = best.get(&i).cloned().unwrap_or((0.0, Vec::new()));
                for &(ei, j) in self.adj.get(&i).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if self.nodes[j].kind != NodeKind::Concept {
                        continue;
                    }
                    let s = base * HOP_DECAY * self.edges[ei].weight.clamp(0.1, 1.0);
                    let entry = best.entry(j).or_insert((0.0, Vec::new()));
                    if s > entry.0 {
                        let mut v = via.clone();
                        v.push(self.nodes[j].label.clone());
                        *entry = (s, v);
                        next.push(j);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }

        // Pasajes puenteados: cada chunk hereda el mejor score de sus conceptos.
        let mut chunks: HashMap<String, (f32, Vec<String>)> = HashMap::new();
        for (i, (s, via)) in &best {
            for c in &self.nodes[*i].chunk_ids {
                let entry = chunks.entry(c.clone()).or_insert((0.0, Vec::new()));
                if *s > entry.0 {
                    *entry = (*s, via.clone());
                }
            }
        }
        let mut out: Vec<LocalHit> = chunks
            .into_iter()
            .map(|(chunk_id, (score, via))| LocalHit {
                chunk_id,
                score,
                via,
            })
            .collect();
        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// Nivel GLOBAL de consulta: comunidades por coseno con su resumen embebido.
    pub fn global_candidates(&self, q_emb: &[f32], k: usize) -> Vec<(f32, &Community)> {
        let mut scored: Vec<(f32, &Community)> = self
            .communities
            .iter()
            .filter(|c| !c.summary_embedding.is_empty() && !c.summary.is_empty())
            .map(|c| (cosine(q_emb, &c.summary_embedding), c))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k.max(1));
        scored
    }

    // -- comunidades (Label Propagation ponderada + regla de split de graphify) ----

    /// ¿Hace falta (re)detectar comunidades? Sí cuando no hay ninguna con aristas
    /// presentes, o cuando >15 % de los conceptos conectados quedaron sin asignar
    /// (el grafo creció desde la última detección).
    pub fn communities_stale(&self) -> bool {
        if self.edges.is_empty() {
            return false;
        }
        if self.communities.is_empty() {
            return true;
        }
        let connected: Vec<usize> = (0..self.nodes.len())
            .filter(|i| {
                self.nodes[*i].kind == NodeKind::Concept
                    && self.adj.get(i).is_some_and(|v| !v.is_empty())
            })
            .collect();
        if connected.is_empty() {
            return false;
        }
        let unassigned = connected
            .iter()
            .filter(|i| self.nodes[**i].community.is_none())
            .count();
        unassigned * 100 > connected.len() * 15
    }

    /// Detecta comunidades temáticas por **Label Propagation ponderada** (determinista:
    /// orden por id, desempate por etiqueta menor, máx 20 iteraciones — converge en ms
    /// con miles de nodos). Comunidades gigantes (>25 % del grafo, ≥10 nodos) se parten
    /// re-propagando sobre su subgrafo con solo las aristas sobre la mediana de peso
    /// (regla de graphify). Asigna `node.community`, calcula cohesión y persiste.
    /// Devuelve el nº de comunidades.
    pub fn detect_communities(&mut self) -> usize {
        let concepts: Vec<usize> = (0..self.nodes.len())
            .filter(|i| {
                self.nodes[*i].kind == NodeKind::Concept
                    && self.adj.get(i).is_some_and(|v| !v.is_empty())
            })
            .collect();
        if concepts.is_empty() {
            self.communities.clear();
            return 0;
        }
        let mut labels = self.label_propagation(&concepts, 0.0);

        // Split de comunidades gigantes.
        let total = concepts.len();
        let mut groups: HashMap<u32, Vec<usize>> = HashMap::new();
        for (&i, &l) in &labels {
            groups.entry(l).or_default().push(i);
        }
        let mut next_label = labels.values().copied().max().unwrap_or(0) + 1;
        for (_, members) in groups.clone() {
            if members.len() < 10 || members.len() * 4 <= total {
                continue;
            }
            let mut weights: Vec<f32> = Vec::new();
            let mset: HashSet<usize> = members.iter().copied().collect();
            for &i in &members {
                for &(ei, j) in self.adj.get(&i).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if mset.contains(&j) {
                        weights.push(self.edges[ei].weight);
                    }
                }
            }
            weights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let median = weights.get(weights.len() / 2).copied().unwrap_or(0.0);
            let sub = self.label_propagation(&members, median);
            let distinct: HashSet<u32> = sub.values().copied().collect();
            if distinct.len() <= 1 {
                continue; // el split no separó nada: se conserva la original
            }
            let mut remap: HashMap<u32, u32> = HashMap::new();
            for (&i, &sl) in &sub {
                let nl = *remap.entry(sl).or_insert_with(|| {
                    let l = next_label;
                    next_label += 1;
                    l
                });
                labels.insert(i, nl);
            }
        }

        // Reindexa por tamaño (desc) y asigna a los nodos.
        let mut groups: HashMap<u32, Vec<usize>> = HashMap::new();
        for (&i, &l) in &labels {
            groups.entry(l).or_default().push(i);
        }
        let mut ordered: Vec<(u32, Vec<usize>)> = groups.into_iter().collect();
        ordered.sort_by_key(|(_, m)| std::cmp::Reverse(m.len()));
        for n in &mut self.nodes {
            n.community = None;
        }
        let old: Vec<Community> = std::mem::take(&mut self.communities);
        for (new_id, (_, members)) in ordered.iter().enumerate() {
            let new_id = new_id as u32;
            let mset: HashSet<usize> = members.iter().copied().collect();
            let mut intra = 0usize;
            let mut touching = 0usize;
            for (ei, e) in self.edges.iter().enumerate() {
                let _ = ei;
                let (Some(&ia), Some(&ib)) = (self.by_id.get(&e.a), self.by_id.get(&e.b)) else {
                    continue;
                };
                let a_in = mset.contains(&ia);
                let b_in = mset.contains(&ib);
                if a_in || b_in {
                    touching += 1;
                }
                if a_in && b_in {
                    intra += 1;
                }
            }
            // Etiqueta humana: los 3 conceptos más frecuentes del grupo.
            let mut by_freq: Vec<usize> = members.clone();
            by_freq.sort_by_key(|&i| std::cmp::Reverse(self.nodes[i].freq));
            let label = by_freq
                .iter()
                .take(3)
                .map(|&i| self.nodes[i].label.clone())
                .collect::<Vec<_>>()
                .join(" · ");
            // Si la comunidad conserva EXACTAMENTE los mismos miembros que una vieja,
            // hereda su resumen (no se tira trabajo del refinador).
            let member_ids: HashSet<&str> =
                members.iter().map(|&i| self.nodes[i].id.as_str()).collect();
            let inherited = old.iter().find(|c| {
                c.size == members.len() && {
                    let assigned: HashSet<&str> = self
                        .nodes
                        .iter()
                        .filter(|n| n.community == Some(c.id))
                        .map(|n| n.id.as_str())
                        .collect();
                    assigned.is_empty() || assigned == member_ids
                }
            });
            for &i in members {
                self.nodes[i].community = Some(new_id);
            }
            self.communities.push(Community {
                id: new_id,
                label,
                summary: inherited.map(|c| c.summary.clone()).unwrap_or_default(),
                summary_embedding: inherited
                    .map(|c| c.summary_embedding.clone())
                    .unwrap_or_default(),
                size: members.len(),
                cohesion: if touching == 0 {
                    0.0
                } else {
                    (intra as f32 / touching as f32 * 100.0).round() / 100.0
                },
            });
        }
        let _ = self.persist();
        self.communities.len()
    }

    /// LPA ponderada sobre un subconjunto de nodos, ignorando aristas bajo `min_weight`.
    fn label_propagation(&self, members: &[usize], min_weight: f32) -> HashMap<usize, u32> {
        let mset: HashSet<usize> = members.iter().copied().collect();
        let mut order: Vec<usize> = members.to_vec();
        order.sort_by(|&a, &b| self.nodes[a].id.cmp(&self.nodes[b].id));
        let mut labels: HashMap<usize, u32> = order
            .iter()
            .enumerate()
            .map(|(pos, &i)| (i, pos as u32))
            .collect();
        for _ in 0..20 {
            let mut changed = false;
            for &i in &order {
                let mut tally: std::collections::BTreeMap<u32, f32> =
                    std::collections::BTreeMap::new();
                for &(ei, j) in self.adj.get(&i).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if !mset.contains(&j) || self.edges[ei].weight < min_weight {
                        continue;
                    }
                    *tally.entry(labels[&j]).or_insert(0.0) += self.edges[ei].weight.max(0.05);
                }
                // Mejor etiqueta: mayor peso; a igual peso, la MENOR (determinista).
                let Some(best) = tally
                    .iter()
                    .max_by(|a, b| {
                        a.1.partial_cmp(b.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(b.0.cmp(a.0))
                    })
                    .map(|(&l, _)| l)
                else {
                    continue;
                };
                if labels[&i] != best {
                    labels.insert(i, best);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        labels
    }

    // -- soporte del refinador idle/sueño (LLM con vocabulario cerrado) -------------

    /// Pares de nodos unidos por arista `Ambiguous` (posibles duplicados), por peso
    /// desc: (id_a, id_b, label_a, label_b).
    pub fn ambiguous_pairs(&self, n: usize) -> Vec<(String, String, String, String)> {
        let mut v: Vec<&GraphEdge> = self
            .edges
            .iter()
            .filter(|e| e.confidence == Confidence::Ambiguous)
            .collect();
        v.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v.into_iter()
            .take(n)
            .filter_map(|e| {
                let la = self.by_id.get(&e.a).map(|&i| self.nodes[i].label.clone())?;
                let lb = self.by_id.get(&e.b).map(|&i| self.nodes[i].label.clone())?;
                Some((e.a.clone(), e.b.clone(), la, lb))
            })
            .collect()
    }

    /// Aristas `Extracted` aún sin tipar ("co-ocurre"), por peso desc, con un chunk
    /// COMPARTIDO por ambos conceptos como evidencia: (a, b, label_a, label_b, chunk_id).
    pub fn top_untyped(&self, n: usize) -> Vec<(String, String, String, String, Option<String>)> {
        let mut v: Vec<&GraphEdge> = self
            .edges
            .iter()
            .filter(|e| e.confidence == Confidence::Extracted && e.rel == "co-ocurre")
            .collect();
        v.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v.into_iter()
            .take(n)
            .filter_map(|e| {
                let &ia = self.by_id.get(&e.a)?;
                let &ib = self.by_id.get(&e.b)?;
                let shared = self.nodes[ia]
                    .chunk_ids
                    .iter()
                    .find(|c| self.nodes[ib].chunk_ids.contains(c))
                    .cloned();
                Some((
                    e.a.clone(),
                    e.b.clone(),
                    self.nodes[ia].label.clone(),
                    self.nodes[ib].label.clone(),
                    shared,
                ))
            })
            .collect()
    }

    /// Tipa la relación de una arista (vocabulario cerrado, validado por el llamador).
    pub fn set_edge_rel(&mut self, a: &str, b: &str, rel: &str) {
        if let Some(e) = self.edges.iter_mut().find(|e| e.a == a && e.b == b) {
            e.rel = rel.to_string();
        }
    }

    /// Resuelve una arista `Ambiguous`: si `same`, los nodos se FUNDEN (el de menor
    /// frecuencia pasa a alias del otro); si no, la arista baja a `Inferred`.
    pub fn resolve_ambiguous(&mut self, a: &str, b: &str, same: bool) {
        if same {
            let (Some(&ia), Some(&ib)) = (self.by_id.get(a), self.by_id.get(b)) else {
                return;
            };
            let (keep, drop) = if self.nodes[ia].freq >= self.nodes[ib].freq {
                (a.to_string(), b.to_string())
            } else {
                (b.to_string(), a.to_string())
            };
            self.merge_nodes(&keep, &drop);
        } else if let Some(e) = self
            .edges
            .iter_mut()
            .find(|e| e.a == a && e.b == b && e.confidence == Confidence::Ambiguous)
        {
            e.confidence = Confidence::Inferred;
            e.rel = "relacionado".into();
        }
    }

    /// Funde `drop` dentro de `keep`: aliases + puentes se unen, las aristas de `drop`
    /// se redirigen a `keep` (sin auto-bucles ni duplicados) y `drop` desaparece.
    pub fn merge_nodes(&mut self, keep: &str, drop: &str) {
        let (Some(&ik), Some(&id_)) = (self.by_id.get(keep), self.by_id.get(drop)) else {
            return;
        };
        if ik == id_ {
            return;
        }
        let dropped = self.nodes[id_].clone();
        let k = &mut self.nodes[ik];
        let drop_norm = dropped
            .id
            .strip_prefix("c:")
            .unwrap_or(&dropped.id)
            .to_string();
        if !k.aliases.contains(&drop_norm) {
            k.aliases.push(drop_norm);
        }
        for a in dropped.aliases {
            if !k.aliases.contains(&a) {
                k.aliases.push(a);
            }
        }
        for c in dropped.chunk_ids {
            if !k.chunk_ids.contains(&c) {
                k.chunk_ids.push(c);
            }
        }
        for m in dropped.memory_ids {
            if !k.memory_ids.contains(&m) {
                k.memory_ids.push(m);
            }
        }
        // Redirige aristas y elimina auto-bucles/duplicados.
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut kept_edges: Vec<GraphEdge> = Vec::new();
        for mut e in std::mem::take(&mut self.edges) {
            if e.a == drop {
                e.a = keep.to_string();
            }
            if e.b == drop {
                e.b = keep.to_string();
            }
            if e.a == e.b {
                continue;
            }
            if e.a > e.b {
                std::mem::swap(&mut e.a, &mut e.b);
            }
            let key = (e.a.clone(), e.b.clone());
            if let Some(prev) = kept_edges.iter_mut().find(|p| p.a == key.0 && p.b == key.1) {
                prev.weight = prev.weight.max(e.weight);
                for s in e.sources {
                    if !prev.sources.contains(&s) {
                        prev.sources.push(s);
                    }
                }
                continue;
            }
            seen.insert(key);
            kept_edges.push(e);
        }
        self.edges = kept_edges;
        self.nodes.retain(|n| n.id != drop);
        self.recompute_freq();
        self.rebuild_index();
    }

    /// Comunidades sin resumen (por tamaño desc): (id, etiqueta, top labels, chunks
    /// de muestra de sus miembros más frecuentes).
    pub fn communities_needing_summary(
        &self,
        n: usize,
    ) -> Vec<(u32, String, Vec<String>, Vec<String>)> {
        let mut v: Vec<&Community> = self
            .communities
            .iter()
            .filter(|c| c.summary.is_empty() && c.size >= 3)
            .collect();
        v.sort_by_key(|c| std::cmp::Reverse(c.size));
        v.into_iter()
            .take(n)
            .map(|c| {
                let mut members: Vec<&GraphNode> = self
                    .nodes
                    .iter()
                    .filter(|nd| nd.community == Some(c.id))
                    .collect();
                members.sort_by_key(|nd| std::cmp::Reverse(nd.freq));
                let labels: Vec<String> =
                    members.iter().take(5).map(|nd| nd.label.clone()).collect();
                let chunks: Vec<String> = members
                    .iter()
                    .flat_map(|nd| nd.chunk_ids.first().cloned())
                    .take(2)
                    .collect();
                (c.id, c.label.clone(), labels, chunks)
            })
            .collect()
    }

    /// Guarda el resumen (saneado) y su embedding para el retrieval global.
    pub fn set_community_summary(&mut self, id: u32, summary: &str, embedding: Vec<f32>) {
        if let Some(c) = self.communities.iter_mut().find(|c| c.id == id) {
            let clean: String = summary
                .chars()
                .filter(|ch| !ch.is_control() && *ch != '<' && *ch != '>')
                .take(400)
                .collect();
            c.summary = clean.split_whitespace().collect::<Vec<_>>().join(" ");
            c.summary_embedding = embedding;
        }
    }

    /// Puente memoria→grafo: añade `memory_ids` a conceptos YA existentes que aparecen
    /// en recuerdos recientes (conservador: jamás crea conceptos desde la memoria).
    pub fn attach_memories(&mut self, items: &[(String, String)]) -> usize {
        let mut added = 0usize;
        for (mid, content) in items {
            for c in extract_concepts(content) {
                let id = format!("c:{}", c.norm);
                let target = if self.by_id.contains_key(&id) {
                    Some(id)
                } else {
                    self.alias_owner(&c.norm)
                };
                if let Some(t) = target {
                    if let Some(&i) = self.by_id.get(&t) {
                        if !self.nodes[i].memory_ids.contains(mid) {
                            self.nodes[i].memory_ids.push(mid.clone());
                            added += 1;
                        }
                    }
                }
            }
        }
        if added > 0 {
            self.recompute_freq();
        }
        added
    }

    /// Poda darwiniana ligera (espejo del `consolidate` de la memoria): fuera conceptos
    /// sin aristas, con una sola mención y de más de 7 días; y si el grafo supera el
    /// cap blando, caen los de menor frecuencia. Devuelve cuántos nodos se podaron.
    pub fn prune_weak(&mut self) -> usize {
        let now = chrono::Utc::now().timestamp();
        let week = 7 * 24 * 3600;
        let before = self.nodes.len();
        let connected: HashSet<String> = self
            .edges
            .iter()
            .flat_map(|e| [e.a.clone(), e.b.clone()])
            .collect();
        self.nodes.retain(|n| {
            n.kind != NodeKind::Concept
                || n.freq > 1
                || connected.contains(&n.id)
                || now - n.created_at < week
        });
        if self.nodes.len() > SOFT_NODE_CAP {
            let mut concepts: Vec<(u32, String)> = self
                .nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Concept)
                .map(|n| (n.freq, n.id.clone()))
                .collect();
            concepts.sort();
            let excess = self.nodes.len() - SOFT_NODE_CAP;
            let dead: HashSet<String> = concepts
                .into_iter()
                .take(excess)
                .map(|(_, id)| id)
                .collect();
            self.nodes.retain(|n| !dead.contains(&n.id));
            self.edges
                .retain(|e| !dead.contains(&e.a) && !dead.contains(&e.b));
        }
        let alive: HashSet<String> = self.nodes.iter().map(|n| n.id.clone()).collect();
        self.edges
            .retain(|e| alive.contains(&e.a) && alive.contains(&e.b));
        self.rebuild_index();
        before - self.nodes.len()
    }

    /// Persiste el estado actual (para el refinador, que muta con varios métodos).
    pub fn save(&self) -> Result<(), String> {
        self.persist()
    }

    /// Vista para la UI (página Mente): top nodos por frecuencia + las aristas entre
    /// ellos + comunidades. Limitada SIEMPRE (un grafo de 20k nodos no se pinta).
    pub fn export_view(&self, max_nodes: usize) -> serde_json::Value {
        let mut idx: Vec<usize> = (0..self.nodes.len()).collect();
        idx.sort_by_key(|&i| std::cmp::Reverse(self.nodes[i].freq));
        idx.truncate(max_nodes.max(1));
        let keep: HashSet<&str> = idx.iter().map(|&i| self.nodes[i].id.as_str()).collect();
        let nodes: Vec<serde_json::Value> = idx
            .iter()
            .map(|&i| {
                let n = &self.nodes[i];
                serde_json::json!({
                    "id": n.id,
                    "label": n.label,
                    "kind": n.kind,
                    "freq": n.freq,
                    "community": n.community,
                    "summary": n.summary,
                })
            })
            .collect();
        let edges: Vec<serde_json::Value> = self
            .edges
            .iter()
            .filter(|e| keep.contains(e.a.as_str()) && keep.contains(e.b.as_str()))
            .map(|e| {
                serde_json::json!({
                    "a": e.a, "b": e.b, "rel": e.rel,
                    "weight": e.weight, "confidence": e.confidence,
                })
            })
            .collect();
        let communities: Vec<serde_json::Value> = self
            .communities
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id, "label": c.label, "summary": c.summary,
                    "size": c.size, "cohesion": c.cohesion,
                })
            })
            .collect();
        serde_json::json!({ "nodes": nodes, "edges": edges, "communities": communities })
    }

    // -- internos ------------------------------------------------------------

    fn alias_owner(&self, norm: &str) -> Option<String> {
        self.nodes
            .iter()
            .find(|n| n.kind == NodeKind::Concept && n.aliases.iter().any(|a| a == norm))
            .map(|n| n.id.clone())
    }

    fn push_node(&mut self, n: GraphNode) {
        if self.by_id.contains_key(&n.id) {
            return;
        }
        self.by_id.insert(n.id.clone(), self.nodes.len());
        self.nodes.push(n);
    }

    fn push_edge(&mut self, e: GraphEdge) {
        let (Some(&ia), Some(&ib)) = (self.by_id.get(&e.a), self.by_id.get(&e.b)) else {
            return;
        };
        let ei = self.edges.len();
        self.adj.entry(ia).or_default().push((ei, ib));
        self.adj.entry(ib).or_default().push((ei, ia));
        self.edges.push(e);
    }

    fn recompute_freq(&mut self) {
        for n in &mut self.nodes {
            if n.kind == NodeKind::Concept {
                n.freq = (n.chunk_ids.len() + n.memory_ids.len()) as u32;
            }
        }
    }

    fn rebuild_index(&mut self) {
        self.by_id = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();
        self.adj.clear();
        for (ei, e) in self.edges.iter().enumerate() {
            if let (Some(&ia), Some(&ib)) = (self.by_id.get(&e.a), self.by_id.get(&e.b)) {
                self.adj.entry(ia).or_default().push((ei, ib));
                self.adj.entry(ib).or_default().push((ei, ia));
            }
        }
    }

    fn persist(&self) -> Result<(), String> {
        let mut out = String::new();
        for n in &self.nodes {
            if let Ok(l) = serde_json::to_string(&Row::Node(n.clone())) {
                out.push_str(&l);
                out.push('\n');
            }
        }
        for e in &self.edges {
            if let Ok(l) = serde_json::to_string(&Row::Edge(e.clone())) {
                out.push_str(&l);
                out.push('\n');
            }
        }
        for c in &self.communities {
            if let Ok(l) = serde_json::to_string(&Row::Community(c.clone())) {
                out.push_str(&l);
                out.push('\n');
            }
        }
        crate::write_atomic(&self.path, &out);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Extracción determinista de conceptos (RAKE-like trilingüe, sin LLM)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ExtractedConcept {
    /// Forma normalizada (clave de dedup).
    pub norm: String,
    /// Forma de superficie de la primera aparición (para el label).
    pub label: String,
    pub score: f32,
    /// Ocurrencias en el texto (alimenta la frecuencia mínima del documento).
    pub count: u32,
}

/// Longitud máxima (palabras) de una racha de no-stopwords y de un n-grama concepto.
const MAX_RUN_WORDS: usize = 8;
const MAX_NGRAM_WORDS: usize = 4;

/// Extrae los conceptos más informativos de un texto, sin LLM:
/// 1) corta el texto en rachas de no-stopwords (frontera = stopword/puntuación);
/// 2) genera TODOS los sub-n-gramas contiguos de 1-4 palabras y cuenta ocurrencias —
///    así "proyecto vega" emerge de "proyecto Vega usa" aunque nunca esté aislado;
/// 3) puntúa con RAKE (`Σ deg(w)/freq(w)`) × `(1+ln(ocurrencias))`, boost ×1.5 a los
///    capitalizados (nombres propios);
/// 4) descarta los n-gramas SUBSUMIDOS por uno más largo con las mismas ocurrencias
///    (se queda la forma más específica). Devuelve top 10.
pub fn extract_concepts(text: &str) -> Vec<ExtractedConcept> {
    // 1) Rachas de no-stopwords.
    let mut runs: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for raw in text.split_whitespace() {
        let trimmed = raw.trim_matches(|c: char| !c.is_alphanumeric());
        let boundary_after = raw
            .trim_end()
            .ends_with(['.', ',', ';', ':', '!', '?', ')', ']', '»', '"']);
        let lw = trimmed.to_lowercase();
        let ok =
            trimmed.len() >= 2 && !is_stopword(&lw) && !trimmed.chars().all(|c| c.is_ascii_digit());
        if ok {
            current.push(trimmed);
        }
        if (!ok || boundary_after || current.len() >= MAX_RUN_WORDS) && !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }

    // 2) Scores RAKE por palabra (freq y deg sobre las rachas).
    let mut freq: HashMap<String, f32> = HashMap::new();
    let mut deg: HashMap<String, f32> = HashMap::new();
    for run in &runs {
        for w in run {
            let lw = w.to_lowercase();
            *freq.entry(lw.clone()).or_insert(0.0) += 1.0;
            *deg.entry(lw).or_insert(0.0) += run.len() as f32;
        }
    }

    // 3) Sub-n-gramas con conteo de ocurrencias + mejor superficie + capitalización.
    struct Acc {
        label: String,
        count: u32,
        capitalized: bool,
        words: usize,
        rake: f32,
    }
    let mut by_norm: HashMap<String, Acc> = HashMap::new();
    for run in &runs {
        for len in 1..=MAX_NGRAM_WORDS.min(run.len()) {
            for start in 0..=(run.len() - len) {
                let gram = &run[start..start + len];
                let surface = gram.join(" ");
                if surface.len() < 3 || surface.len() > MAX_LABEL_CHARS {
                    continue;
                }
                let norm = normalize_concept(&surface);
                if norm.len() < 3 {
                    continue;
                }
                let rake: f32 = gram
                    .iter()
                    .map(|w| {
                        let lw = w.to_lowercase();
                        deg.get(&lw).copied().unwrap_or(0.0) / freq.get(&lw).copied().unwrap_or(1.0)
                    })
                    .sum();
                let caps = gram
                    .iter()
                    .all(|w| w.chars().next().is_some_and(|c| c.is_uppercase()));
                let e = by_norm.entry(norm).or_insert(Acc {
                    label: surface,
                    count: 0,
                    capitalized: false,
                    words: len,
                    rake,
                });
                e.count += 1;
                e.capitalized |= caps;
                if rake > e.rake {
                    e.rake = rake;
                }
            }
        }
    }

    // 4) Subsunción: fuera los n-gramas contenidos en otro más largo con >= ocurrencias
    //    (p. ej. "vega" cede ante "proyecto vega" si siempre aparece dentro de él).
    let mut items: Vec<(String, Acc)> = by_norm.into_iter().collect();
    let keys: Vec<(String, usize, u32)> = items
        .iter()
        .map(|(n, a)| (format!(" {n} "), a.words, a.count))
        .collect();
    items.retain(|(norm, acc)| {
        let padded = format!(" {norm} ");
        !keys
            .iter()
            .any(|(other, ow, oc)| *ow > acc.words && *oc >= acc.count && other.contains(&padded))
    });

    let mut out: Vec<ExtractedConcept> = items
        .into_iter()
        .map(|(norm, a)| {
            let mut score = a.rake * (1.0 + (a.count as f32).ln());
            if a.capitalized {
                score *= 1.5;
            }
            ExtractedConcept {
                norm,
                label: a.label,
                score,
                count: a.count,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(TOP_CONCEPTS_PER_CHUNK);
    out
}

/// Normaliza un concepto: minúsculas, espacios colapsados y singular CONSERVADOR
/// (quita la `s` final de la última palabra si len>4 y no termina en is/us/es —
/// suficiente para es/en/it sin lematizador; los plurales en -es los funde el
/// dedup por embedding).
pub fn normalize_concept(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let mut words: Vec<String> = lower.split_whitespace().map(|s| s.to_string()).collect();
    if let Some(last) = words.last_mut() {
        if last.len() > 4
            && last.ends_with('s')
            && !last.ends_with("is")
            && !last.ends_with("us")
            && !last.ends_with("es")
        {
            last.pop();
        }
    }
    words.join(" ")
}

/// Sanea un label antes de que entre al grafo (y de ahí al prompt): sin saltos de
/// línea, sin `<`/`>` (un doc malicioso no puede inyectar pseudo-etiquetas), ≤64 chars.
fn sanitize_label(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control() && *c != '<' && *c != '>')
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() > MAX_LABEL_CHARS {
        clean.chars().take(MAX_LABEL_CHARS).collect()
    } else {
        clean
    }
}

// ---------------------------------------------------------------------------
// Stopwords es/en/it (embebidas: cero deps, mismo espíritu multilingüe que BGE-M3)
// ---------------------------------------------------------------------------

fn is_stopword(w: &str) -> bool {
    STOPWORDS.contains(&w)
}

const STOPWORDS: &[&str] = &[
    // — español —
    "el", "la", "los", "las", "un", "una", "unos", "unas", "de", "del", "al", "a", "ante", "bajo",
    "con", "contra", "desde", "durante", "en", "entre", "hacia", "hasta", "mediante", "para",
    "por", "según", "sin", "sobre", "tras", "y", "o", "u", "e", "ni", "que", "como", "cuando",
    "donde", "quien", "cual", "cuyo", "si", "no", "más", "mas", "menos", "muy", "mucho", "poco",
    "tan", "tanto", "también", "tampoco", "ya", "aún", "todavía", "siempre", "nunca", "es", "son",
    "era", "eran", "fue", "fueron", "ser", "estar", "está", "están", "estaba", "hay", "ha", "han",
    "haber", "he", "hemos", "tiene", "tienen", "tener", "hace", "hacen", "hacer", "puede",
    "pueden", "poder", "debe", "deben", "este", "esta", "estos", "estas", "ese", "esa", "esos",
    "esas", "aquel", "aquella", "lo", "le", "les", "se", "su", "sus", "mi", "mis", "tu", "tus",
    "te", "me", "nos", "os", "yo", "él", "ella", "ellos", "ellas", "nosotros", "usted", "ustedes",
    "pero", "sino", "porque", "pues", "aunque", "mientras", "además", "luego", "entonces", "así",
    "cada", "todo", "toda", "todos", "todas", "otro", "otra", "otros", "otras", "mismo", "misma",
    "algo", "alguien", "nada", "nadie", "uno", "dos", "vez", "veces", "qué", "cómo", "cuál",
    "dónde", "cuándo", "sí", // — inglés —
    "the", "of", "and", "or", "to", "in", "on", "at", "by", "for", "with", "from", "as", "is",
    "are", "was", "were", "be", "been", "being", "am", "it", "its", "this", "that", "these",
    "those", "a", "an", "but", "if", "then", "than", "so", "such", "not", "nor", "too", "very",
    "can", "could", "may", "might", "must", "shall", "should", "will", "would", "do", "does",
    "did", "done", "have", "has", "had", "having", "i", "you", "he", "she", "we", "they", "them",
    "his", "her", "their", "our", "your", "my", "me", "us", "him", "who", "whom", "whose", "which",
    "what", "when", "where", "why", "how", "all", "any", "both", "each", "few", "more", "most",
    "other", "some", "no", "only", "own", "same", "just", "also", "there", "here", "out", "up",
    "down", "over", "under", "again", "once", "about", "into", "through", "during", "before",
    "after", "above", "below", "between", "while", "because", "until", "against", "per",
    // — italiano —
    "il", "lo", "i", "gli", "le", "un'", "uno", "una", "di", "da", "del", "della", "dello", "dei",
    "degli", "delle", "nel", "nella", "nello", "nei", "negli", "nelle", "sul", "sulla", "sullo",
    "sui", "sugli", "sulle", "col", "coi", "ed", "od", "anche", "ancora", "che", "chi", "cui",
    "come", "dove", "quando", "perché", "se", "non", "più", "meno", "molto", "poco", "tanto",
    "troppo", "già", "mai", "sempre", "è", "sono", "era", "erano", "fu", "furono", "essere",
    "stato", "stata", "sta", "stanno", "ho", "hai", "ha", "hanno", "avere", "aveva", "può",
    "possono", "deve", "devono", "questo", "questa", "questi", "queste", "quello", "quella",
    "quelli", "quelle", "mio", "mia", "tuo", "tua", "suo", "sua", "loro", "nostro", "vostra", "ci",
    "vi", "si", "ne", "io", "tu", "lui", "lei", "noi", "voi", "ma", "però", "quindi", "allora",
    "così", "ogni", "tutto", "tutta", "tutti", "tutte", "altro", "altra", "altri", "altre",
    "stesso", "stessa", "qualcosa", "qualcuno", "niente", "nessuno", "essa", "esso",
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Embedder determinista sin red: vector según hash de palabras. Misma cadena →
    /// mismo vector; cadenas distintas → casi ortogonales (salvo las forzadas iguales).
    struct MockEmbedder {
        /// Pares que deben dar embeddings idénticos (para probar el merge ≥0.92).
        twins: Vec<(String, String)>,
    }

    #[async_trait::async_trait]
    impl ConceptEmbedder for MockEmbedder {
        async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
            let mut key = text.to_string();
            for (a, b) in &self.twins {
                if text == b {
                    key = a.clone();
                }
            }
            let mut v = vec![0.0f32; 64];
            let mut h: u64 = 1469598103934665603;
            for byte in key.bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(1099511628211);
            }
            for slot in v.iter_mut() {
                h ^= h << 13;
                h ^= h >> 7;
                h ^= h << 17;
                *slot = ((h % 2000) as f32 - 1000.0) / 1000.0;
            }
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            Ok(v.into_iter().map(|x| x / norm).collect())
        }
    }

    fn tmp_graph() -> (KnowledgeGraph, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("aion-graph-test-{}.jsonl", uuid::Uuid::new_v4()));
        (KnowledgeGraph::open(&path), path)
    }

    fn doc_chunks(domain: &str, source: &str, texts: &[&str]) -> Vec<(String, String)> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("{domain}::{source}#{i}"), t.to_string()))
            .collect()
    }

    #[test]
    fn extraccion_espanol() {
        let cs = extract_concepts(
            "La fotosíntesis convierte la luz solar en energía química. \
             La fotosíntesis ocurre en los cloroplastos. La energía química \
             alimenta la célula vegetal.",
        );
        let norms: Vec<&str> = cs.iter().map(|c| c.norm.as_str()).collect();
        assert!(
            norms.iter().any(|n| n.contains("fotosíntesis")),
            "{norms:?}"
        );
        assert!(
            norms.iter().any(|n| n.contains("energía química")),
            "{norms:?}"
        );
        // Las stopwords no aparecen como conceptos.
        assert!(!norms.iter().any(|n| *n == "la" || *n == "en"));
    }

    #[test]
    fn extraccion_ingles_e_italiano() {
        let en = extract_concepts(
            "Neural networks learn patterns from data. Neural networks need training data.",
        );
        assert!(
            en.iter().any(|c| c.norm.contains("neural network")),
            "{:?}",
            en.iter().map(|c| &c.norm).collect::<Vec<_>>()
        );
        let it = extract_concepts(
            "La memoria associativa collega i ricordi. La memoria associativa usa vettori.",
        );
        assert!(
            it.iter().any(|c| c.norm.contains("memoria associativa")),
            "{:?}",
            it.iter().map(|c| &c.norm).collect::<Vec<_>>()
        );
    }

    #[test]
    fn normaliza_conservador() {
        assert_eq!(normalize_concept("Cloroplastos"), "cloroplasto");
        // -es, -is, -us se conservan (stripping daría formas inválidas).
        assert_eq!(normalize_concept("redes"), "redes");
        assert_eq!(normalize_concept("análisis"), "análisis");
        assert_eq!(normalize_concept("  Energía   Química "), "energía química");
    }

    #[test]
    fn labels_saneados() {
        let s = sanitize_label("hola <script> mundo\nmalicioso");
        assert!(!s.contains('<') && !s.contains('>') && !s.contains('\n'));
        let largo = "x".repeat(200);
        assert!(sanitize_label(&largo).chars().count() <= MAX_LABEL_CHARS);
    }

    #[tokio::test]
    async fn upsert_crea_nodos_y_coocurrencia() {
        let (mut g, path) = tmp_graph();
        let emb = MockEmbedder { twins: vec![] };
        let chunks = doc_chunks(
            "bio",
            "plantas.md",
            &[
                "La fotosíntesis produce energía química. La fotosíntesis usa clorofila verde.",
                "La energía química alimenta la planta. La clorofila verde absorbe la luz roja. \
             La energía química viene de la fotosíntesis.",
            ],
        );
        let stats = g
            .upsert_document("bio", "plantas.md", &chunks, &emb)
            .await
            .unwrap();
        assert!(stats.concepts_new >= 2, "{stats:?}");
        assert!(stats.edges_extracted >= 1, "{stats:?}");
        // El concepto puentea a sus chunks y el nodo Source existe.
        let foto = g
            .nodes()
            .iter()
            .find(|n| n.id.starts_with("c:fotosíntesi"))
            .unwrap();
        assert!(!foto.chunk_ids.is_empty());
        assert!(g.nodes().iter().any(|n| n.kind == NodeKind::Source));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn upsert_idempotente_y_remove_limpia() {
        let (mut g, path) = tmp_graph();
        let emb = MockEmbedder { twins: vec![] };
        let chunks = doc_chunks(
            "bio",
            "doc.md",
            &["El proyecto Vega usa la base Lumen. El proyecto Vega despliega la base Lumen."],
        );
        g.upsert_document("bio", "doc.md", &chunks, &emb)
            .await
            .unwrap();
        let (n1, e1) = (g.node_count(), g.edge_count());
        // Re-ingestar el MISMO doc no duplica nada.
        g.upsert_document("bio", "doc.md", &chunks, &emb)
            .await
            .unwrap();
        assert_eq!((g.node_count(), g.edge_count()), (n1, e1));
        // Borrar el doc deja el grafo vacío (era su única evidencia).
        let removed = g.remove_document("bio", "doc.md").unwrap();
        assert!(removed > 0);
        assert_eq!(g.node_count(), 0, "quedaron nodos huérfanos");
        assert_eq!(g.edge_count(), 0, "quedaron aristas huérfanas");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn dedup_por_embedding_funde_alias() {
        let (mut g, path) = tmp_graph();
        // "auto eléctrico" y "coche eléctrico" devuelven el MISMO embedding.
        let emb = MockEmbedder {
            twins: vec![("auto eléctrico".into(), "coche eléctrico".into())],
        };
        let d1 = doc_chunks(
            "motor",
            "a.md",
            &["El auto eléctrico carga rápido. El auto eléctrico usa baterías nuevas."],
        );
        g.upsert_document("motor", "a.md", &d1, &emb).await.unwrap();
        let d2 = doc_chunks(
            "motor",
            "b.md",
            &["El coche eléctrico no contamina. El coche eléctrico gana mercado urbano."],
        );
        g.upsert_document("motor", "b.md", &d2, &emb).await.unwrap();
        // No hay dos nodos para el mismo concepto: uno es alias del otro.
        let owners: Vec<&GraphNode> = g
            .nodes()
            .iter()
            .filter(|n| n.id == "c:auto eléctrico" || n.id == "c:coche eléctrico")
            .collect();
        assert_eq!(owners.len(), 1, "el twin debió fundirse como alias");
        assert!(owners[0].aliases.iter().any(|a| a.contains("eléctrico")));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn persistencia_round_trip() {
        let (mut g, path) = tmp_graph();
        let emb = MockEmbedder { twins: vec![] };
        let chunks = doc_chunks(
            "test",
            "rt.md",
            &["La memoria vectorial guarda recuerdos. La memoria vectorial usa similitud coseno."],
        );
        g.upsert_document("test", "rt.md", &chunks, &emb)
            .await
            .unwrap();
        let (n, e) = (g.node_count(), g.edge_count());
        drop(g);
        let g2 = KnowledgeGraph::open(&path);
        assert_eq!((g2.node_count(), g2.edge_count()), (n, e));
        assert!(g2.stats()["nodes"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn lpa_separa_dos_clusteres_y_resolve_ambiguous_funde() {
        let (mut g, path) = tmp_graph();
        let emb = MockEmbedder { twins: vec![] };
        // Dos documentos con vocabularios disjuntos → dos comunidades.
        let d1 = doc_chunks("bio", "celulas.md", &[
            "La membrana celular protege el citoplasma denso. La membrana celular regula el citoplasma denso.",
        ]);
        let d2 = doc_chunks("astro", "estrellas.md", &[
            "La fusión nuclear alimenta la estrella gigante. La fusión nuclear sostiene la estrella gigante.",
        ]);
        g.upsert_document("bio", "celulas.md", &d1, &emb)
            .await
            .unwrap();
        g.upsert_document("astro", "estrellas.md", &d2, &emb)
            .await
            .unwrap();
        let n = g.detect_communities();
        assert!(n >= 2, "esperaba ≥2 comunidades, hubo {n}");
        // Todos los conceptos conectados quedaron asignados.
        assert!(!g.communities_stale());
        // merge_nodes vía resolve_ambiguous(same=true) compacta sin dejar huérfanos.
        let (a, b) = (
            "c:membrana celular".to_string(),
            "c:citoplasma denso".to_string(),
        );
        let nodes_before = g.node_count();
        g.merge_nodes(&a, &b);
        assert_eq!(g.node_count(), nodes_before - 1);
        assert!(
            g.edges().iter().all(|e| e.a != b && e.b != b),
            "aristas colgando del nodo fundido"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn local_candidates_multihop_cruza_documentos() {
        let (mut g, path) = tmp_graph();
        let emb = MockEmbedder { twins: vec![] };
        // Doc 1: Vega ↔ Lumen co-ocurren. Doc 2: Lumen ↔ Marta Ríos co-ocurren.
        let d1 = doc_chunks(
            "kg",
            "uno.md",
            &["El proyecto Vega usa la base Lumen. El proyecto Vega adora la base Lumen."],
        );
        let d2 = doc_chunks(
            "kg",
            "dos.md",
            &["La base Lumen la creó Marta Ríos. La base Lumen es obra de Marta Ríos."],
        );
        g.upsert_document("kg", "uno.md", &d1, &emb).await.unwrap();
        g.upsert_document("kg", "dos.md", &d2, &emb).await.unwrap();
        // Consulta léxicamente cercana a "proyecto vega": el multi-hop debe alcanzar
        // el chunk del doc 2 vía el concepto compartido (base lumen).
        let q_emb = emb.embed("proyecto vega").await.unwrap();
        let hits = g.local_candidates(&q_emb, "proyecto vega", 4, 2);
        assert!(
            hits.iter().any(|h| h.chunk_id.starts_with("kg::dos.md")),
            "el multi-hop no cruzó documentos: {:?}",
            hits.iter().map(|h| &h.chunk_id).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_file(&path);
    }
}
