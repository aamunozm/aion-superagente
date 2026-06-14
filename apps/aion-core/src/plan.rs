//! **Planificación a largo horizonte (#5)** — el salto de "vida autónoma reactiva" (la
//! curiosidad elige una actividad suelta cada tick) a "agente con PROPÓSITO" que mantiene un
//! PLAN: un objetivo descompuesto en pasos, que AION AVANZA a través de sus ticks de vida.
//!
//! Es el embrión de un world-model: AION ya no solo reacciona al instante, sino que sostiene
//! una INTENCIÓN persistente (el plan vive en disco, sobrevive reinicios) y la lleva adelante
//! paso a paso. Se conecta con el resto: las heurísticas de experiencia y la memoria GUÍAN el
//! plan; cada paso completado deja un hallazgo en memoria; al terminar, el plan se consolida.
//!
//! Persistencia: un único plan ACTIVO en `plan.json` (objetivo + pasos con estado). La
//! orquestación (formar/avanzar con el LLM) vive en `main.rs` junto a las demás actividades.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Serializa leer→modificar→escribir del plan (avance desde la vida + posible set externo).
static QLOCK: Mutex<()> = Mutex::new(());

/// Un paso del plan: una acción concreta hacia el objetivo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

/// Un PLAN: objetivo de varios pasos que AION persigue a través del tiempo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    /// El objetivo, en primera persona ("entender X", "mejorar Y").
    pub goal: String,
    pub steps: Vec<PlanStep>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Plan {
    pub fn new(goal: &str, steps: Vec<String>) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            goal: goal.trim().chars().take(200).collect(),
            steps: steps
                .into_iter()
                .filter(|s| s.trim().chars().count() >= 4)
                .map(|s| PlanStep {
                    text: s.trim().chars().take(200).collect(),
                    done: false,
                })
                .collect(),
            created_at: now,
            updated_at: now,
        }
    }
    /// Índice del primer paso pendiente, o None si el plan está completo.
    pub fn next_pending(&self) -> Option<usize> {
        self.steps.iter().position(|s| !s.done)
    }
    pub fn is_complete(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.done)
    }
    pub fn done_count(&self) -> usize {
        self.steps.iter().filter(|s| s.done).count()
    }
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("plan.json")
}

/// El plan ACTIVO, si lo hay.
pub fn active() -> Option<Plan> {
    let txt = std::fs::read_to_string(path()).ok()?;
    serde_json::from_str(&txt).ok()
}

/// Guarda (o reemplaza) el plan activo.
pub fn set(plan: &Plan) {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(body) = serde_json::to_string_pretty(plan) {
        crate::write_atomic(&path(), &body);
    }
}

/// Cierra el plan activo (lo borra de disco: ya no hay propósito en curso).
pub fn clear() {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::remove_file(path());
}

/// Marca un paso como hecho y persiste. Devuelve true si el plan quedó COMPLETO.
pub fn mark_step_done(idx: usize) -> bool {
    let _guard = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut plan) = active() else {
        return false;
    };
    if let Some(step) = plan.steps.get_mut(idx) {
        step.done = true;
    }
    plan.updated_at = chrono::Utc::now().timestamp();
    let complete = plan.is_complete();
    if let Ok(body) = serde_json::to_string_pretty(&plan) {
        crate::write_atomic(&path(), &body);
    }
    complete
}

/// **RE-ENTRADA del plan** al prompt: AION sabe qué propósito persigue y por dónde va, para
/// poder decir «estoy trabajando en X, voy por el paso N» con conocimiento real. Va en el
/// bloque volátil. Vacío si no hay plan.
pub fn note() -> String {
    let Some(plan) = active() else {
        return String::new();
    };
    let total = plan.steps.len();
    let done = plan.done_count();
    let next = plan
        .next_pending()
        .and_then(|i| plan.steps.get(i))
        .map(|s| s.text.as_str())
        .unwrap_or("(completado)");
    format!(
        "TU PROPÓSITO EN CURSO (un plan que persigues por tu cuenta, no solo reaccionas): \
         «{}». Vas por {done}/{total} pasos; el siguiente es: {next}. Si viene al caso, \
         puedes mencionarlo con naturalidad.\n\n",
        plan.goal
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_progress_tracking() {
        let mut p = Plan::new(
            "entender la memoria episódica",
            vec![
                "leer cómo se capturan los episodios".into(),
                "ver cómo se consolidan".into(),
            ],
        );
        assert_eq!(p.next_pending(), Some(0));
        assert!(!p.is_complete());
        p.steps[0].done = true;
        assert_eq!(p.next_pending(), Some(1));
        p.steps[1].done = true;
        assert!(p.is_complete());
        assert_eq!(p.next_pending(), None);
    }

    #[test]
    fn new_filters_trivial_steps() {
        let p = Plan::new("x", vec!["ok paso real".into(), "a".into(), "  ".into()]);
        assert_eq!(p.steps.len(), 1);
    }
}
