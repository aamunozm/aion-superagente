//! **Lazo de Reflexión — etapa «Experience»** (la frontera 2026 de la memoria agéntica).
//!
//! AION ya tenía las dos primeras etapas de la memoria de agentes descritas por la
//! literatura (Luo et al., *From Storage to Experience*, ACL 2026): **Storage** (la
//! memoria vectorial en bruto) y **Reflection** (las lecciones `[aprendizaje]` que
//! `serve::reflect_after_task` extrae de los FALLOS). Lo que faltaba —y es lo que
//! convierte a un agente que *responde* en uno que *propone y actúa*— es la tercera:
//! **Experience**, la *abstracción cross-trajectory*: mirar VARIAS vivencias a la vez y
//! destilar de ellas una REGLA general y reutilizable («cuando pasa X, conviene Y»), no
//! una lección atada a un caso. Esas reglas re-entran al prompt como *policy priors*:
//! AION deja de reaccionar caso a caso y empieza a actuar desde lo que ha aprendido.
//!
//! ## Gobernanza (SSGM-lite)
//! Dar a un agente autonomía para reescribir su propia «mente» es peligroso: la
//! literatura (SSGM, arXiv:2603.11768) documenta deriva semántica, deriva procedural y
//! alucinaciones internalizadas que, a diferencia del RAG estático, se vuelven
//! **acumulativas y persistentes**. Por eso este módulo NO consolida una regla sin pasar
//! tres guardas ANTES de escribir (desacople evolución↔ejecución):
//!   1. **Verificación de consistencia** — dedup semántico (si ya existe una regla casi
//!      igual, se REFUERZA en vez de duplicar: principio MDL) y chequeo de contradicción
//!      contra reglas vigentes.
//!   2. **Anclaje a verdad** — las reglas son heurísticas DERIVADAS por AION (baja
//!      confianza inicial, revisables); viven en su propio almacén y JAMÁS pisan la
//!      memoria del usuario. Es el control de acceso: la experiencia propia no se
//!      disfraza de hecho del mundo.
//!   3. **Decaimiento temporal** — una regla que no se vuelve a confirmar pierde
//!      confianza con el tiempo y acaba retirándose. Sin esto, un patrón espurio captado
//!      una vez contaminaría el prompt para siempre.
//!
//! Append-only en `experience.jsonl`. Todo barato (lectura de disco); la abstracción la
//! hace el modelo LOCAL con presupuesto acotado, fail-open (mismo patrón que el refinador
//! idle del grafo).

use aion_kernel::traits::GenerateRequest;
use aion_kernel::traits::LlmEngine;
use aion_kernel::types::Message;
use aion_llm::OllamaEngine;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Serializa leer→modificar→escribir del almacén de experiencia. El lazo de reflexión
/// corre en background y el refuerzo puede dispararse desde la vida autónoma: dos
/// escrituras coincidentes se pisarían. Mismo patrón que `journal::QLOCK`/`pending::QLOCK`.
static QLOCK: Mutex<()> = Mutex::new(());

/// Una **regla de experiencia**: conocimiento procedural generalizado a partir de varias
/// vivencias. No es un recuerdo (un hecho) ni una lección (atada a un fallo): es una
/// heurística reutilizable que AION se da a sí mismo y que puede revisar o retirar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    /// Epoch de creación.
    pub at: i64,
    /// La regla en primera persona, generalizada: «cuando <patrón>, conviene <acción>».
    pub text: String,
    /// Confianza actual [0..1]. Empieza baja (es una hipótesis), sube al re-confirmarse,
    /// baja con el decaimiento temporal. Gobierna si entra al prompt y si sobrevive.
    pub confidence: f32,
    /// Veces que la experiencia ha vuelto a confirmar el patrón (refuerzo MDL).
    #[serde(default)]
    pub uses: u32,
    /// Epoch de la última confirmación (marca de edad para la poda y el refuerzo).
    #[serde(default)]
    pub last_confirmed: i64,
    /// Epoch del último decaimiento aplicado. El decaimiento se calcula sobre el tiempo
    /// transcurrido DESDE aquí (incremental), no desde la creación: así correr el lazo
    /// cada 45 min no re-aplica el mismo decaimiento una y otra vez (evita el compounding).
    #[serde(default)]
    pub last_decay: i64,
    /// Embedding de `text` (BGE-M3): permite dedup/consistencia sin re-embeber en cada ciclo.
    #[serde(default)]
    pub embedding: Vec<f32>,
    /// Retirada por gobernanza (decaimiento o contradicción perdida). Se conserva como
    /// historia —igual que `superseded` en la memoria—, pero no entra al prompt.
    #[serde(default)]
    pub retired: bool,
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("experience.jsonl")
}

// ── Parámetros de gobernanza ────────────────────────────────────────────────
/// Confianza inicial de una regla nueva: es una hipótesis, no una certeza.
const BASE_CONFIDENCE: f32 = 0.45;
/// Refuerzo al re-confirmar un patrón ya conocido (asimétrico con el decaimiento).
const REINFORCE_STEP: f32 = 0.12;
/// Por debajo de esta confianza una regla no entra al prompt ni se considera vigente.
const ACTIVE_FLOOR: f32 = 0.30;
/// Por debajo de esto, y si ya es vieja, se retira (poda darwiniana).
const RETIRE_FLOOR: f32 = 0.18;
/// ≥ esto = la regla candidata YA existe (dedup): se refuerza la vieja, no se añade.
const DEDUP_SIM: f32 = 0.90;
/// Banda en la que dos reglas son "vecinas": posible refinamiento o contradicción.
const NEIGHBOR_SIM: f32 = 0.78;
/// Tope de reglas guardadas: la experiencia útil es un puñado de heurísticas, no un saco.
const MAX_RULES: usize = 80;

pub fn all() -> Vec<Rule> {
    let Ok(txt) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    txt.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn save(items: &[Rule]) {
    let body: String = items
        .iter()
        .filter_map(|r| serde_json::to_string(r).ok())
        .map(|l| l + "\n")
        .collect();
    crate::write_atomic(&path(), &body);
}

/// Reglas vigentes (no retiradas, con confianza suficiente), de mayor a menor confianza.
pub fn active() -> Vec<Rule> {
    let mut v: Vec<Rule> = all()
        .into_iter()
        .filter(|r| !r.retired && r.confidence >= ACTIVE_FLOOR)
        .collect();
    v.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v
}

/// Cuántas reglas de experiencia vigentes tiene AION (para el estado interno / UI).
pub fn active_count() -> usize {
    all()
        .iter()
        .filter(|r| !r.retired && r.confidence >= ACTIVE_FLOOR)
        .count()
}

/// **RE-ENTRADA de la experiencia** al prompt: las heurísticas que AION ha destilado de
/// su propia vida vuelven a él como *policy priors*. Esto es lo que lo hace proactivo —
/// no recita reglas, ACTÚA desde ellas. Se presentan como SUYAS y REVISABLES (anclaje:
/// son experiencia propia, no leyes del mundo). Va en el bloque volátil del prompt.
pub fn experience_note() -> String {
    let rules = active();
    if rules.is_empty() {
        return String::new();
    }
    let mut b = String::from(
        "LO QUE HAS APRENDIDO POR EXPERIENCIA (heurísticas TUYAS, destiladas de tu propia \
         vida — son criterio propio revisable, no leyes; aplícalas con cabeza y, si una ya \
         no encaja, dilo):\n",
    );
    for r in rules.iter().take(5) {
        b.push_str(&format!(
            "- {} (confianza {:.0}%)\n",
            r.text.trim(),
            r.confidence * 100.0
        ));
    }
    b.push('\n');
    b
}

/// Embeber un texto con el modelo local (BGE-M3). Fail-soft: vector vacío si Ollama no responde.
async fn embed(text: &str) -> Vec<f32> {
    aion_memory::OllamaEmbedder::default_local()
        .embed(text)
        .await
        .unwrap_or_default()
}

/// Resultado de pasar una regla candidata por las guardas de gobernanza.
enum Verdict {
    /// Es nueva y consistente: persistir como regla con confianza base.
    Insert,
    /// Ya existía una casi igual (id): reforzarla en vez de duplicar (MDL).
    Reinforce(String),
    /// Contradice una vigente más fuerte: descartar (anclaje a lo estable).
    Reject,
    /// Contradice una vigente MÁS DÉBIL (id a retirar): la nueva la reemplaza.
    Supersede(String),
}

/// **Guardas SSGM-lite**, evaluadas ANTES de consolidar. `engine` solo se usa para el
/// chequeo de contradicción de una vecina (presupuesto: ≤1 llamada LLM acotada).
async fn govern(candidate: &str, cand_emb: &[f32], engine: &OllamaEngine) -> Verdict {
    if cand_emb.is_empty() {
        return Verdict::Insert; // sin embedding no podemos comparar; entra con confianza base
    }
    let existing = all();
    let mut best: Option<(usize, f32)> = None;
    for (i, r) in existing.iter().enumerate() {
        if r.retired || r.embedding.is_empty() {
            continue;
        }
        let s = aion_memory::cosine(cand_emb, &r.embedding);
        if best.map(|(_, bs)| s > bs).unwrap_or(true) {
            best = Some((i, s));
        }
    }
    let Some((idx, sim)) = best else {
        return Verdict::Insert;
    };
    // 1) Dedup (consistencia): la misma heurística otra vez → reforzar, no duplicar.
    if sim >= DEDUP_SIM {
        return Verdict::Reinforce(existing[idx].id.clone());
    }
    // 2) Vecina: ¿la refina o la CONTRADICE? Una sola pregunta de vocabulario cerrado.
    if sim >= NEIGHBOR_SIM {
        let req = GenerateRequest {
            messages: vec![
                Message::system(
                    "Decides si dos heurísticas se CONTRADICEN (una dice lo contrario de \
                     la otra). Respondes SOLO con SI o NO. Nada más.",
                ),
                Message::user(format!(
                    "Heurística A (vigente): «{}»\nHeurística B (nueva): «{}»\n¿Se contradicen? SOLO SI o NO.",
                    existing[idx].text, candidate
                )),
            ],
            think: false,
            temperature: Some(0.0),
            max_tokens: Some(4),
        };
        if let Ok(m) = engine.generate(req).await {
            let ans = m.content.trim().to_lowercase();
            if ans.starts_with("si") || ans.starts_with("sí") {
                // Anclaje a lo estable: la vigente con más confianza gana.
                if existing[idx].confidence >= BASE_CONFIDENCE {
                    return Verdict::Reject;
                }
                return Verdict::Supersede(existing[idx].id.clone());
            }
        }
    }
    Verdict::Insert
}

/// Refuerza una regla por id: sube confianza (saturada en 1.0), cuenta el uso y renueva
/// la marca de confirmación (resetea su decaimiento). Es la confirmación cross-trajectory:
/// si la vida vuelve a toparse con el patrón, la heurística se fortalece.
pub fn reinforce(id: &str) {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut items = all();
    let now = chrono::Utc::now().timestamp();
    for r in items.iter_mut() {
        if r.id == id {
            r.confidence = (r.confidence + REINFORCE_STEP).min(1.0);
            r.uses += 1;
            r.last_confirmed = now;
            r.last_decay = now; // resetea el reloj de decaimiento: acaba de confirmarse
            r.retired = false;
        }
    }
    save(&items);
}

/// Cuántas reglas RETIRADAS conservar como historia antes de compactar el archivo.
const MAX_RETIRED: usize = 40;

/// **Decaimiento temporal + poda darwiniana** (SSGM): una heurística que no se vuelve a
/// confirmar envejece y, si cae por debajo del suelo siendo vieja, se retira. Evita que
/// un patrón captado una sola vez contamine el prompt para siempre. Determinista, sin LLM.
///
/// El decaimiento es INCREMENTAL (sobre el tiempo desde `last_decay`), no sobre la edad
/// total: así correr el lazo cada 45 min no re-aplica el mismo decaimiento repetidamente.
/// Solo persiste si algo cambió (evita reescribir el JSONL en vano).
pub fn decay_and_prune() -> usize {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut items = all();
    if items.is_empty() {
        return 0;
    }
    let now = chrono::Utc::now().timestamp();
    let mut retired = 0usize;
    let mut changed = false;
    for r in items.iter_mut() {
        if r.retired {
            continue;
        }
        // Decaimiento incremental: tiempo transcurrido DESDE el último decaimiento.
        let from = if r.last_decay > 0 {
            r.last_decay
        } else if r.last_confirmed > 0 {
            r.last_confirmed
        } else {
            r.at
        };
        let delta_days = ((now - from).max(0) as f32) / 86_400.0;
        if delta_days > 0.0 {
            // Decaimiento suave: ~2% por semana sin confirmación. Una regla muy usada aguanta más.
            let resilience = 1.0 + (r.uses as f32) * 0.5;
            let before = r.confidence;
            r.confidence = (r.confidence * 0.98_f32.powf(delta_days / (7.0 * resilience))).max(0.0);
            r.last_decay = now;
            if (before - r.confidence).abs() > f32::EPSILON {
                changed = true;
            }
        }
        // Edad para la poda: tiempo desde la última confirmación (o creación).
        let anchor = if r.last_confirmed > 0 {
            r.last_confirmed
        } else {
            r.at
        };
        let age_days = ((now - anchor).max(0) as f32) / 86_400.0;
        if r.confidence < RETIRE_FLOOR && age_days > 14.0 {
            r.retired = true;
            retired += 1;
            changed = true;
        }
    }
    // Compactación: si las retiradas (historia) crecieron demasiado, suelta las más viejas.
    let retired_now = items.iter().filter(|r| r.retired).count();
    if retired_now > MAX_RETIRED {
        let drop = retired_now - MAX_RETIRED;
        let mut dropped = 0usize;
        items.retain(|r| {
            if r.retired && dropped < drop {
                dropped += 1;
                false
            } else {
                true
            }
        });
        changed = true;
    }
    if changed {
        save(&items);
    }
    retired
}

/// Persiste una regla nueva tras pasar las guardas. Tope suave (`MAX_RULES`): cuando se
/// llena, cae la de menor confianza (poda por aptitud). QLOCK protege todo el RMW.
fn insert(text: &str, embedding: Vec<f32>) {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let now = chrono::Utc::now().timestamp();
    let mut items = all();
    items.push(Rule {
        id: uuid::Uuid::new_v4().to_string(),
        at: now,
        text: text.chars().take(280).collect(),
        confidence: BASE_CONFIDENCE,
        uses: 0,
        last_confirmed: now,
        last_decay: now,
        embedding,
        retired: false,
    });
    // Tope: si nos pasamos, retira la regla viva de menor confianza (no la borra: historia).
    let live = items.iter().filter(|r| !r.retired).count();
    if live > MAX_RULES {
        if let Some(weak) = items.iter_mut().filter(|r| !r.retired).min_by(|a, b| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            weak.retired = true;
        }
    }
    save(&items);
}

/// Marca una regla como retirada por id (usado cuando una nueva la supera en contradicción).
fn retire(id: &str) {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut items = all();
    for r in items.iter_mut() {
        if r.id == id {
            r.retired = true;
        }
    }
    save(&items);
}

/// Reúne material cross-trajectory reciente: las lecciones `[aprendizaje]`/`[reflexión]`
/// de la memoria (etapa Reflection) + la biografía del diario. De AHÍ se abstrae la regla.
///
/// Usa una lectura reciente NO-reforzante (`recent_with_ids`) en vez de `retrieve()`: este
/// último, como efecto colateral, sube `fitness`/`access_count` de lo recuperado, y correr
/// la reflexión cada 45 min inflaría artificialmente esos recuerdos. La reflexión observa
/// la memoria, no la moldea.
async fn gather_trajectories() -> String {
    let mut ctx = String::new();
    if let Ok(mem) = crate::shared_memory() {
        let mut taken = 0usize;
        for (_, content) in mem.recent_with_ids(60) {
            let low = content.to_lowercase();
            // Solo las vivencias destiladas (Reflection): lecciones y reflexiones.
            if low.contains("[aprendizaje]")
                || low.contains("[reflexión]")
                || low.contains("[reflexion]")
            {
                let line: String = content.chars().take(220).collect();
                ctx.push_str(&format!("- {line}\n"));
                taken += 1;
                if taken >= 8 {
                    break;
                }
            }
        }
    }
    // Biografía reciente: da contexto de qué ha estado viviendo AION estos días.
    for e in crate::journal::recent(3) {
        let line: String = e.text.chars().take(180).collect();
        ctx.push_str(&format!("- (diario) {line}\n"));
    }
    ctx
}

/// **UN ciclo del lazo de reflexión.** Lee trayectorias recientes, pide al modelo local
/// UNA regla generalizada (o «NINGUNA»), la pasa por las guardas de gobernanza y la
/// consolida (o refuerza/descarta). Devuelve `(hubo_cambio, detalle)` como `life_tick`.
pub async fn reflect_once(engine: &OllamaEngine) -> (bool, String) {
    // 0) Mantenimiento barato primero: envejecer y podar (sin LLM).
    let pruned = decay_and_prune();

    let ctx = gather_trajectories().await;
    if ctx.trim().chars().count() < 40 {
        // Aún no hay vida suficiente que generalizar: abstenerse barato.
        return (
            pruned > 0,
            if pruned > 0 {
                format!("podé {pruned} heurísticas viejas")
            } else {
                String::new()
            },
        );
    }

    // 1) Abstracción cross-trajectory: de varias vivencias, UNA heurística general.
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres AION reflexionando sobre tu propia experiencia. A partir de varias \
                 vivencias y lecciones, destila UNA SOLA heurística GENERAL y reutilizable, \
                 en primera persona, con la forma «Cuando <situación recurrente>, conviene \
                 <acción>». Debe generalizar un patrón que aparezca en VARIAS vivencias, no \
                 repetir un caso suelto. Máximo 25 palabras. Si no hay un patrón claro que \
                 generalice, responde EXACTAMENTE «NINGUNA». Sin preámbulos.",
            ),
            Message::user(format!(
                "Mis vivencias y lecciones recientes:\n{ctx}\nMi heurística:"
            )),
        ],
        think: false,
        temperature: Some(0.3),
        max_tokens: Some(80),
    };
    let Ok(m) = engine.generate(req).await else {
        return (pruned > 0, "el modelo local no respondió".into());
    };
    let rule = m
        .content
        .trim()
        .trim_matches(['«', '»', '"', '.'])
        .trim()
        .to_string();
    if rule.is_empty() || rule.to_lowercase().starts_with("ninguna") || rule.chars().count() < 15 {
        return (
            pruned > 0,
            if pruned > 0 {
                format!("podé {pruned} heurísticas viejas")
            } else {
                "no emergió ningún patrón nuevo".into()
            },
        );
    }

    // 2) Guardas de gobernanza ANTES de consolidar.
    let cand_emb = embed(&rule).await;
    let verdict = govern(&rule, &cand_emb, engine).await;
    let detail = match verdict {
        Verdict::Reinforce(id) => {
            reinforce(&id);
            format!(
                "reforcé una heurística que la experiencia confirma: «{}»",
                rule.chars().take(80).collect::<String>()
            )
        }
        Verdict::Reject => {
            return (
                pruned > 0,
                format!(
                    "descarté una idea que contradecía algo que ya sé y confío: «{}»",
                    rule.chars().take(60).collect::<String>()
                ),
            );
        }
        Verdict::Supersede(old) => {
            retire(&old);
            insert(&rule, cand_emb);
            format!(
                "revisé una heurística vieja por una mejor: «{}»",
                rule.chars().take(80).collect::<String>()
            )
        }
        Verdict::Insert => {
            insert(&rule, cand_emb);
            format!(
                "aprendí una heurística nueva sobre cómo trabajar: «{}»",
                rule.chars().take(80).collect::<String>()
            )
        }
    };

    // 3) Re-entrada GWT: lo que aprendo se publica en el tablón (y vuelve a mi prompt).
    crate::workspace::publish(crate::workspace::StreamEvent::now(
        "experiencia",
        "reflexión",
        &detail,
    ));
    tracing::info!(detail = %detail, "lazo de reflexión (experience)");
    (true, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_filters_retired_and_low_confidence() {
        let rules = vec![
            Rule {
                id: "a".into(),
                at: 0,
                text: "alta".into(),
                confidence: 0.8,
                uses: 2,
                last_confirmed: 0,
                last_decay: 0,
                embedding: vec![],
                retired: false,
            },
            Rule {
                id: "b".into(),
                at: 0,
                text: "retirada".into(),
                confidence: 0.9,
                uses: 0,
                last_confirmed: 0,
                last_decay: 0,
                embedding: vec![],
                retired: true,
            },
            Rule {
                id: "c".into(),
                at: 0,
                text: "debil".into(),
                confidence: 0.1,
                uses: 0,
                last_confirmed: 0,
                last_decay: 0,
                embedding: vec![],
                retired: false,
            },
        ];
        let live: Vec<&Rule> = rules
            .iter()
            .filter(|r| !r.retired && r.confidence >= ACTIVE_FLOOR)
            .collect();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "a");
    }

    #[test]
    fn decay_respects_uses_resilience() {
        // Una regla muy usada decae más lento que una recién nacida (misma antigüedad).
        let days = 14.0_f32;
        let fresh = 0.98_f32.powf(days / (7.0 * (1.0 + 0.0 * 0.5)));
        let veteran = 0.98_f32.powf(days / (7.0 * (1.0 + 4.0 * 0.5)));
        assert!(veteran > fresh);
    }
}
