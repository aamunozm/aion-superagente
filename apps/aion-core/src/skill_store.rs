//! Almacén **persistente** de skills forjadas. Cuando AION se escribe una skill
//! nueva, se guarda aquí (WAT + manifiesto) y se vuelve a cargar en cada arranque.
//! Así su caja de herramientas CRECE con el tiempo: es mejor en lo que hace porque
//! acumula capacidades, no parte de cero en cada sesión.

use aion_skills::{SkillManifest, WasmSkillHost};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSkill {
    name: String,
    description: String,
    wat: String,
    /// Nº de tests que superó la mejor versión aceptada (para el RATCHET).
    #[serde(default)]
    passed: usize,
}

fn store_path() -> PathBuf {
    crate::app_data_dir().join("skills.jsonl")
}

/// RATCHET: nº de tests que superó la MEJOR versión guardada de una skill. Una
/// re-forja solo debe reemplazarla si iguala o supera esta marca (no regresar).
pub fn best_passed(name: &str) -> usize {
    load_records()
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.passed)
        .unwrap_or(0)
}

/// Guarda una skill forjada (idempotente por nombre: reemplaza si ya existía).
pub fn save(name: &str, description: &str, wat: &str, passed: usize) -> std::io::Result<()> {
    let path = store_path();
    let mut skills = load_records();
    skills.retain(|s| s.name != name);
    skills.push(StoredSkill {
        name: name.to_string(),
        description: description.to_string(),
        wat: wat.to_string(),
        passed,
    });
    let body: String = skills
        .iter()
        .filter_map(|s| serde_json::to_string(s).ok())
        .map(|s| s + "\n")
        .collect();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)?;
    // Invalida el embedding cacheado de esta skill: si se re-forjó con otra
    // descripción, el de la caché quedó obsoleto y la hidratación la rankearía mal.
    // Se recomputará la próxima vez que se necesite.
    invalidate_emb_cache(name);
    Ok(())
}

/// Quita una skill de la caché de embeddings (tras re-forjarla). Barato y fail-soft.
fn invalidate_emb_cache(name: &str) {
    let mut cache = load_emb_cache();
    if cache.remove(name).is_some() {
        save_emb_cache(&cache);
    }
}

/// Carga TODAS las skills persistidas en el host (al arrancar). Devuelve cuántas.
pub fn load_all(host: &WasmSkillHost) -> usize {
    let mut n = 0;
    for s in load_records() {
        if host
            .register(
                SkillManifest {
                    name: s.name.clone(),
                    description: s.description.clone(),
                },
                s.wat.as_bytes(),
            )
            .is_ok()
        {
            n += 1;
        }
    }
    n
}

/// Por debajo de esto no merece la pena filtrar: registrar todas es más barato que
/// embeber. Por encima, hidratamos solo las relevantes (el almacén puede crecer sin fin).
const HYDRATE_FLOOR: usize = 8;

fn emb_cache_path() -> PathBuf {
    crate::app_data_dir().join("skills_emb.jsonl")
}

#[derive(Deserialize)]
struct SkillEmb {
    name: String,
    embedding: Vec<f32>,
}

/// Variante prestada para serializar SIN clonar (escritura del cache en caliente).
#[derive(Serialize)]
struct SkillEmbRef<'a> {
    name: &'a str,
    embedding: &'a [f32],
}

fn load_emb_cache() -> std::collections::HashMap<String, Vec<f32>> {
    let Ok(txt) = std::fs::read_to_string(emb_cache_path()) else {
        return std::collections::HashMap::new();
    };
    txt.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<SkillEmb>(l).ok())
        .map(|s| (s.name, s.embedding))
        .collect()
}

fn save_emb_cache(map: &std::collections::HashMap<String, Vec<f32>>) {
    let body: String = map
        .iter()
        .filter_map(|(name, embedding)| {
            serde_json::to_string(&SkillEmbRef { name, embedding }).ok()
        })
        .map(|l| l + "\n")
        .collect();
    crate::write_atomic(&emb_cache_path(), &body);
}

/// Funde embeddings nuevos en el cache con un read-merge-write: re-lee lo último de disco
/// antes de escribir, así dos hidrataciones concurrentes no se pisan los inserts (mitiga el
/// lost-update sin un lock; el peor caso residual es re-embeber una skill, auto-sanable).
fn merge_emb_cache(fresh: Vec<(String, Vec<f32>)>) {
    if fresh.is_empty() {
        return;
    }
    let mut cache = load_emb_cache();
    for (name, emb) in fresh {
        cache.insert(name, emb);
    }
    save_emb_cache(&cache);
}

/// **Hidratación en frío** (cold registry → on-demand semantic hydration). En vez de
/// cargar TODAS las skills forjadas en el host —lo que infla el catálogo y el contexto
/// del modelo sin límite a medida que AION acumula capacidades—, registra solo las `k`
/// más relevantes a `task` por similitud semántica. Mantiene la caja de herramientas
/// ACTIVA pequeña aunque el almacén sea enorme: es el patrón de frontera 2026 para tools
/// (RAG-MCP / Tool Search), clave con un LLM local de ventana acotada como Gemma 12B.
/// Los embeddings de descripción se cachean en `skills_emb.jsonl` para no re-embeber.
/// Devuelve cuántas skills hidrató; fail-soft a `load_all` si Ollama no responde.
pub async fn hydrate_relevant(host: &WasmSkillHost, task: &str, k: usize) -> usize {
    let records = load_records();
    if records.len() <= HYDRATE_FLOOR.max(k) {
        return load_all(host); // pocas skills: filtrar no compensa el coste de embeber
    }
    let embedder = aion_memory::OllamaEmbedder::default_local();
    let Ok(q) = embedder.embed(task).await else {
        return load_all(host);
    };
    let cache = load_emb_cache();
    let mut fresh: Vec<(String, Vec<f32>)> = Vec::new();
    let mut scored: Vec<(f32, StoredSkill)> = Vec::new();
    let mut fails = 0usize;
    for s in records {
        // Dim-check: una entrada cacheada de OTRO modelo de embeddings (dimensión distinta)
        // no es comparable → se re-embebe en vez de dar un coseno basura (auto-sana si
        // cambió AION_EMBED_MODEL sin invalidar el cache).
        let cached = cache.get(&s.name).filter(|e| e.len() == q.len());
        let score = match cached {
            Some(e) => aion_memory::cosine(&q, e), // sin clonar: cosine toma &[f32]
            None => {
                let text = format!("{} — {}", s.name, s.description);
                match embedder.embed(&text).await {
                    Ok(e) => {
                        let sc = aion_memory::cosine(&q, &e);
                        fresh.push((s.name.clone(), e));
                        sc
                    }
                    Err(_) => {
                        fails += 1;
                        continue;
                    }
                }
            }
        };
        scored.push((score, s));
    }
    // Si el embedder falló a mitad en demasiadas, el top-k estaría sesgado por exclusión
    // SILENCIOSA (una skill clave podría no registrarse nunca). Mejor caer al fallback
    // completo que hidratar un subconjunto arbitrario creyendo que elegimos las mejores.
    if fails > 0 && scored.len() < HYDRATE_FLOOR.max(k) {
        return load_all(host);
    }
    merge_emb_cache(fresh); // read-merge-write (tolera concurrencia)
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut n = 0;
    for (_, s) in scored.into_iter().take(k) {
        if host
            .register(
                SkillManifest {
                    name: s.name.clone(),
                    description: s.description.clone(),
                },
                s.wat.as_bytes(),
            )
            .is_ok()
        {
            n += 1;
        }
    }
    n
}

/// Nombres + descripciones de las skills persistidas (para mostrarlas al agente).
pub fn catalog() -> Vec<(String, String)> {
    load_records()
        .into_iter()
        .map(|s| (s.name, s.description))
        .collect()
}

fn load_records() -> Vec<StoredSkill> {
    match std::fs::read_to_string(store_path()) {
        Ok(t) => t
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
        Err(_) => vec![],
    }
}
