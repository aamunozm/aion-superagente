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

/// Fecha DESCONOCIDA: epoch (1970) es el valor por defecto de los recuerdos previos
/// al campo `created_at`. No es una fecha real → la tratamos como «sin fecha» para
/// no sesgar la recencia ni mostrar «1970-01-01» en resúmenes.
pub fn is_unknown_time(t: DateTime<Utc>) -> bool {
    t.timestamp() <= 0
}

/// **Forma canónica del nombre de un proyecto.** Unifica variantes (mayúsculas, acentos,
/// espacios, signos) a un slug estable: `AION`→`aion`, `Peace Harmony`→`peace-harmony`.
/// Resuelve además alias humanos que NO coinciden por slug (p. ej. «Peace Harmony AFC» y
/// «peace-harmony» son el MISMO proyecto). Se usa AL ESCRIBIR (para no generar duplicados
/// nuevos) y en la migración de etiquetas existentes, de modo que medir/exportar/borrar por
/// proyecto capture TODAS las variantes — y TODAS las ramas, que comparten la etiqueta.
pub fn canonical_project(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    let mut slug = String::with_capacity(lower.len());
    let mut prev_dash = false;
    for ch in lower.chars() {
        let mapped = match ch {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        };
        if mapped == '-' {
            if !prev_dash && !slug.is_empty() {
                slug.push('-');
                prev_dash = true;
            }
        } else {
            slug.push(mapped);
            prev_dash = false;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    // Alias humanos que no coinciden por slug (mismo proyecto, nombre escrito distinto).
    match slug.as_str() {
        "peace-harmony-afc" => "peace-harmony".to_string(),
        _ => slug,
    }
}

/// Extrae la etiqueta `[proyecto: X]` (CRUDA, sin canonicalizar) del inicio del contenido.
/// `None` si el recuerdo no lleva etiqueta de proyecto (memoria propia de AION).
fn parse_project_tag(content: &str) -> Option<String> {
    let rest = content.trim_start().strip_prefix("[proyecto:")?;
    let end = rest.find(']')?;
    let raw = rest[..end].trim().to_string();
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

/// Resumen de uso de memoria por proyecto (canónico) — alimenta el panel de gestión.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectStat {
    /// Nombre canónico del proyecto (ver [`canonical_project`]).
    pub project: String,
    /// Recuerdos VIGENTES (no obsoletos) etiquetados con este proyecto.
    pub count: usize,
    /// Bytes aproximados en el JSONL (suma de registros serializados; lo domina el
    /// embedding de 1024 floats → ~11 KB/recuerdo).
    pub bytes: usize,
    /// Última actividad conocida (created_at más reciente). `None` si es desconocida (epoch).
    pub last_activity: Option<DateTime<Utc>>,
}

/// Reporte de la migración de etiquetas de proyecto a su forma canónica.
#[derive(Debug, Clone, Serialize)]
pub struct NormalizeReport {
    /// Registros inspeccionados.
    pub scanned: usize,
    /// Registros cuya etiqueta se reescribió a canónica.
    pub rewritten: usize,
    /// `(etiqueta_origen, etiqueta_canónica, nº recuerdos)` para las que CAMBIARON.
    pub mapping: Vec<(String, String, usize)>,
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
    /// Derivados INMUTABLES del contenido, cacheados para NO recomputarlos en cada
    /// consulta. Antes `retrieve` re-tokenizaba (`fold_words`) y re-extraía entidades
    /// (`entities`) por CADA recuerdo y CADA query → O(n·L) en CPU + 2n allocaciones de
    /// `HashSet` por consulta, creciendo con la memoria. Como el contenido no cambia, se
    /// calculan UNA vez (al crear/cargar). Transitorios: `serde(skip)` → no cambian el
    /// formato en disco y los JSONL viejos cargan igual. `None` = aún sin calcular.
    #[serde(skip)]
    words: Option<std::collections::HashSet<String>>,
    #[serde(skip)]
    ents: Option<std::collections::HashSet<String>>,
}

impl MemoryRecord {
    /// Calcula y cachea los derivados léxicos/entidades del contenido (idempotente).
    /// Llamar al crear o cargar un recuerdo; tras esto `retrieve` no recomputa nada.
    fn prime_features(&mut self) {
        if self.words.is_none() {
            self.words = Some(fold_words(&self.content));
        }
        if self.ents.is_none() {
            self.ents = Some(entities(&self.content));
        }
    }
}

fn default_importance() -> f32 {
    0.5
}

/// ¿Puede un recuerdo nuevo dejar OBSOLETO a uno casi idéntico? Mismo origen: basta
/// con pesar casi tanto (tolerancia 0.1, permite refrescar valores). CRUZADO (nuevo
/// EXTERNO sobre uno del usuario, `origin` vacío): exige pesar ESTRICTAMENTE más —
/// como la importancia externa está capada a 0.6, jamás invalida una preferencia.
fn may_supersede(new_imp: f32, new_origin: &str, old_imp: f32, old_origin: &str) -> bool {
    let cross_origin = !new_origin.is_empty() && old_origin.is_empty();
    if cross_origin {
        new_imp > old_imp
    } else {
        new_imp + 0.1 >= old_imp
    }
}

/// Memoria vectorial: embeddings + recuperación por coseno, con persistencia opcional.
///
/// El embedder es un [`aion_kernel::Embedder`] cualquiera (no Ollama fijo): cambia el
/// backend de embeddings y la memoria lo usa sin cambios. Por defecto OllamaEmbedder+BGE-M3.
pub struct VectorMemory {
    embedder: Box<dyn aion_kernel::Embedder>,
    records: Mutex<Vec<MemoryRecord>>,
    path: Option<PathBuf>,
}

impl VectorMemory {
    /// Memoria efímera (solo RAM). Acepta CUALQUIER embedder (no solo Ollama).
    pub fn new(embedder: impl aion_kernel::Embedder + 'static) -> Self {
        Self {
            embedder: Box::new(embedder),
            records: Mutex::new(Vec::new()),
            path: None,
        }
    }

    /// Memoria efímera con valores por defecto (localhost + BGE-M3 vía Ollama).
    pub fn default_local() -> Self {
        Self::new(OllamaEmbedder::default_local())
    }

    /// Memoria persistente: carga los recuerdos previos del archivo JSONL si existe.
    pub fn persistent(
        embedder: impl aion_kernel::Embedder + 'static,
        path: impl Into<PathBuf>,
    ) -> Result<Self> {
        let path = path.into();
        let records = load_jsonl(&path)?;
        Ok(Self {
            embedder: Box::new(embedder),
            records: Mutex::new(records),
            path: Some(path),
        })
    }

    /// Persistente con valores por defecto.
    pub fn persistent_local(path: impl Into<PathBuf>) -> Result<Self> {
        Self::persistent(OllamaEmbedder::default_local(), path)
    }

    pub fn len(&self) -> usize {
        self.records.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Tamaño aproximado en bytes de TODA la memoria (suma de registros serializados ≈ tamaño
    /// del JSONL en disco; lo domina el embedding). Para calcular el % que cada proyecto ocupa
    /// del total en el panel de gestión.
    pub fn byte_size(&self) -> usize {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|r| serde_json::to_string(r).ok().map(|s| s.len() + 1))
            .sum()
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
        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
        recs.iter()
            .filter(|r| !r.superseded)
            .rev()
            .take(n)
            .map(|r| (r.id.clone(), r.content.clone()))
            .collect()
    }

    /// Vacía la memoria EN RAM y borra el archivo persistente (factory reset). En una
    /// instancia COMPARTIDA deja el estado coherente con el disco al instante (sin esto,
    /// borrar el archivo a mano dejaría el snapshot en RAM y la próxima escritura lo
    /// resucitaría). Idempotente si ya está vacía.
    pub fn clear(&self) -> Result<()> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        if let Some(path) = &self.path {
            if path.exists() {
                std::fs::remove_file(path).map_err(|e| AionError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Recarga la memoria desde el archivo persistente, descartando la copia EN RAM.
    /// Necesario cuando algo sobrescribe el JSONL POR FUERA del singleton (p. ej. una
    /// restauración de backup): sin esto, el snapshot viejo en RAM pisaría el archivo
    /// recién restaurado en la próxima escritura. No-op si la memoria es efímera.
    pub fn reload(&self) -> Result<()> {
        if let Some(path) = &self.path {
            let fresh = load_jsonl(path)?;
            *self.records.lock().unwrap_or_else(|e| e.into_inner()) = fresh;
        }
        Ok(())
    }

    /// PROCEDENCIA (`origin`) de un conjunto de ids: `""` = el propio AION, otro valor
    /// (p. ej. `"claude-code"`) = agente externo. Permite marcar en los prompts internos
    /// lo escrito por terceros sin separar almacenes (cuarentena suave).
    pub fn origins_for(&self, ids: &[String]) -> std::collections::HashMap<String, String> {
        let set: std::collections::HashSet<&String> = ids.iter().collect();
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| set.contains(&r.id))
            .map(|r| (r.id.clone(), r.origin.clone()))
            .collect()
    }

    /// **Qué cambió** desde `since` (bi-temporal): lo que AION aprendió de nuevo y lo
    /// que dejó de ser válido en esa ventana. Permite responder «¿qué ha cambiado desde
    /// la semana pasada?» sin un grafo completo.
    pub fn changes_since(&self, since: DateTime<Utc>) -> (Vec<String>, Vec<String>) {
        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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

    /// Borra recuerdos por id **permanentemente** (los elimina de RAM y del disco). A
    /// diferencia de `supersede` (que solo los oculta del retrieval), `forget` no deja rastro:
    /// es para purgar recuerdos erróneos u obsoletos. Devuelve cuántos se borraron de verdad
    /// (ids inexistentes se ignoran). Persiste de forma atómica bajo el mismo lock.
    pub fn forget(&self, ids: &[String]) -> Result<usize> {
        let set: std::collections::HashSet<&String> = ids.iter().collect();
        let mut recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
        let before = recs.len();
        recs.retain(|r| !set.contains(&r.id));
        let n = before - recs.len();
        if n > 0 {
            // Saneo del grafo GAAMA: quita las aristas (`links`) de los supervivientes que
            // apuntaban a ids borrados. Sin esto quedarían referencias colgantes que fragmentan
            // la recuperación asociativa y dejan basura permanente en el JSONL.
            for r in recs.iter_mut() {
                r.links.retain(|l| !set.contains(l));
            }
            if let Some(path) = &self.path {
                rewrite_jsonl(path, &recs)?;
            }
        }
        Ok(n)
    }

    /// Par de recuerdos LEJANOS entre sí (mínima similitud coseno entre los
    /// recientes): materia prima de la **bisociación creativa** — cruzar lo que
    /// normalmente no se toca. `None` si hay pocos recuerdos o todos se parecen
    /// (sim mínima ≥ 0.5): la creatividad forzada produce ruido, no ideas.
    pub fn distant_pair(&self) -> Option<(String, String)> {
        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
        let pool: Vec<&MemoryRecord> = recs
            .iter()
            .filter(|r| !r.superseded && !r.embedding.is_empty())
            .rev()
            .take(40)
            .collect();
        if pool.len() < 6 {
            return None;
        }
        let mut best: Option<(usize, usize, f32)> = None;
        for i in 0..pool.len() {
            for j in (i + 1)..pool.len() {
                let s = cosine(&pool[i].embedding, &pool[j].embedding);
                if best.map(|(_, _, b)| s < b).unwrap_or(true) {
                    best = Some((i, j, s));
                }
            }
        }
        let (i, j, s) = best?;
        if s >= 0.5 {
            return None;
        }
        Some((pool[i].content.clone(), pool[j].content.clone()))
    }

    /// **Re-scoring darwiniano por resultado REAL**: ajusta la aptitud (`fitness`)
    /// de los recuerdos que sirvieron de grounding a una tarea según cómo terminó.
    /// Un recuerdo que acompaña éxitos sube (sobrevive a la poda); uno que acompaña
    /// fracasos baja. Asimétrico a propósito: el fracaso informa más que el éxito,
    /// y así una lección equivocada no se perpetúa solo por ser recuperada a menudo.
    pub fn reinforce(&self, ids: &[String], success: bool) -> Result<usize> {
        let set: std::collections::HashSet<&String> = ids.iter().collect();
        let delta = if success { 0.05 } else { -0.10 };
        let mut recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
        let mut n = 0;
        for r in recs.iter_mut() {
            if set.contains(&r.id) {
                r.fitness = (r.fitness + delta).clamp(0.0, 1.0);
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
            let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut records = self.records.lock().unwrap_or_else(|e| e.into_inner());
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

    /// **Desglose de memoria por proyecto** (canónico), solo recuerdos VIGENTES, ordenado por
    /// nº de recuerdos desc. La memoria SIN etiqueta (la propia de AION) no entra aquí: el
    /// panel mide lo atribuible a proyectos. Las ramas se agregan bajo su proyecto (comparten
    /// etiqueta). Ver [`canonical_project`].
    pub fn project_breakdown(&self) -> Vec<ProjectStat> {
        use std::collections::HashMap;
        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
        let mut map: HashMap<String, ProjectStat> = HashMap::new();
        for r in recs.iter() {
            if r.superseded {
                continue;
            }
            let Some(raw) = parse_project_tag(&r.content) else {
                continue;
            };
            let canon = canonical_project(&raw);
            if canon.is_empty() {
                continue;
            }
            let bytes = serde_json::to_string(r).map(|s| s.len()).unwrap_or(0);
            let e = map.entry(canon.clone()).or_insert_with(|| ProjectStat {
                project: canon.clone(),
                count: 0,
                bytes: 0,
                last_activity: None,
            });
            e.count += 1;
            e.bytes += bytes;
            if !is_unknown_time(r.created_at) {
                e.last_activity = Some(match e.last_activity {
                    Some(prev) if prev >= r.created_at => prev,
                    _ => r.created_at,
                });
            }
        }
        let mut out: Vec<ProjectStat> = map.into_values().collect();
        out.sort_by(|a, b| b.count.cmp(&a.count));
        out
    }

    /// **Exporta como JSONL los recuerdos de un proyecto** (canónico), INCLUYENDO obsoletos
    /// para un backup fiel. Cada línea es un `MemoryRecord` completo (con embedding), así la
    /// restauración por `import_jsonl` no necesita re-embeddings. Captura todas las ramas y
    /// variantes de etiqueta del proyecto.
    pub fn export_project_jsonl(&self, project: &str) -> String {
        let canon = canonical_project(project);
        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
        recs.iter()
            .filter(|r| {
                parse_project_tag(&r.content)
                    .map(|raw| canonical_project(&raw) == canon)
                    .unwrap_or(false)
            })
            .filter_map(|r| serde_json::to_string(r).ok())
            .map(|s| s + "\n")
            .collect()
    }

    /// IDs de TODOS los recuerdos (vigentes u obsoletos) de un proyecto canónico. Para borrar
    /// y liberar espacio: se pasa a [`forget`](Self::forget).
    pub fn ids_for_project(&self, project: &str) -> Vec<String> {
        let canon = canonical_project(project);
        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
        recs.iter()
            .filter(|r| {
                parse_project_tag(&r.content)
                    .map(|raw| canonical_project(&raw) == canon)
                    .unwrap_or(false)
            })
            .map(|r| r.id.clone())
            .collect()
    }

    /// **Migra las etiquetas `[proyecto: X]` existentes a su forma canónica** ([`canonical_project`]).
    /// Hace BACKUP (`.jsonl.bak`) antes de reescribir. Solo toca el prefijo de proyecto del
    /// contenido; embedding, id y fechas quedan intactos. Idempotente (si ya es canónico, no
    /// reescribe). Re-cachea los derivados léxicos del contenido modificado. Devuelve un reporte.
    pub fn normalize_project_tags(&self) -> Result<NormalizeReport> {
        use std::collections::HashMap;
        let mut recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
        let scanned = recs.len();
        let mut rewritten = 0usize;
        let mut mapping: HashMap<(String, String), usize> = HashMap::new();
        for r in recs.iter_mut() {
            let Some(raw) = parse_project_tag(&r.content) else {
                continue;
            };
            let canon = canonical_project(&raw);
            if canon.is_empty() || canon == raw {
                continue;
            }
            let from = format!("[proyecto: {raw}]");
            let to = format!("[proyecto: {canon}]");
            if let Some(pos) = r.content.find(&from) {
                r.content.replace_range(pos..pos + from.len(), &to);
                // El contenido cambió → invalidar y recachear léxico/entidades.
                r.words = None;
                r.ents = None;
                r.prime_features();
                rewritten += 1;
                *mapping.entry((raw, canon)).or_insert(0) += 1;
            }
        }
        if rewritten > 0 {
            if let Some(path) = &self.path {
                if path.exists() {
                    let bak = path.with_extension("jsonl.bak");
                    let _ = std::fs::copy(path, &bak);
                }
                rewrite_jsonl(path, &recs)?;
            }
        }
        let mapping = mapping.into_iter().map(|((f, t), n)| (f, t, n)).collect();
        Ok(NormalizeReport {
            scanned,
            rewritten,
            mapping,
        })
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
            words: None,
            ents: None,
        };
        // Cachea léxico/entidades UNA vez al crear, para que `retrieve` no los recompute.
        record.prime_features();
        let now = Utc::now();
        let mut recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
            // Supersede CONSCIENTE DE IMPORTANCIA Y PROCEDENCIA: un comentario de paso
            // no puede invalidar una preferencia/decisión del usuario sobre el mismo
            // tema (con BGE-M3, paráfrasis del mismo tópico superan 0.88 con facilidad).
            // Mismo origen: basta con pesar casi tanto (tolerancia 0.1). CRUZADO
            // (un recuerdo EXTERNO sobre uno del usuario): exige pesar ESTRICTAMENTE
            // más — como la importancia externa está capada a 0.6, jamás pisa una
            // preferencia del usuario. Si no procede supersede, quedan ENLAZADOS.
            if sim > SUPERSEDE_SIM
                && may_supersede(record.importance, &record.origin, r.importance, &r.origin)
            {
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
        // Pliega/tokeniza la consulta UNA sola vez (antes se re-plegaba por cada recuerdo).
        let q_words = fold_words(query);

        let mut recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
                // Léxico y entidades desde el set CACHEADO del recuerdo (calculado al
                // crear/cargar). Solo si falta (recuerdo sin primar) se recompute al vuelo.
                let lex = match &r.words {
                    Some(w) => jaccard(&q_words, w),
                    None => lexical_overlap_pre(&q_words, &r.content),
                };
                let ent = match &r.ents {
                    Some(e) => entity_overlap(&q_ents, e),
                    None => entity_overlap(&q_ents, &entities(&r.content)),
                };
                // Recencia REAL (Generative Agents): decay exponencial por edad con
                // semivida de 7 días — antes era el índice ordinal, que premiaba la
                // posición en el archivo y no el tiempo. Las fechas DESCONOCIDAS (epoch
                // 1970: recuerdos previos al campo `created_at`) no deben hundir la
                // recencia a ~0 y enterrarlos para siempre → recencia neutra.
                let rec = if is_unknown_time(r.created_at) {
                    0.3
                } else {
                    let age_days = (now - r.created_at).num_seconds().max(0) as f32 / 86_400.0;
                    0.5_f32.powf(age_days / 7.0)
                };
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

        let recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
        if t.chars().count() < 2 {
            return None;
        }
        let has_digit = t.chars().any(|c| c.is_ascii_digit());
        let has_upper = t.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
        let has_sym = t.contains('#') || t.contains('-') || t.contains('_');
        if has_digit || has_upper || has_sym {
            // Detección sobre el token ORIGINAL (mayúscula/dígito/símbolo), pero se guarda
            // plegado para que "Milán"/"Milan" o "José"/"Jose" casen entre consulta y recuerdo.
            Some(fold(t))
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

/// Pliega diacríticos latinos comunes (español **e italiano**) a su letra base y pasa a
/// minúscula. Determinista y sin dependencias: une "decisión"/"decision", "está"/"esta",
/// "città"/"citta", "Milán"/"Milan" en el matching léxico y de entidades → robustez a
/// acentos y a typos DE ACENTO, gratis. NO corrige typos arbitrarios (eso lo cubre la señal
/// semántica BGE-M3, que es robusta a ruido). Aplicado por igual a la consulta y al recuerdo
/// en tiempo de comparación, así que NO requiere re-embeber ni migrar lo ya almacenado.
fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// Palabras significativas de un texto, normalizadas con `fold` (minúscula + sin acentos).
/// Filtro por CARÁCTERES (no bytes): así el umbral es estable al plegar acentos (`año`→`ano`
/// no cambia de categoría por pasar de 4 bytes a 3). Ignora palabras muy cortas/funcionales.
fn fold_words(s: &str) -> std::collections::HashSet<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 3)
        .map(|w| w.to_string())
        .collect()
}

/// Jaccard entre dos conjuntos de palabras YA plegados. Núcleo del solapamiento léxico:
/// `retrieve` lo usa con el set del recuerdo CACHEADO (cero recomputación por consulta).
fn jaccard(qa: &std::collections::HashSet<String>, wb: &std::collections::HashSet<String>) -> f32 {
    if qa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let inter = qa.intersection(wb).count() as f32;
    let union = qa.union(wb).count() as f32;
    if union > 0.0 {
        inter / union
    } else {
        0.0
    }
}

/// Jaccard entre la consulta pre-plegada y un TEXTO (lo pliega al vuelo). Fallback para
/// recuerdos cuyo set léxico aún no está cacheado (p. ej. construidos en tests).
fn lexical_overlap_pre(qa: &std::collections::HashSet<String>, b: &str) -> f32 {
    jaccard(qa, &fold_words(b))
}

/// Solapamiento léxico (Jaccard de palabras significativas, normalizadas con `fold`). El hot
/// path (`retrieve`) usa `lexical_overlap_pre` con la consulta pre-plegada; este wrapper queda
/// para los tests de comodidad.
#[cfg(test)]
fn lexical_overlap(a: &str, b: &str) -> f32 {
    lexical_overlap_pre(&fold_words(a), b)
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
        let mut recs = self.records.lock().unwrap_or_else(|e| e.into_inner());
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
                // PROCEDENCIA: si uno de los fundidos es del usuario (origin vacío) y el
                // superviviente era externo, la versión del usuario manda — el recuerdo
                // resultante deja de estar en cuarentena y conserva el texto del usuario.
                if r.origin.is_empty() && !k.origin.is_empty() {
                    k.content = r.content.clone();
                    k.origin = String::new();
                }
                k.importance = k.importance.max(r.importance);
                merged += 1;
            } else {
                kept.push(r);
            }
        }

        // 3) Poda: fuera los de aptitud baja que nunca se usaron — SALVO los
        // importantes ([aprendizaje]/[reflexión] arrancan en 0.7; preferencias del
        // usuario ≥0.65): el decay por sí solo los hundía bajo el suelo en ~12
        // consolidaciones sin uso, y AION olvidaba sus propias lecciones.
        let after_merge = kept.len();
        kept.retain(|r| r.fitness >= cfg.prune_floor || r.access_count > 0 || r.importance >= 0.65);
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
    // Escritura ATÓMICA (tmp + rename): un crash o corte a mitad de fs::write
    // dejaría el archivo de memoria a medias — aquí o queda la versión vieja
    // completa o la nueva completa, nunca un JSONL truncado.
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, buf).map_err(|e| AionError::Memory(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| AionError::Memory(e.to_string()))
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
        if let Ok(mut rec) = serde_json::from_str::<MemoryRecord>(&line) {
            // `words`/`ents` no se persisten (serde skip): se calculan al cargar, una vez.
            rec.prime_features();
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

    /// Vector determinista de 64 dims a partir del texto (para benchmark sin Ollama).
    fn mock_vec(text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; 64];
        for (i, b) in text.bytes().enumerate() {
            v[i % 64] += (b as f32) / 255.0;
        }
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        for x in v.iter_mut() {
            *x /= n;
        }
        v
    }

    struct MockEmb;
    #[async_trait::async_trait]
    impl aion_kernel::Embedder for MockEmb {
        async fn embed(&self, text: &str) -> Result<Vec<f32>> {
            Ok(mock_vec(text))
        }
        fn model(&self) -> &str {
            "mock"
        }
    }

    /// Construye un MemoryRecord mínimo para tests de gestión por proyecto.
    fn mkrec(id: &str, content: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            content: content.to_string(),
            embedding: mock_vec(content),
            fitness: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            superseded: false,
            superseded_at: None,
            importance: 0.5,
            links: Vec::new(),
            origin: "claude-code".to_string(),
            words: None,
            ents: None,
        }
    }

    #[test]
    fn canonical_project_unifies_variants() {
        assert_eq!(canonical_project("AION"), "aion");
        assert_eq!(canonical_project("  aion "), "aion");
        assert_eq!(canonical_project("Peace Harmony"), "peace-harmony");
        // Alias humano: AFC es el mismo proyecto que peace-harmony.
        assert_eq!(canonical_project("Peace Harmony AFC"), "peace-harmony");
        assert_eq!(canonical_project("peace-harmony"), "peace-harmony");
        // Acentos y signos colapsan a slug estable.
        assert_eq!(canonical_project("Café   del Día!!"), "cafe-del-dia");
        // aion-superagente NO se funde con aion (es otro repo).
        assert_eq!(canonical_project("aion-superagente"), "aion-superagente");
    }

    #[test]
    fn parse_project_tag_extracts_raw() {
        assert_eq!(
            parse_project_tag("[proyecto: AION] hola").as_deref(),
            Some("AION")
        );
        assert_eq!(
            parse_project_tag("  [proyecto: Peace Harmony AFC] x").as_deref(),
            Some("Peace Harmony AFC")
        );
        assert_eq!(parse_project_tag("sin etiqueta"), None);
        assert_eq!(parse_project_tag("[proyecto: ] vacio"), None);
    }

    #[test]
    fn breakdown_export_and_ids_group_by_canonical() {
        let mem = VectorMemory::new(MockEmb);
        {
            let mut recs = mem.records.lock().unwrap();
            recs.push(mkrec("1", "[proyecto: aion] uno"));
            recs.push(mkrec("2", "[proyecto: AION] dos"));
            recs.push(mkrec("3", "[proyecto: Peace Harmony AFC] tres"));
            recs.push(mkrec("4", "[proyecto: peace-harmony] cuatro"));
            recs.push(mkrec("5", "memoria propia sin proyecto"));
        }
        let bd = mem.project_breakdown();
        // 2 proyectos canónicos (aion=2, peace-harmony=2); el sin-etiqueta no cuenta.
        assert_eq!(bd.len(), 2);
        let aion = bd.iter().find(|p| p.project == "aion").unwrap();
        assert_eq!(aion.count, 2);
        let ph = bd.iter().find(|p| p.project == "peace-harmony").unwrap();
        assert_eq!(ph.count, 2);
        // Export e ids capturan ambas variantes de cada proyecto.
        assert_eq!(mem.ids_for_project("AION").len(), 2);
        assert_eq!(mem.ids_for_project("peace-harmony").len(), 2);
        assert_eq!(mem.export_project_jsonl("aion").lines().count(), 2);
    }

    #[test]
    fn normalize_rewrites_variant_tags() {
        let mem = VectorMemory::new(MockEmb);
        {
            let mut recs = mem.records.lock().unwrap();
            recs.push(mkrec("1", "[proyecto: AION] uno"));
            recs.push(mkrec("2", "[proyecto: aion] dos"));
            recs.push(mkrec("3", "[proyecto: Peace Harmony AFC] tres"));
        }
        let report = mem.normalize_project_tags().unwrap();
        // Se reescriben AION→aion y Peace Harmony AFC→peace-harmony (2), aion ya canónico (0).
        assert_eq!(report.rewritten, 2);
        // Idempotente: una segunda pasada no cambia nada.
        assert_eq!(mem.normalize_project_tags().unwrap().rewritten, 0);
        // Tras normalizar, todo aion bajo una etiqueta.
        let recs = mem.records.lock().unwrap();
        assert!(recs[0].content.starts_with("[proyecto: aion]"));
        assert!(recs[2].content.starts_with("[proyecto: peace-harmony]"));
    }

    /// BENCHMARK (ignored: correr con `cargo test -p aion-memory --release bench_retrieve
    /// -- --ignored --nocapture`). Mide el coste por consulta del escaneo de `retrieve`
    /// ANTES (sin cachear `words`/`ents` → recomputa por recuerdo y consulta) y DESPUÉS
    /// (cacheados). Verifica además que cachear NO cambia los resultados (top-k idénticos).
    #[tokio::test]
    #[ignore]
    async fn bench_retrieve_cached_vs_recompute() {
        const N: usize = 4000;
        const ITERS: usize = 40;
        let mem = VectorMemory::new(MockEmb);
        {
            let mut recs = mem.records.lock().unwrap();
            for i in 0..N {
                let content = format!(
                    "Ariel decidió usar Rust para el proyecto AION-{i} en Milano con Gemma 12B \
                     y embeddings BGE-M3 porque prefiere local-first, baja latencia y coste cero",
                );
                let emb = mock_vec(&content);
                let mut r = rec(emb, 0.5, 0);
                r.content = content;
                r.words = None; // estado "viejo": sin cachear
                r.ents = None;
                recs.push(r);
            }
        }
        let query = "qué stack eligió Ariel para AION en Milano y por qué local-first";

        // Calienta y MIDE el camino de recompute (campos en None → fallback).
        let _ = mem.retrieve(query, 8).await.unwrap();
        let before_ids = top_ids(&mem.retrieve(query, 8).await.unwrap());
        let t0 = std::time::Instant::now();
        for _ in 0..ITERS {
            let _ = mem.retrieve(query, 8).await.unwrap();
        }
        let recompute = t0.elapsed();

        // Prima (cachea) y MIDE de nuevo sobre los MISMOS datos.
        {
            let mut recs = mem.records.lock().unwrap();
            for r in recs.iter_mut() {
                r.prime_features();
            }
        }
        let after_ids = top_ids(&mem.retrieve(query, 8).await.unwrap());
        let t1 = std::time::Instant::now();
        for _ in 0..ITERS {
            let _ = mem.retrieve(query, 8).await.unwrap();
        }
        let cached = t1.elapsed();

        let per_recompute = recompute.as_micros() as f64 / ITERS as f64;
        let per_cached = cached.as_micros() as f64 / ITERS as f64;
        println!(
            "retrieve sobre {N} recuerdos — recompute: {per_recompute:.0} µs/consulta | \
             cacheado: {per_cached:.0} µs/consulta | speedup x{:.2}",
            per_recompute / per_cached.max(1.0)
        );
        // CORRECCIÓN: cachear no debe cambiar el resultado.
        assert_eq!(
            before_ids, after_ids,
            "el top-k cambió al cachear (regresión)"
        );
        // RENDIMIENTO: el camino cacheado debe ser estrictamente más rápido.
        assert!(
            cached < recompute,
            "cacheado no fue más rápido que recompute"
        );
    }

    fn top_ids(hits: &[MemoryHit]) -> Vec<String> {
        hits.iter().map(|h| h.id.clone()).collect()
    }

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
            words: None,
            ents: None,
        }
    }

    #[test]
    fn reinforce_adjusts_fitness_by_real_outcome() {
        let mem = VectorMemory::default_local();
        let (id_a, id_b) = {
            let mut r = mem.records.lock().unwrap_or_else(|e| e.into_inner());
            r.push(rec(vec![1.0, 0.0, 0.0], 0.5, 0));
            r.push(rec(vec![0.0, 1.0, 0.0], 0.5, 0));
            (r[0].id.clone(), r[1].id.clone())
        };
        // Éxito: sube poco. Fracaso: baja más (asimetría a propósito).
        assert_eq!(mem.reinforce(std::slice::from_ref(&id_a), true).unwrap(), 1);
        assert_eq!(
            mem.reinforce(std::slice::from_ref(&id_b), false).unwrap(),
            1
        );
        let r = mem.records.lock().unwrap_or_else(|e| e.into_inner());
        assert!((r[0].fitness - 0.55).abs() < 1e-6);
        assert!((r[1].fitness - 0.40).abs() < 1e-6);
        drop(r);
        // El fitness queda acotado a [0,1] aunque se refuerce muchas veces.
        for _ in 0..20 {
            let _ = mem.reinforce(std::slice::from_ref(&id_b), false);
        }
        assert!(mem.records.lock().unwrap_or_else(|e| e.into_inner())[1].fitness >= 0.0);
    }

    #[test]
    fn consolidation_merges_duplicates_and_prunes_weak() {
        let mem = VectorMemory::default_local();
        {
            let mut r = mem.records.lock().unwrap_or_else(|e| e.into_inner());
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
            let mut r = mem.records.lock().unwrap_or_else(|e| e.into_inner());
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
    fn external_memory_cannot_supersede_user_preference() {
        // Preferencia del usuario (origin="") con importancia típica 0.66.
        // Un recuerdo EXTERNO capado a 0.6 NO puede invalidarla aunque sea idéntico.
        assert!(!may_supersede(0.6, "claude-code", 0.66, ""));
        // Tampoco a una de igual peso (exige estrictamente mayor en cruzado).
        assert!(!may_supersede(0.6, "claude-code", 0.6, ""));
        // Pero el propio usuario sí puede refrescar su recuerdo (mismo origen, tol 0.1).
        assert!(may_supersede(0.6, "", 0.66, ""));
        // Y un externo puede actualizar a otro externo menos importante.
        assert!(may_supersede(0.6, "claude-code", 0.5, "claude-code"));
    }

    #[test]
    fn merge_prefers_user_provenance_over_external() {
        let mem = VectorMemory::default_local();
        {
            let mut r = mem.records.lock().unwrap_or_else(|e| e.into_inner());
            // Mismo embedding → se funden. El externo entró primero (superviviente),
            // el del usuario después: tras fundir, gana la procedencia del usuario.
            let mut externo = rec(vec![1.0, 0.0, 0.0], 0.5, 0);
            externo.origin = "claude-code".into();
            externo.content = "nota externa".into();
            externo.importance = 0.6;
            r.push(externo);
            let mut propio = rec(vec![1.0, 0.0, 0.0], 0.5, 0);
            propio.origin = String::new();
            propio.content = "decisión del usuario".into();
            propio.importance = 0.66;
            r.push(propio);
        }
        let report = mem.consolidate(&ConsolidationConfig::default()).unwrap();
        assert_eq!(report.merged, 1);
        let r = mem.records.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].origin, "", "el superviviente queda como del usuario");
        assert_eq!(r[0].content, "decisión del usuario");
        assert!((r[0].importance - 0.66).abs() < 1e-6);
    }

    #[test]
    fn clear_and_reload_roundtrip_with_file() {
        // Archivo JSONL con un recuerdo, cargado por una memoria persistente.
        let path = std::env::temp_dir().join(format!("aion-mem-{}.jsonl", Uuid::new_v4()));
        let r = rec(vec![1.0, 0.0], 0.5, 0);
        std::fs::write(&path, serde_json::to_string(&r).unwrap() + "\n").unwrap();
        let mem = VectorMemory::persistent(OllamaEmbedder::default_local(), &path).unwrap();
        assert_eq!(mem.len(), 1);

        // clear(): vacía RAM y borra el archivo.
        mem.clear().unwrap();
        assert_eq!(mem.len(), 0);
        assert!(!path.exists());

        // Alguien restaura el archivo POR FUERA; reload() lo trae de vuelta a RAM.
        std::fs::write(&path, serde_json::to_string(&r).unwrap() + "\n").unwrap();
        assert_eq!(mem.len(), 0, "aún no se ve sin reload");
        mem.reload().unwrap();
        assert_eq!(mem.len(), 1, "reload trae el archivo restaurado a RAM");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn unknown_time_detects_epoch() {
        assert!(is_unknown_time(epoch()));
        assert!(!is_unknown_time(Utc::now()));
    }

    #[test]
    fn origins_for_maps_ids_to_provenance() {
        let mem = VectorMemory::default_local();
        let (id_u, id_e) = {
            let mut r = mem.records.lock().unwrap_or_else(|e| e.into_inner());
            let mut u = rec(vec![1.0, 0.0], 0.5, 0);
            u.origin = String::new();
            let mut e = rec(vec![0.0, 1.0], 0.5, 0);
            e.origin = "claude-code".into();
            r.push(u.clone());
            r.push(e.clone());
            (u.id, e.id)
        };
        let map = mem.origins_for(&[id_u.clone(), id_e.clone()]);
        assert_eq!(map.get(&id_u).map(String::as_str), Some(""));
        assert_eq!(map.get(&id_e).map(String::as_str), Some("claude-code"));
    }

    #[test]
    fn fold_strips_es_and_it_diacritics() {
        assert_eq!(
            fold("Decisión Città Milán ñoño"),
            "decision citta milan nono"
        );
    }

    #[test]
    fn fold_words_filters_by_chars_not_bytes() {
        // "año"→"ano" (3 chars → fuera), "añil"→"anil" (4 → dentro), "decisión"→"decision".
        // Con el filtro por BYTES anterior, "año" (4 bytes) habría entrado incoherentemente.
        let w = fold_words("año añil decisión");
        assert!(w.contains("anil"));
        assert!(w.contains("decision"));
        assert!(
            !w.contains("ano"),
            "palabra de 3 chars debe filtrarse pese a tener acento"
        );
    }

    #[test]
    fn lexical_overlap_is_accent_insensitive() {
        // La misma frase con acentos y sin (typo de acento) debe casar fuerte: el plegado
        // une "decisión"/"decision", "está"/"esta", "autenticación"/"autenticacion".
        let con = "la decisión crítica está en la autenticación";
        let sin = "la decision critica esta en la autenticacion";
        assert!(
            lexical_overlap(con, sin) > 0.9,
            "los acentos deberían plegarse en la señal léxica"
        );
    }

    #[test]
    fn forget_removes_by_id_and_persists() {
        let path = std::env::temp_dir().join(format!("aion-forget-{}.jsonl", Uuid::new_v4()));
        let a = rec(vec![1.0, 0.0], 0.5, 0);
        let mut b = rec(vec![0.0, 1.0], 0.5, 0);
        // b tiene una ARISTA hacia a (grafo GAAMA): debe limpiarse al borrar a.
        b.links = vec![a.id.clone()];
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&a).unwrap(),
                serde_json::to_string(&b).unwrap()
            ),
        )
        .unwrap();
        let mem = VectorMemory::persistent(OllamaEmbedder::default_local(), &path).unwrap();
        assert_eq!(mem.len(), 2);

        // Borra uno; los ids inexistentes se ignoran (no cuentan).
        let removed = mem.forget(&[a.id.clone(), "no-existe".into()]).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(mem.len(), 1);

        // Persistió en disco: una memoria nueva sobre el mismo archivo ya no ve el borrado…
        let mem2 = VectorMemory::persistent(OllamaEmbedder::default_local(), &path).unwrap();
        assert_eq!(mem2.len(), 1);
        assert!(mem2.recent_with_ids(10).iter().all(|(id, _)| *id != a.id));
        // …y la arista colgante hacia a fue saneada en el superviviente b.
        {
            let recs = mem2.records.lock().unwrap_or_else(|e| e.into_inner());
            assert!(
                recs.iter().all(|r| !r.links.contains(&a.id)),
                "forget debe limpiar los links hacia ids borrados"
            );
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn changes_since_separates_new_and_obsolete() {
        let mem = VectorMemory::default_local();
        let t0 = epoch();
        {
            let mut r = mem.records.lock().unwrap_or_else(|e| e.into_inner());
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
