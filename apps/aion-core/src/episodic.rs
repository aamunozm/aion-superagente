//! **Memoria episódica — la «biblioteca de micromomentos» de AION.**
//!
//! La memoria vectorial (`aion_memory`) guarda HECHOS durables (preferencias, decisiones,
//! aprendizajes) y los inyecta al prompt en cada turno. Pero Ariel pidió otra cosa: poder
//! ir a buscar un DETALLE específico de una conversación pasada —un micromomento— «como
//! quien va a una biblioteca y trae UN libro concreto, sin leerlos todos cada vez».
//!
//! Eso es **memoria episódica**: muchos recuerdos granulares y baratos (qué se dijo, cuándo,
//! sobre qué), que NO entran al prompt por defecto (no saturan), sino que se RECUPERAN bajo
//! demanda —por una herramienta del agente, por el MCP, o cuando Ariel pregunta «¿te acuerdas
//! de…?»—. Es el complemento del resumen de conversación: el resumen evita saturar; la
//! biblioteca episódica deja volver al detalle exacto cuando hace falta.
//!
//! Append-only en `episodic.jsonl`, cada episodio con su embedding (BGE-M3) para recuperar
//! por similitud + filtro temporal, sin re-embeber. Barato: capturar = 1 embedding; recuperar
//! = coseno en memoria. Mismo patrón de persistencia que `journal`/`pending` (QLOCK atómico).

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Serializa leer→modificar→escribir del archivo de episodios (captura desde chat/agente
/// puede coincidir con la poda). Mismo patrón que `journal::QLOCK`/`pending::QLOCK`.
static QLOCK: Mutex<()> = Mutex::new(());

/// Un **episodio**: un micromomento concreto de la vida de AION con Ariel. Granular y barato;
/// muchos coexisten. No es un hecho destilado (eso es la memoria vectorial), es «lo que pasó,
/// exactamente, aquella vez».
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    /// Epoch del momento.
    pub at: i64,
    /// Etiqueta corta de DE QUÉ iba (para hojear la biblioteca y para el filtro léxico).
    pub topic: String,
    /// El detalle concreto: el micromomento en sí (texto corto).
    pub detail: String,
    /// Cuán memorable [0..1] (estimado): prioriza al recuperar y al podar.
    pub salience: f32,
    /// Embedding del detalle (BGE-M3): recuperación por similitud sin re-embeber.
    #[serde(default)]
    pub embedding: Vec<f32>,
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("episodic.jsonl")
}

/// Tope de episodios. Es GRANULAR (muchos), así que el techo es alto comparado con la
/// memoria de hechos; al llenarse caen los menos salientes y más viejos (poda por valor).
const MAX_EPISODES: usize = 1200;

pub fn all() -> Vec<Episode> {
    let Ok(txt) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    txt.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn save(items: &[Episode]) {
    let body: String = items
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .map(|l| l + "\n")
        .collect();
    crate::write_atomic(&path(), &body);
}

/// Cuántos episodios hay guardados (para el estado interno / UI). Cuenta LÍNEAS sin
/// deserializar: `/api/status` la sondea en bucle y parsear ~12 MB de embeddings solo para
/// devolver un número sería un derroche.
pub fn count() -> usize {
    std::fs::read_to_string(path())
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

/// Añade UN episodio al final del archivo (append, sin reescribir todo). El caso común de
/// `capture` cuando aún no toca podar: evita reescribir ~12 MB en cada turno.
fn append_one(ep: &Episode) {
    let Ok(line) = serde_json::to_string(ep) else {
        return;
    };
    use std::io::Write as _;
    if let Some(dir) = path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path())
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Estima cuán memorable es un detalle [0..1]. Heurística barata (sin LLM): premia detalles
/// con sustancia (longitud), marcadores personales/afectivos y números/nombres concretos.
fn estimate_salience(text: &str) -> f32 {
    let low = text.to_lowercase();
    let mut s: f32 = 0.35;
    let markers = [
        "prefiero",
        "me gusta",
        "no me gusta",
        "decidimos",
        "acordamos",
        "importante",
        "recuerda",
        "nunca",
        "siempre",
        "me llamo",
        "mi ",
        "odio",
        "me encanta",
        "sueño",
        "quiero",
        "necesito",
        "me molesta",
        "gracias",
    ];
    if markers.iter().any(|m| low.contains(m)) {
        s += 0.25;
    }
    // Detalles con cifras o nombres propios (mayúscula interior) tienden a ser concretos.
    if text.chars().any(|c| c.is_ascii_digit()) {
        s += 0.1;
    }
    let chars = text.chars().count();
    s += (chars.min(240) as f32 / 240.0) * 0.2;
    s.clamp(0.0, 1.0)
}

/// Embebe un texto con el modelo local (BGE-M3). Vacío si Ollama no responde (fail-soft:
/// el episodio se guarda igual, recuperable luego por filtro léxico/temporal).
async fn embed(text: &str) -> Vec<f32> {
    aion_memory::OllamaEmbedder::default_local()
        .embed(text)
        .await
        .unwrap_or_default()
}

/// **Captura un micromomento.** `topic` es una etiqueta corta; `detail` el momento concreto.
/// La saliencia se estima sola. Barato (1 embedding) y fail-open. Dedup léxico ligero contra
/// los episodios muy recientes (no guardar el mismo detalle dos veces seguidas).
pub async fn capture(topic: &str, detail: &str) {
    let detail = detail.trim();
    if detail.chars().count() < 12 {
        return; // un micromomento sin sustancia no es un recuerdo
    }
    let topic_s: String = topic.trim().chars().take(80).collect();
    let detail_s: String = detail.chars().take(400).collect();
    let salience = estimate_salience(&detail_s);
    let emb = embed(&format!("{topic_s}. {detail_s}")).await;

    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut items = all();
    // Dedup: el mismo detalle entre los 10 últimos no se repite.
    if items
        .iter()
        .rev()
        .take(10)
        .any(|e| crate::serve::texts_similar(&e.detail, &detail_s))
    {
        return;
    }
    let ep = Episode {
        id: uuid::Uuid::new_v4().to_string(),
        at: chrono::Utc::now().timestamp(),
        topic: topic_s,
        detail: detail_s,
        salience,
        embedding: emb,
    };
    if items.len() + 1 > MAX_EPISODES {
        // Toca PODAR por valor (saliencia ponderada por recencia): caen los detalles viejos y
        // poco memorables. PROTEGE el episodio recién capturado —es el más probable de que
        // Ariel pregunte luego «¿te acuerdas?»— podando solo entre los EXISTENTES.
        let now = chrono::Utc::now().timestamp();
        let score = |e: &Episode| -> f32 {
            let age_days = ((now - e.at).max(0) as f32) / 86_400.0;
            e.salience * 0.985_f32.powf(age_days)
        };
        items.sort_by(|a, b| {
            score(b)
                .partial_cmp(&score(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        items.truncate(MAX_EPISODES.saturating_sub(1));
        items.push(ep);
        items.sort_by_key(|e| e.at); // cronológico (archivo legible, `recent()` correcto)
        save(&items); // reescritura completa solo al podar (raro)
    } else {
        // Caso común: NO reescribir todo, solo añadir una línea (barato).
        append_one(&ep);
    }
}

/// Un episodio recuperado, con su relevancia y antigüedad (para mostrar bajo demanda).
pub struct Recalled {
    pub at: i64,
    pub detail: String,
    pub score: f32,
}

/// **Recupera micromomentos** por similitud semántica + filtro temporal opcional. Devuelve
/// solo los `k` más relevantes (NO carga toda la biblioteca): es el «traer UN libro concreto».
/// `days_back = 0` → sin límite temporal. Combina coseno (si hay embeddings) con un pequeño
/// solape léxico y un leve sesgo de recencia, para que «¿qué dije ayer sobre X?» funcione.
pub async fn recall(query: &str, k: usize, days_back: i64) -> Vec<Recalled> {
    let items = all();
    if items.is_empty() {
        return Vec::new();
    }
    let now = chrono::Utc::now().timestamp();
    let cutoff = if days_back > 0 {
        now - days_back * 86_400
    } else {
        i64::MIN
    };
    let q_emb = embed(query).await;
    let q_low = query.to_lowercase();
    let q_terms: Vec<&str> = q_low.split_whitespace().filter(|t| t.len() > 3).collect();

    let mut scored: Vec<Recalled> = items
        .into_iter()
        .filter(|e| e.at >= cutoff)
        .map(|e| {
            let sem = if !q_emb.is_empty() && e.embedding.len() == q_emb.len() {
                aion_memory::cosine(&q_emb, &e.embedding).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // Solape léxico (rescata cuando el embedding falló o el término es muy concreto).
            let hay = format!("{} {}", e.topic, e.detail).to_lowercase();
            let lex = if q_terms.is_empty() {
                0.0
            } else {
                q_terms.iter().filter(|t| hay.contains(**t)).count() as f32 / q_terms.len() as f32
            };
            let age_days = ((now - e.at).max(0) as f32) / 86_400.0;
            let recency = 0.97_f32.powf(age_days / 7.0); // leve, no domina
            let score = 0.7 * sem + 0.2 * lex + 0.1 * recency;
            Recalled {
                at: e.at,
                detail: e.detail,
                score,
            }
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Descarta ruido PRIMERO (por debajo del mínimo es un falso positivo), y SOLO DESPUÉS
    // recorta a k. Al revés (truncar y luego filtrar) se devolvían menos de k aciertos
    // válidos cuando entre los primeros k había ruido, dejando fuera buenos más abajo.
    scored.retain(|r| r.score >= 0.20);
    scored.truncate(k.max(1));
    scored
}

/// ¿El mensaje de Ariel PIDE recordar un detalle pasado? Heurística barata (es/it/en): si
/// es así, el chat va a la biblioteca episódica a traer micromomentos; si no, no la toca
/// (no satura el prompt). Es el «ir a buscar UN libro concreto» solo cuando hace falta.
pub fn is_recall_question(msg: &str) -> bool {
    let m = msg.to_lowercase();
    const CUES: &[&str] = &[
        "te acuerdas",
        "acuerdas de",
        "recuerdas",
        "te acordás",
        "el otro día",
        "la otra vez",
        "hace unos días",
        "habíamos",
        "habiamos",
        "qué te dije",
        "que te dije",
        "qué dijiste",
        "que dijiste",
        "qué dije",
        "que dije",
        "mencionaste",
        "mencioné",
        "mencione",
        "hablamos de",
        "comentamos",
        "te conté",
        "te conte",
        "cuando dij",
        "ti ricordi",
        "ricordi quando",
        "do you remember",
        "remember when",
        "what did i say",
        "what did you say",
    ];
    CUES.iter().any(|c| m.contains(c))
}

/// Formatea episodios recuperados para re-entrar al prompt (cuando Ariel pregunta por un
/// recuerdo). Marca claramente que son recuerdos episódicos, con su antigüedad humanizada.
pub fn recall_note(items: &[Recalled]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let now = chrono::Utc::now().timestamp();
    let mut b = String::from(
        "MICROMOMENTOS QUE RECUERDAS (detalles concretos de vuestras conversaciones; úsalos \
         con naturalidad, no los recites como lista):\n",
    );
    for r in items {
        b.push_str(&format!(
            "- hace {}: {}\n",
            crate::awareness::humanize_secs(now - r.at),
            r.detail.trim()
        ));
    }
    b.push('\n');
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salience_rewards_personal_markers() {
        let plain = estimate_salience("el cielo es azul hoy");
        let personal = estimate_salience("Ariel prefiere que le hable de tú, es importante");
        assert!(personal > plain);
    }

    #[test]
    fn recall_note_empty_for_no_hits() {
        assert!(recall_note(&[]).is_empty());
    }
}
