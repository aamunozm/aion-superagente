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
/// Confianza inicial de una regla nueva. **CUARENTENA**: está por DEBAJO de `ACTIVE_FLOOR`
/// a propósito — una hipótesis destilada de una sola corrida NO debe guiar el comportamiento
/// hasta que la experiencia la reconfirme al menos una vez (un refuerzo la lleva a 0.37 ≥ 0.30).
const BASE_CONFIDENCE: f32 = 0.25;
/// Refuerzo al re-confirmar un patrón ya conocido (asimétrico con el decaimiento).
const REINFORCE_STEP: f32 = 0.12;
/// Por debajo de esta confianza una regla no entra al prompt ni se considera vigente.
const ACTIVE_FLOOR: f32 = 0.30;
/// Por debajo de esto, y si ya es vieja, se retira (poda darwiniana).
const RETIRE_FLOOR: f32 = 0.18;
/// ≥ esto = la regla candidata es un DUPLICADO casi exacto (dedup): se refuerza, no se añade.
const DEDUP_SIM: f32 = 0.90;
/// Entre vecindad y dedup: una REFORMULACIÓN del mismo patrón → refuerza la vigente (amplía
/// el refuerzo más allá del duplicado literal y frena la acumulación de cuasi-sinónimos).
const REINFORCE_SIM: f32 = 0.85;
/// Banda en la que dos reglas son "vecinas": posible refinamiento, refuerzo o contradicción.
const NEIGHBOR_SIM: f32 = 0.78;
/// Cuántas vecinas más próximas se comprueban por contradicción (presupuesto de LLM por ciclo).
const MAX_NEIGHBOR_CHECKS: usize = 3;
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

/// Resultado de pasar una regla candidata por las guardas de gobernanza. Los índices
/// apuntan al Vec de reglas que `reflect_once` mantiene en memoria (un único RMW por ciclo).
enum Verdict {
    /// Es nueva y distinta: persistir como regla en cuarentena (confianza base).
    Insert,
    /// Reconfirma una vigente (idx): reforzarla en vez de duplicar (MDL + refuerzo ampliado).
    Reinforce(usize),
    /// Contradice una vigente consolidada: descartar (anclaje a lo estable).
    Reject,
    /// Contradice una vigente MÁS DÉBIL (idx a retirar): la nueva la reemplaza.
    Supersede(usize),
}

/// ¿Las dos heurísticas se CONTRADICEN? Una sola pregunta de vocabulario cerrado (SI/NO).
/// Ante fallo del modelo devuelve `false`: el peor caso es un duplicado, no una pérdida.
async fn contradicts(engine: &OllamaEngine, a: &str, b: &str) -> bool {
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Decides si dos heurísticas se CONTRADICEN (una dice lo contrario de la \
                 otra). Respondes SOLO con SI o NO. Nada más.",
            ),
            Message::user(format!(
                "Heurística A (vigente): «{a}»\nHeurística B (nueva): «{b}»\n¿Se contradicen? SOLO SI o NO."
            )),
        ],
        think: false,
        temperature: Some(0.0),
        max_tokens: Some(4),
    };
    match engine.generate(req).await {
        Ok(m) => {
            let ans = m.content.trim().to_lowercase();
            ans.starts_with("si") || ans.starts_with("sí")
        }
        Err(_) => false,
    }
}

/// **Guardas SSGM-lite**, sobre las reglas YA cargadas en memoria (sin tocar disco).
/// Mejoras frente a la primera versión: (1) refuerzo AMPLIADO — toda reconfirmación
/// (sim ≥ REINFORCE_SIM), no solo el duplicado literal ≥ 0.90; (2) contradicción comprobada
/// contra VARIAS vecinas, no solo la más próxima; (3) dim-check — si cambió el modelo de
/// embeddings, las reglas viejas no son comparables y se ignoran (en vez de dar coseno falso).
async fn govern(
    items: &[Rule],
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
        .filter(|(_, r)| !r.retired && r.embedding.len() == cand_emb.len())
        .map(|(i, r)| (i, aion_memory::cosine(cand_emb, &r.embedding)))
        .filter(|(_, s)| *s >= NEIGHBOR_SIM)
        .collect();
    neigh.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let Some(&(best_idx, best_sim)) = neigh.first() else {
        return Verdict::Insert; // sin vecinas: heurística genuinamente nueva
    };
    // Duplicado casi exacto → reforzar.
    if best_sim >= DEDUP_SIM {
        return Verdict::Reinforce(best_idx);
    }
    // ¿Contradice a alguna de las vecinas más próximas? (no solo la primera).
    for &(idx, _) in neigh.iter().take(MAX_NEIGHBOR_CHECKS) {
        if contradicts(engine, &items[idx].text, candidate).await {
            // Anclaje: la vigente consolidada gana; si es débil (cuarentena/decaída), la nueva la reemplaza.
            return if items[idx].confidence >= BASE_CONFIDENCE {
                Verdict::Reject
            } else {
                Verdict::Supersede(idx)
            };
        }
    }
    // Sin contradicción: si es muy parecida, es una reformulación → refuerza.
    if best_sim >= REINFORCE_SIM {
        Verdict::Reinforce(best_idx)
    } else {
        Verdict::Insert // relacionada pero distinta: entra como regla nueva en cuarentena
    }
}

/// Aplica refuerzo a la regla `idx` (in-memory): sube confianza, cuenta el uso, renueva la
/// confirmación y resetea su reloj de decaimiento. Es la confirmación cross-trajectory.
fn apply_reinforce(items: &mut [Rule], idx: usize, now: i64) {
    let r = &mut items[idx];
    r.confidence = (r.confidence + REINFORCE_STEP).min(1.0);
    r.uses += 1;
    r.last_confirmed = now;
    r.last_decay = now;
    r.retired = false;
}

/// Inserta una regla nueva (in-memory) en CUARENTENA (BASE_CONFIDENCE < ACTIVE_FLOOR: no
/// guía el comportamiento hasta reconfirmarse). Hace CONVERGER el tope `MAX_RULES` retirando
/// las vivas más débiles (un `while`, no un solo retiro, por si hubo resurrecciones).
fn apply_insert(items: &mut Vec<Rule>, text: &str, embedding: Vec<f32>, now: i64) {
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
    while items.iter().filter(|r| !r.retired).count() > MAX_RULES {
        let Some(weak) = items.iter_mut().filter(|r| !r.retired).min_by(|a, b| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) else {
            break;
        };
        weak.retired = true;
    }
}

/// Cuántas reglas RETIRADAS conservar como historia antes de compactar el archivo.
const MAX_RETIRED: usize = 40;

/// **Decaimiento temporal + poda darwiniana** (SSGM) sobre el Vec en memoria. Devuelve
/// `(nº retiradas en esta pasada, hubo_cambio)`. Decaimiento INCREMENTAL (desde `last_decay`)
/// para no re-aplicar el mismo decaimiento en cada ciclo. Determinista, sin LLM.
fn decay_prune_inplace(items: &mut Vec<Rule>, now: i64) -> (usize, bool) {
    if items.is_empty() {
        return (0, false);
    }
    let mut retired = 0usize;
    let mut changed = false;
    for r in items.iter_mut() {
        if r.retired {
            continue;
        }
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
    // Compactación: conserva las MAX_RETIRED retiradas MÁS RECIENTES y suelta las más viejas
    // por TIEMPO (last_confirmed/at), no por posición en el archivo.
    let retired_count = items.iter().filter(|r| r.retired).count();
    if retired_count > MAX_RETIRED {
        let drop_n = retired_count - MAX_RETIRED;
        let mut retired_idx: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, r)| r.retired)
            .map(|(i, _)| i)
            .collect();
        retired_idx.sort_by_key(|&i| {
            let r = &items[i];
            if r.last_confirmed > 0 {
                r.last_confirmed
            } else {
                r.at
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

/// Escribe el almacén bajo QLOCK (único punto de escritura por ciclo).
fn save_locked(items: &[Rule]) {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    save(items);
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
    let now = chrono::Utc::now().timestamp();
    // UN SOLO RMW por ciclo: cargar una vez, mutar en memoria (decaimiento + veredicto) y
    // guardar una sola vez al final. Evita las ~5 lecturas + 3 escrituras del JSONL que hacía
    // la versión anterior, encoge la ventana de carrera y hace el Supersede ATÓMICO.
    let mut items = all();
    let (pruned, changed) = decay_prune_inplace(&mut items, now);

    let ctx = gather_trajectories().await;
    if ctx.trim().chars().count() < 40 {
        // Aún no hay vida suficiente que generalizar: abstenerse (pero persistir el decaimiento).
        if changed {
            save_locked(&items);
        }
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
        if changed {
            save_locked(&items);
        }
        return (pruned > 0 || changed, "el modelo local no respondió".into());
    };
    let rule = m
        .content
        .trim()
        .trim_matches(['«', '»', '"', '.', ' '])
        .trim()
        .to_string();
    // Detección robusta de la abstención: "NINGUNA" aunque venga con preámbulo corto.
    let low = rule.to_lowercase();
    let is_none = low == "ninguna"
        || low.starts_with("ninguna")
        || low.ends_with("ninguna")
        || (low.contains("ninguna") && rule.chars().count() < 30);
    if rule.is_empty() || is_none || rule.chars().count() < 15 {
        if changed {
            save_locked(&items);
        }
        return (
            pruned > 0,
            if pruned > 0 {
                format!("podé {pruned} heurísticas viejas")
            } else {
                "no emergió ningún patrón nuevo".into()
            },
        );
    }

    // 2) Guardas de gobernanza (in-memory, sobre `items`).
    let cand_emb = embed(&rule).await;
    let short: String = rule.chars().take(80).collect();
    let detail = match govern(&items, &cand_emb, &rule, engine).await {
        Verdict::Reinforce(idx) => {
            apply_reinforce(&mut items, idx, now);
            format!("reforcé una heurística que la experiencia confirma: «{short}»")
        }
        Verdict::Reject => {
            if changed {
                save_locked(&items);
            }
            return (
                pruned > 0 || changed,
                format!(
                    "descarté una idea que contradecía algo que ya sé: «{}»",
                    rule.chars().take(60).collect::<String>()
                ),
            );
        }
        Verdict::Supersede(idx) => {
            items[idx].retired = true;
            apply_insert(&mut items, &rule, cand_emb, now);
            format!("revisé una heurística vieja por una mejor: «{short}»")
        }
        Verdict::Insert => {
            apply_insert(&mut items, &rule, cand_emb, now);
            format!("aprendí una heurística nueva (en cuarentena hasta reconfirmarse): «{short}»")
        }
    };
    // Un único guardado atómico de todo el ciclo (siempre hubo mutación: refuerzo/inserción).
    save_locked(&items);

    // Re-entrada GWT: lo que aprendo se publica en el tablón (y vuelve a mi prompt).
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
    fn newborn_rule_starts_in_quarantine() {
        // Invariante clave: una regla recién nacida NO entra al prompt (cuarentena).
        assert!(
            BASE_CONFIDENCE < ACTIVE_FLOOR,
            "una regla nueva no debe guiar el comportamiento hasta reconfirmarse"
        );
        // Pero un único refuerzo debe bastar para activarla (si no, nunca actuaría).
        assert!(
            BASE_CONFIDENCE + REINFORCE_STEP >= ACTIVE_FLOOR,
            "un refuerzo debe sacar la regla de la cuarentena"
        );
        // El refuerzo se amplía a reformulaciones, no solo a duplicados literales.
        assert!(REINFORCE_SIM < DEDUP_SIM && REINFORCE_SIM >= NEIGHBOR_SIM);
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
