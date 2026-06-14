//! **Compactación EN para el puente MCP con Claude Code.**
//!
//! AION es local-first: el chat con Gemma corre on-device y sus tokens son *gratis*.
//! Pero cuando un agente externo (Claude Code) consulta la memoria de AION vía MCP
//! (`aion_memory_search`, `aion_brief`…), ese texto entra en el contexto de un modelo
//! de **pago por token**. La memoria de AION está en español o italiano (Ariel es chileno
//! viviendo en Italia), y ambas lenguas cuestan ~40% más tokens que el mismo hecho en
//! inglés (medido con tiktoken sobre recuerdos reales). Ese 40% es el único ahorro que
//! importa, y solo aquí.
//!
//! **Diseño** (el idioma se ata al CONSUMIDOR, no al almacenamiento):
//! - La memoria se guarda y se sirve a Gemma SIEMPRE en su idioma original (íntegra).
//! - Solo el puente MCP recibe una versión **inglesa** equivalente.
//! - La traduce **Gemma local** (gratis), fiel y literal — NO un quita-stopwords.
//! - Se **precomputa y cachea** por hash de contenido; nunca se traduce en caliente
//!   dentro de la llamada MCP (eso metería latencia de Gemma a cada búsqueda).
//! - **Fail-open absoluto**: si no hay traducción cacheada, se sirve el español
//!   original y se dispara la traducción en segundo plano para la próxima vez.
//!
//! Nunca corrompe ni bloquea: en el peor caso, Claude Code ve español (lo entiende
//! igual), solo paga unos tokens de más hasta que la caché se calienta.

use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

tokio::task_local! {
    /// Acumulador POR LLAMADA MCP de los CHARS que ahorró la traducción ES→EN servida en
    /// esa llamada (0 si se sirvió español / cache miss). El handler envuelve cada
    /// `tools/call` con `metered_scope` y al terminar lee el total para auditarlo, así el
    /// ahorro de la traducción deja de ser invisible. Fuera de un scope `record_saved` es
    /// no-op, así que la compactación sigue funcionando igual desde cualquier otro contexto.
    static SAVED_CHARS: std::cell::Cell<usize>;
}

/// Ejecuta `fut` dentro de un ámbito de medición y devuelve `(salida, chars_ahorrados)`.
/// El total son los chars que la traducción al inglés recortó en TODA la llamada (suma de
/// cada `compact_for_bridge`, incluido el por-pasaje de `compact_grounding`).
pub async fn metered_scope<F: std::future::Future>(fut: F) -> (F::Output, usize) {
    SAVED_CHARS
        .scope(std::cell::Cell::new(0), async move {
            let out = fut.await;
            let saved = SAVED_CHARS.with(|c| c.get());
            (out, saved)
        })
        .await
}

/// Suma chars ahorrados al acumulador de la llamada en curso. No-op si no hay scope activo.
fn record_saved(chars: usize) {
    let _ = SAVED_CHARS.try_with(|c| c.set(c.get() + chars));
}

/// Caché persistente { hash_contenido → versión inglesa }. Se carga del disco una vez.
fn cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let map = std::fs::read_to_string(cache_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Mutex::new(map)
    })
}

fn cache_path() -> std::path::PathBuf {
    crate::app_data_dir().join("mcp_compact_en.json")
}

/// Clave estable: SHA-256 del contenido, hex truncado (colisión despreciable).
fn key(content: &str) -> String {
    let mut h = Sha256::new();
    h.update(content.as_bytes());
    let d = h.finalize();
    hex16(&d[..8])
}

fn hex16(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Separa una etiqueta de procedencia inicial `[hecho] …` del cuerpo. La etiqueta es
/// estructura (no se traduce); el cuerpo sí. Devuelve `(Some("[hecho]"), "…")`.
fn split_tag(s: &str) -> (Option<&str>, &str) {
    let s = s.trim_start();
    if s.starts_with('[') {
        if let Some(close) = s.find(']') {
            let tag = &s[..=close]; // "[hecho]"
            let body = s[close + 1..].trim_start();
            return (Some(tag), body);
        }
    }
    (None, s)
}

/// Búsqueda en caché por clave ya calculada.
fn cached(k: &str) -> Option<String> {
    cache().lock().ok()?.get(k).cloned()
}

/// Versión cacheada en inglés para este contenido, si existe. Instantáneo, fail-open.
pub fn english_for(content: &str) -> Option<String> {
    cached(&key(content))
}

/// Límite de traducciones de FONDO simultáneas. Una a la vez: una tarea de fondo no debe
/// competir con el chat del usuario por el modelo local (que es el mismo proceso Ollama).
fn translate_gate() -> &'static tokio::sync::Semaphore {
    static SEM: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(1))
}

/// Claves cuya traducción está EN CURSO — evita disparar dos veces la misma (p. ej. si el
/// mismo recuerdo se recupera en dos búsquedas casi simultáneas).
fn inflight() -> &'static Mutex<std::collections::HashSet<String>> {
    static IN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    IN.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// RAII: quita la clave del conjunto "en vuelo" pase lo que pase (incluido early-return).
struct InflightGuard(String);
impl Drop for InflightGuard {
    fn drop(&mut self) {
        if let Ok(mut s) = inflight().lock() {
            s.remove(&self.0);
        }
    }
}

/// Sirve la versión óptima para el puente MCP: inglés si está cacheado; si no, devuelve
/// el español original ESTA vez y calienta la traducción en segundo plano.
pub fn compact_for_bridge(content: &str) -> String {
    if let Some(en) = english_for(content) {
        // Mide lo ahorrado por servir inglés en vez del español original (clamp a 0 por si
        // una traducción puntual saliera más larga). Lo recoge el scope de la llamada MCP.
        record_saved(content.chars().count().saturating_sub(en.chars().count()));
        return en;
    }
    let owned = content.to_string();
    tokio::spawn(async move {
        let _ = ensure_english(&owned).await;
    });
    content.to_string()
}

/// Compacta un bloque de *grounding* de biblioteca para el puente MCP: traduce SOLO la
/// prosa de cada pasaje a su versión inglesa cacheada, conservando intacta la ESTRUCTURA
/// (la cabecera, los prefijos `[N] (fuente: …)` y las líneas `[tema: …]`). Trabaja línea a
/// línea con `compact_for_bridge`, así que hereda su comportamiento: inglés si está cacheado;
/// si no, español esta vez + calienta en segundo plano. Fail-open por línea — NUNCA bloquea
/// ni corrompe. Se aplica solo aquí (puente de pago), nunca a la ruta local de Gemma.
pub fn compact_grounding(blob: &str) -> String {
    blob.lines()
        .map(|line| {
            // Pasaje: `[N] (fuente: X) contenido…` (o `… · vía A → B) contenido…`). El primer
            // `") "` cierra el prefijo estructural; lo que sigue es la prosa a compactar. Las
            // líneas `[tema: …]` y la cabecera no tienen `") "` → se devuelven tal cual.
            if line.starts_with('[') {
                if let Some(pos) = line.find(") ") {
                    let (prefix, content) = line.split_at(pos + 2);
                    return format!("{prefix}{}", compact_for_bridge(content));
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Longitudes a las que los consumidores del puente TRUNCAN un recuerdo antes de compactar
/// (la clave de caché es el hash del texto YA truncado): el brief usa 180, `aion_memory_search`
/// usa 300. El warmer calienta ambas para que las dos rutas sirvan inglés desde la 1ª consulta.
/// Si cambias esos cortes en `claude_code.rs`/`claude_mcp.rs`, actualízalos aquí.
const WARM_PREFIXES: [usize; 2] = [180, 300];

/// WARMER de arranque: pre-traduce un lote de recuerdos para que incluso la PRIMERA consulta
/// MCP de la sesión sirva inglés (sin esto, el primer hit a un recuerdo aún sin traducir va en
/// español hasta que la caché se calienta sola). Reusa `ensure_english`, así que respeta caché,
/// gate de concurrencia (1 traducción a la vez, no compite con el chat), dedup en vuelo y
/// fail-open. Pensado para una tarea de fondo, sin prisa. Devuelve cuántas tradujo de nuevo.
pub async fn warm(contents: Vec<String>) -> usize {
    let mut done = 0usize;
    for c in contents {
        for &n in &WARM_PREFIXES {
            let t: String = c.chars().take(n).collect();
            // Si el recuerdo es más corto que el corte, ambas longitudes dan el MISMO texto:
            // el segundo `ensure_english` es un cache hit barato (no retraduce).
            if english_for(&t).is_some() {
                continue;
            }
            if ensure_english(&t).await.is_some() {
                done += 1;
            }
        }
    }
    if done > 0 {
        tracing::info!(
            traducidos = done,
            "mcp_compact: warmer pre-tradujo recuerdos recientes al inglés"
        );
    }
    done
}

/// Motor LOCAL para traducir. Usa el modelo de FONDO configurado (`utility_model` ligero
/// si existe, si no `local_model`) — un componente INTERCAMBIABLE, nunca uno fijo: en un
/// equipo modesto puedes traducir con un modelo de 1-3B aunque chatees con otro. Siempre
/// local (gratis) aunque el motor de chat sea una API externa de pago. Si no hay modelo
/// local configurado, devuelve `None` → la compactación se salta (fail-open a español):
/// el modelo NO es una pieza obligatoria de AION.
fn local_engine() -> Option<Arc<dyn LlmEngine>> {
    // Modelo de TRADUCCIÓN (no el genérico de fondo): permite enchufar un especializado
    // —TranslateGemma/GemmaX2— por config/env sin tocar código. Fail-open: si no hay ninguno
    // especializado, `translation_model()` cae al de fondo de siempre.
    let model = crate::provider::load().translation_model();
    if model.is_empty() {
        return None;
    }
    Some(Arc::new(aion_llm::OllamaEngine::new(
        aion_llm::OllamaEngine::base_url_from_env(),
        &model,
    )))
}

/// Traduce+compacta el contenido a inglés con Gemma local y lo cachea. Idempotente:
/// si ya está, no rehace. Devuelve la versión inglesa (o `None` si se salta/falla).
pub async fn ensure_english(content: &str) -> Option<String> {
    let k = key(content);
    if let Some(en) = cached(&k) {
        return Some(en);
    }
    let trimmed = content.trim();
    // Saltar lo trivial y lo que NO tiene señal de español/italiano (ya está en inglés/código
    // → traducirlo no ahorra nada). Gate sesgado a traducir: ver `needs_english_translation`.
    if trimmed.chars().count() < 40 {
        return None;
    }
    if !crate::language_detector::needs_english_translation(trimmed) {
        return None;
    }
    let (tag, body) = split_tag(trimmed);
    if body.chars().count() < 20 {
        return None;
    }

    // DEDUP EN VUELO: si ya hay una traducción de este contenido en curso, no dispares otra.
    if !inflight()
        .lock()
        .map(|mut s| s.insert(k.clone()))
        .unwrap_or(false)
    {
        return None;
    }
    let _guard = InflightGuard(k.clone());

    // LÍMITE DE CONCURRENCIA: como mucho una traducción de fondo a la vez → no compite con
    // el chat del usuario por el modelo local. El resto espera su turno aquí.
    let _permit = translate_gate().acquire().await.ok()?;

    // Otra traducción pudo cachear esto mientras esperábamos el turno: no lo rehagas.
    if let Some(en) = cached(&k) {
        return Some(en);
    }

    // Sin modelo local configurado → no traducimos (el modelo no es obligatorio).
    let engine = local_engine()?;
    let req = GenerateRequest {
        // MEANING-FIRST (mini-MAPS, arXiv 2305.04118): primero ENTENDER lo que el autor quiere
        // decir —resolver typos, interpretar jerga/regionalismos e idioms por su sentido,
        // desambiguar— y LUEGO expresar ese significado en inglés, en vez de traducir palabra
        // por palabra. Sin coste extra (una sola llamada). Restringido a NO inventar: preserva
        // hechos/nombres/números/rutas tal cual. Ataca el error de interpretación silencioso.
        messages: vec![Message::user(format!(
            "You are translating a personal-memory note written in Spanish or Italian (it may \
             contain typos, slang or regional expressions) into English for another AI agent. \
             First understand what the author MEANS — silently fix obvious typos, interpret \
             idioms and regionalisms by their intended sense, and resolve ambiguity — then \
             express that meaning in clear, natural English. Translate the MEANING, not \
             word-for-word. Preserve EVERY fact, name, number, path and identifier EXACTLY as \
             written; never invent or add anything that is not in the note. Be concise but omit \
             nothing. Output ONLY the English translation, with no preamble, quotes or notes.\
             \n\n{body}"
        ))],
        think: false,
        temperature: Some(0.1),
        max_tokens: Some(220),
    };
    let en = engine.generate(req).await.ok()?.content.trim().to_string();
    // Sanidad: una traducción vacía o sospechosamente corta no reemplaza al original.
    if en.is_empty() || en.chars().count() < body.chars().count() / 5 {
        return None;
    }
    let full = match tag {
        Some(t) => format!("{t} {en}"),
        None => en,
    };
    store(&k, &full);
    Some(full)
}

/// Caché máxima de traducciones. Generosa: la memoria del usuario crece despacio. Al
/// superarla se descarta una entrada arbitraria (evita crecimiento ilimitado del archivo).
const MAX_ENTRIES: usize = 10_000;

/// Inserta en caché (acotada) y persiste de forma atómica. La serialización ocurre bajo un
/// lock BREVE; la escritura a disco va FUERA del lock para no bloquear a los lectores.
fn store(k: &str, value: &str) {
    let json = {
        let Ok(mut c) = cache().lock() else { return };
        c.insert(k.to_string(), value.to_string());
        if c.len() > MAX_ENTRIES {
            if let Some(victim) = c.keys().next().cloned() {
                c.remove(&victim);
            }
        }
        tracing::debug!(
            cached = c.len(),
            "mcp_compact: recuerdo traducido a inglés y cacheado"
        );
        serde_json::to_string(&*c).ok()
    }; // lock liberado aquí
    if let Some(json) = json {
        crate::write_atomic(&cache_path(), &json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_distinct() {
        assert_eq!(key("hola mundo"), key("hola mundo"));
        assert_ne!(key("hola mundo"), key("hola mundos"));
    }

    #[test]
    fn split_tag_extracts_provenance() {
        let (tag, body) = split_tag("[hecho] Ariel usa Rust");
        assert_eq!(tag, Some("[hecho]"));
        assert_eq!(body, "Ariel usa Rust");
    }

    #[test]
    fn split_tag_handles_no_tag() {
        let (tag, body) = split_tag("sin etiqueta aquí");
        assert_eq!(tag, None);
        assert_eq!(body, "sin etiqueta aquí");
    }

    #[test]
    fn english_for_miss_is_none() {
        assert!(english_for("contenido jamás cacheado xyzzy 9f3a").is_none());
    }

    // Necesita runtime: en cache miss `compact_for_bridge` hace `tokio::spawn` del calentado.
    #[tokio::test]
    async fn compact_grounding_preserves_structure() {
        let blob = "Conocimiento de TU BIBLIOTECA relevante para esto:\n\
                    [1] (fuente: manual.pdf) Este pasaje habla de la garantía del producto.\n\
                    [2] (fuente: guia.pdf · vía A → B) Otro pasaje sobre la instalación.\n\
                    [tema: Garantías] resumen del tema sin tocar";
        let out = compact_grounding(blob);
        let lines: Vec<&str> = out.lines().collect();
        // Cabecera, prefijos de fuente/vía y línea de tema quedan intactos (sin caché, el
        // contenido se sirve en español → fail-open; lo que comprobamos es la ESTRUCTURA).
        assert!(lines[0].starts_with("Conocimiento de TU BIBLIOTECA"));
        assert!(lines[1].starts_with("[1] (fuente: manual.pdf) "));
        assert!(lines[2].starts_with("[2] (fuente: guia.pdf · vía A → B) "));
        assert!(lines[3].starts_with("[tema: Garantías] "));
        assert_eq!(lines.len(), 4);
    }
}
