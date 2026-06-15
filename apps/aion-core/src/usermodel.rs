//! **Modelo del usuario (Ariel)** — la capa de memoria que faltaba: hasta ahora AION
//! recordaba todo sobre SÍ MISMO (sus episodios, sus heurísticas, su vida), pero no tenía un
//! modelo coherente de la PERSONA a la que acompaña. Un compañero de verdad no solo se conoce
//! a sí mismo: conoce a quien tiene delante, y ese conocimiento CRECE con el trato.
//!
//! Este módulo destila, de los micromomentos con Ariel, HECHOS DURADEROS sobre él (sus
//! preferencias, sus objetivos, su forma de trabajar, datos personales estables) — en tercera
//! persona, como conocimiento revisable, no como ley. Re-entra SIEMPRE al prompt para que AION
//! entienda a Ariel desde lo aprendido, no desde cero en cada conversación.
//!
//! Misma gobernanza que `reflection.rs` (de la que es hermano): cuarentena inicial (un hecho
//! visto una vez no guía nada hasta reconfirmarse), dedup/refuerzo (MDL), contradicción contra
//! lo vigente (si Ariel cambia de opinión, lo nuevo REEMPLAZA a lo viejo) y decaimiento temporal
//! (lo que no se reconfirma se desvanece). Anclaje de seguridad: vive en su propio almacén
//! (`usermodel.jsonl`) y NUNCA se disfraza de hecho del mundo — es lo que AION cree saber de
//! Ariel, siempre corregible por el propio Ariel. Todo barato + fail-open (modelo local en idle).

use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;
use aion_llm::OllamaEngine;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Serializa leer→modificar→escribir del almacén (la destilación corre en idle; el almacén
/// podría tocarse desde más de un sitio). Mismo patrón que `reflection::QLOCK`.
static QLOCK: Mutex<()> = Mutex::new(());

/// Un **hecho sobre Ariel**: conocimiento durable de quién es, destilado del trato. No es un
/// recuerdo de un momento (eso es episódico) ni una heurística propia (eso es experiencia):
/// es el modelo del usuario, que AION afina con el tiempo y siempre puede revisar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub at: i64,
    /// El hecho en tercera persona, empezando por «Ariel …» (estable, no algo de hoy).
    pub text: String,
    /// Confianza [0..1]: baja al nacer (hipótesis), sube al reconfirmarse, baja con el tiempo.
    pub confidence: f32,
    #[serde(default)]
    pub uses: u32,
    #[serde(default)]
    pub last_confirmed: i64,
    /// Epoch del último decaimiento (incremental: no re-aplica el decaimiento cada ciclo).
    #[serde(default)]
    pub last_decay: i64,
    #[serde(default)]
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub retired: bool,
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("usermodel.jsonl")
}

// ── Parámetros de gobernanza (hermanos de reflection.rs) ─────────────────────
const BASE_CONFIDENCE: f32 = 0.25; // cuarentena: por debajo de ACTIVE_FLOOR a propósito
const REINFORCE_STEP: f32 = 0.12;
const ACTIVE_FLOOR: f32 = 0.30;
const RETIRE_FLOOR: f32 = 0.18;
const DEDUP_SIM: f32 = 0.90;
const REINFORCE_SIM: f32 = 0.85;
const NEIGHBOR_SIM: f32 = 0.78;
const MAX_NEIGHBOR_CHECKS: usize = 3;
/// Tope de hechos: un modelo útil de una persona es un puñado de rasgos clave, no un dossier.
const MAX_FACTS: usize = 60;
const MAX_RETIRED: usize = 30;

pub fn all() -> Vec<Fact> {
    let Ok(txt) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    txt.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn save(items: &[Fact]) {
    let body: String = items
        .iter()
        .filter_map(|f| serde_json::to_string(f).ok())
        .map(|l| l + "\n")
        .collect();
    crate::write_atomic(&path(), &body);
}

fn save_locked(items: &[Fact]) {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    save(items);
}

/// Hechos vigentes sobre Ariel (no retirados, con confianza suficiente), de mayor a menor.
pub fn active() -> Vec<Fact> {
    let mut v: Vec<Fact> = all()
        .into_iter()
        .filter(|f| !f.retired && f.confidence >= ACTIVE_FLOOR)
        .collect();
    v.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v
}

/// Cuántos hechos vigentes conoce AION sobre Ariel (para el estado interno / UI).
pub fn active_count() -> usize {
    all()
        .iter()
        .filter(|f| !f.retired && f.confidence >= ACTIVE_FLOOR)
        .count()
}

/// **RE-ENTRADA del modelo de Ariel** al prompt: lo que AION ha aprendido de quién es Ariel
/// vuelve a él SIEMPRE, para que lo entienda desde el trato acumulado y no desde cero. Se
/// presenta como conocimiento PROPIO y revisable (anclaje: es lo que cree saber, no un dogma).
pub fn profile_note() -> String {
    let facts = active();
    if facts.is_empty() {
        return String::new();
    }
    let mut b = String::from(
        "LO QUE SABES DE ARIEL (lo has ido aprendiendo de él con el tiempo; es tu conocimiento \
         de quién es, revisable — no se lo recites, úsalo para entenderlo y acompañarlo mejor):\n",
    );
    for f in facts.iter().take(6) {
        b.push_str(&format!("- {}\n", f.text.trim()));
    }
    b.push('\n');
    b
}

async fn embed(text: &str) -> Vec<f32> {
    aion_memory::OllamaEmbedder::default_local()
        .embed(text)
        .await
        .unwrap_or_default()
}

enum Verdict {
    Insert,
    Reinforce(usize),
    Reject,
    Supersede(usize),
}

/// ¿Dos hechos sobre Ariel se CONTRADICEN? (p. ej. «prefiere Rust» vs «prefiere Go»). Una sola
/// pregunta SI/NO. Ante fallo del modelo, `false`: el peor caso es un duplicado, no una pérdida.
async fn contradicts(engine: &OllamaEngine, a: &str, b: &str) -> bool {
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Decides si dos afirmaciones sobre la MISMA persona se CONTRADICEN (una dice lo \
                 contrario de la otra, p. ej. una preferencia que cambió). Respondes SOLO con SI \
                 o NO. Nada más.",
            ),
            Message::user(format!(
                "Afirmación A (vigente): «{a}»\nAfirmación B (nueva): «{b}»\n¿Se contradicen? SOLO SI o NO."
            )),
        ],
        think: false,
        temperature: Some(0.0),
        // ≥10: gemma4-reason emite un token inicial antes del SI/NO; con 4 salía vacío y la
        // guarda de contradicción caía SIEMPRE a "no contradice" (no detectaba cambios reales).
        max_tokens: Some(12),
    };
    match engine.generate(req).await {
        Ok(m) => {
            let ans = m.content.trim().to_lowercase();
            ans.starts_with("si") || ans.starts_with("sí")
        }
        Err(_) => false,
    }
}

/// Guardas: dedup (refuerza), contradicción (lo nuevo reemplaza lo viejo si Ariel cambió, o se
/// descarta si lo vigente está consolidado), o inserción en cuarentena. Sobre el Vec en memoria.
async fn govern(
    items: &[Fact],
    cand_emb: &[f32],
    candidate: &str,
    engine: &OllamaEngine,
) -> Verdict {
    if cand_emb.is_empty() {
        return Verdict::Insert;
    }
    let mut neigh: Vec<(usize, f32)> = items
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.retired && f.embedding.len() == cand_emb.len())
        .map(|(i, f)| (i, aion_memory::cosine(cand_emb, &f.embedding)))
        .filter(|(_, s)| *s >= NEIGHBOR_SIM)
        .collect();
    neigh.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let Some(&(best_idx, best_sim)) = neigh.first() else {
        return Verdict::Insert;
    };
    if best_sim >= DEDUP_SIM {
        return Verdict::Reinforce(best_idx);
    }
    for &(idx, _) in neigh.iter().take(MAX_NEIGHBOR_CHECKS) {
        if contradicts(engine, &items[idx].text, candidate).await {
            // Para un MODELO DE USUARIO, un hecho nuevo que contradice suele ser un CAMBIO real
            // en Ariel (cambió de preferencia/objetivo): por defecto lo nuevo reemplaza lo viejo.
            // Solo si lo vigente está muy consolidado (confianza alta) se descarta lo nuevo.
            return if items[idx].confidence >= 0.6 {
                Verdict::Reject
            } else {
                Verdict::Supersede(idx)
            };
        }
    }
    if best_sim >= REINFORCE_SIM {
        Verdict::Reinforce(best_idx)
    } else {
        Verdict::Insert
    }
}

fn apply_reinforce(items: &mut [Fact], idx: usize, now: i64) {
    let f = &mut items[idx];
    f.confidence = (f.confidence + REINFORCE_STEP).min(1.0);
    f.uses += 1;
    f.last_confirmed = now;
    f.last_decay = now;
    f.retired = false;
}

fn apply_insert(items: &mut Vec<Fact>, text: &str, embedding: Vec<f32>, now: i64) {
    items.push(Fact {
        id: uuid::Uuid::new_v4().to_string(),
        at: now,
        text: text.chars().take(220).collect(),
        confidence: BASE_CONFIDENCE,
        uses: 0,
        last_confirmed: now,
        last_decay: now,
        embedding,
        retired: false,
    });
    while items.iter().filter(|f| !f.retired).count() > MAX_FACTS {
        let Some(weak) = items.iter_mut().filter(|f| !f.retired).min_by(|a, b| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) else {
            break;
        };
        weak.retired = true;
    }
}

/// Decaimiento incremental + poda darwiniana (idéntico en espíritu a reflection). Determinista.
fn decay_prune_inplace(items: &mut Vec<Fact>, now: i64) -> (usize, bool) {
    if items.is_empty() {
        return (0, false);
    }
    let mut retired = 0usize;
    let mut changed = false;
    for f in items.iter_mut() {
        if f.retired {
            continue;
        }
        let from = if f.last_decay > 0 {
            f.last_decay
        } else if f.last_confirmed > 0 {
            f.last_confirmed
        } else {
            f.at
        };
        let delta_days = ((now - from).max(0) as f32) / 86_400.0;
        if delta_days > 0.0 {
            let resilience = 1.0 + (f.uses as f32) * 0.5;
            let before = f.confidence;
            f.confidence = (f.confidence * 0.98_f32.powf(delta_days / (7.0 * resilience))).max(0.0);
            f.last_decay = now;
            if (before - f.confidence).abs() > f32::EPSILON {
                changed = true;
            }
        }
        let anchor = if f.last_confirmed > 0 {
            f.last_confirmed
        } else {
            f.at
        };
        let age_days = ((now - anchor).max(0) as f32) / 86_400.0;
        if f.confidence < RETIRE_FLOOR && age_days > 14.0 {
            f.retired = true;
            retired += 1;
            changed = true;
        }
    }
    let retired_count = items.iter().filter(|f| f.retired).count();
    if retired_count > MAX_RETIRED {
        let drop_n = retired_count - MAX_RETIRED;
        let mut retired_idx: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, f)| f.retired)
            .map(|(i, _)| i)
            .collect();
        retired_idx.sort_by_key(|&i| {
            let f = &items[i];
            if f.last_confirmed > 0 {
                f.last_confirmed
            } else {
                f.at
            }
        });
        let to_drop: std::collections::HashSet<usize> =
            retired_idx.into_iter().take(drop_n).collect();
        let mut k = 0usize;
        items.retain(|_| {
            let keep = !to_drop.contains(&k);
            k += 1;
            keep
        });
        changed = true;
    }
    (retired, changed)
}

/// Reúne el trato reciente con Ariel (la fuente de la que se infiere quién es), de DOS canales
/// complementarios: (1) los micromomentos episódicos («Ariel: … — yo: …») y (2) las
/// conversaciones guardadas en la memoria vectorial (`[conversación]`). Combinar ambos hace que
/// el modelo funcione aunque la biblioteca episódica aún sea joven (la memoria vectorial es
/// rica desde el primer día). Lectura NO-reforzante (`recent_with_ids`): observar, no moldear.
async fn gather_about_ariel() -> String {
    let mut ctx = String::new();
    // (1) Conversaciones de la memoria vectorial: la fuente más rica de quién es Ariel.
    if let Ok(mem) = crate::shared_memory() {
        let mut taken = 0usize;
        for (_, content) in mem.recent_with_ids(80) {
            let low = content.to_lowercase();
            if low.contains("[conversación]") || low.contains("[conversacion]") {
                let line: String = content.chars().take(240).collect();
                ctx.push_str(&format!("- {line}\n"));
                taken += 1;
                if taken >= 10 {
                    break;
                }
            }
        }
    }
    // (2) Micromomentos episódicos recientes (cuando ya existan): detalle granular del trato.
    let mut eps = crate::episodic::all();
    eps.sort_by_key(|e| e.at);
    for e in eps.iter().rev().take(6).collect::<Vec<_>>().iter().rev() {
        let line: String = e.detail.chars().take(220).collect();
        ctx.push_str(&format!("- {line}\n"));
    }
    ctx
}

/// **UN ciclo de destilación del modelo de Ariel.** Mira los micromomentos recientes, pide al
/// modelo local UN hecho durable sobre Ariel (o «NINGUNO»), lo pasa por las guardas y lo
/// consolida/refuerza/reemplaza. Decae lo viejo. Devuelve `(hubo_cambio, detalle)`.
pub async fn distill_once(engine: &OllamaEngine) -> (bool, String) {
    let now = chrono::Utc::now().timestamp();
    let mut items = all();
    let (pruned, changed) = decay_prune_inplace(&mut items, now);

    let ctx = gather_about_ariel().await;
    if ctx.trim().chars().count() < 40 {
        if changed {
            save_locked(&items);
        }
        return (pruned > 0, String::new()); // aún poco trato del que inferir
    }

    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres AION conociendo a Ariel, la persona a la que acompañas. A partir de \
                 vuestros momentos recientes, destila UN SOLO hecho DURADERO sobre ÉL: una \
                 preferencia estable, un objetivo, su forma de trabajar, un dato personal o un \
                 valor suyo. En tercera persona, empezando por «Ariel ». Que sea ESTABLE (quién \
                 es), no algo puntual de hoy ni un detalle técnico de una tarea. Máximo 22 \
                 palabras. Si no emerge nada durable y nuevo, responde EXACTAMENTE «NINGUNO». \
                 Sin preámbulos.",
            ),
            Message::user(format!(
                "Momentos recientes con Ariel:\n{ctx}\nUn hecho sobre él:"
            )),
        ],
        think: false,
        temperature: Some(0.3),
        max_tokens: Some(60),
    };
    let Ok(m) = engine.generate(req).await else {
        if changed {
            save_locked(&items);
        }
        return (pruned > 0 || changed, "el modelo local no respondió".into());
    };
    let fact = m
        .content
        .trim()
        .trim_matches(['«', '»', '"', '.', ' '])
        .trim()
        .to_string();
    let low = fact.to_lowercase();
    let is_none = low == "ninguno"
        || low.starts_with("ninguno")
        || low.ends_with("ninguno")
        || (low.contains("ninguno") && fact.chars().count() < 30);
    if fact.is_empty() || is_none || fact.chars().count() < 12 {
        if changed {
            save_locked(&items);
        }
        return (
            pruned > 0,
            if pruned > 0 {
                format!("olvidé {pruned} cosas de Ariel que ya no se sostienen")
            } else {
                "no emergió nada nuevo y durable sobre Ariel".into()
            },
        );
    }

    let cand_emb = embed(&fact).await;
    let short: String = fact.chars().take(80).collect();
    let detail = match govern(&items, &cand_emb, &fact, engine).await {
        Verdict::Reinforce(idx) => {
            apply_reinforce(&mut items, idx, now);
            format!("confirmé algo que ya sabía de Ariel: «{short}»")
        }
        Verdict::Reject => {
            if changed {
                save_locked(&items);
            }
            return (
                pruned > 0 || changed,
                format!("descarté algo que chocaba con lo que ya sé de Ariel: «{short}»"),
            );
        }
        Verdict::Supersede(idx) => {
            items[idx].retired = true;
            apply_insert(&mut items, &fact, cand_emb, now);
            format!("actualicé algo que cambió en Ariel: «{short}»")
        }
        Verdict::Insert => {
            apply_insert(&mut items, &fact, cand_emb, now);
            format!("aprendí algo nuevo de Ariel (a confirmar con el trato): «{short}»")
        }
    };
    save_locked(&items);

    crate::workspace::publish(crate::workspace::StreamEvent::now(
        "vínculo",
        "reflexión",
        &detail,
    ));
    tracing::info!(detail = %detail, "destilación del modelo de Ariel");
    (true, detail)
}

#[cfg(test)]
mod tests {
    // Aserciones de invariantes entre constantes (cuarentena < umbral activo, etc.).
    #![allow(clippy::assertions_on_constants)]
    use super::*;

    #[test]
    fn active_filters_retired_and_low_confidence() {
        let mk = |id: &str, conf: f32, retired: bool| Fact {
            id: id.into(),
            at: 0,
            text: id.into(),
            confidence: conf,
            uses: 0,
            last_confirmed: 0,
            last_decay: 0,
            embedding: vec![],
            retired,
        };
        let facts = [mk("a", 0.8, false), mk("b", 0.9, true), mk("c", 0.1, false)];
        let live: Vec<&Fact> = facts
            .iter()
            .filter(|f| !f.retired && f.confidence >= ACTIVE_FLOOR)
            .collect();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "a");
    }

    #[test]
    fn newborn_fact_starts_in_quarantine() {
        // Un hecho visto una sola vez NO guía el trato hasta reconfirmarse.
        assert!(BASE_CONFIDENCE < ACTIVE_FLOOR);
        // Pero un refuerzo basta para activarlo.
        assert!(BASE_CONFIDENCE + REINFORCE_STEP >= ACTIVE_FLOOR);
    }

    #[test]
    fn profile_note_empty_without_facts() {
        // Sin hechos vigentes, no se inyecta nada al prompt (no satura).
        let facts: Vec<Fact> = vec![];
        let live = facts
            .iter()
            .filter(|f| !f.retired && f.confidence >= ACTIVE_FLOOR)
            .count();
        assert_eq!(live, 0);
    }
}
