//! Motivación intrínseca por *learning progress* (LP) — patrón MAGELLAN.
//!
//! Para cada objetivo se guarda una ventana de resultados recientes (éxito/fallo).
//! El LP estima cuánto está MEJORANDO el agente en ese objetivo (media de la mitad
//! reciente − media de la mitad antigua). El agente curioso elige el objetivo con
//! mayor LP: ni los ya dominados (LP≈0) ni los imposibles (LP≈0), sino los que
//! están en su "zona de aprendizaje".

use std::collections::{HashMap, VecDeque};

pub struct CuriosityEngine {
    window: usize,
    history: HashMap<String, VecDeque<bool>>,
}

impl CuriosityEngine {
    pub fn new(window: usize) -> Self {
        Self {
            window: window.max(2),
            history: HashMap::new(),
        }
    }

    /// Registra el resultado (éxito/fallo) de un intento sobre un objetivo.
    pub fn record(&mut self, goal: &str, success: bool) {
        let w = self.window;
        let dq = self.history.entry(goal.to_string()).or_default();
        dq.push_back(success);
        while dq.len() > w {
            dq.pop_front();
        }
    }

    /// Competencia actual = tasa de éxito en la ventana (0..1).
    pub fn competence(&self, goal: &str) -> f32 {
        match self.history.get(goal) {
            Some(dq) if !dq.is_empty() => {
                dq.iter().filter(|&&s| s).count() as f32 / dq.len() as f32
            }
            _ => 0.0,
        }
    }

    /// Learning progress: mejora reciente de competencia (mitad reciente − mitad antigua).
    /// Positivo = está aprendiendo; ~0 = dominado o estancado.
    pub fn learning_progress(&self, goal: &str) -> f32 {
        let Some(dq) = self.history.get(goal) else {
            return 0.0;
        };
        let n = dq.len();
        if n < 2 {
            return 0.0;
        }
        let half = n / 2;
        let mean = |it: &[bool]| {
            if it.is_empty() {
                0.0
            } else {
                it.iter().filter(|&&s| s).count() as f32 / it.len() as f32
            }
        };
        let v: Vec<bool> = dq.iter().copied().collect();
        mean(&v[half..]) - mean(&v[..half])
    }

    /// Elige el siguiente objetivo a perseguir (curiosidad). Los no explorados
    /// tienen prioridad (explorar); entre los explorados, el de mayor LP.
    pub fn next_goal<'a>(&self, candidates: &[&'a str]) -> Option<&'a str> {
        candidates
            .iter()
            .copied()
            .max_by(|a, b| score(self, a).total_cmp(&score(self, b)))
    }
}

fn score(eng: &CuriosityEngine, goal: &str) -> f32 {
    match eng.history.get(goal) {
        None => 1.0, // no explorado → máxima prioridad de exploración
        Some(_) => eng.learning_progress(goal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn improving_goal_has_positive_lp() {
        let mut c = CuriosityEngine::new(6);
        // Empieza fallando, luego acierta → progreso.
        for s in [false, false, false, true, true, true] {
            c.record("g", s);
        }
        assert!(
            c.learning_progress("g") > 0.3,
            "lp={}",
            c.learning_progress("g")
        );
        assert!((c.competence("g") - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mastered_goal_has_low_lp() {
        let mut c = CuriosityEngine::new(6);
        for _ in 0..6 {
            c.record("m", true); // siempre éxito → ya dominado
        }
        assert!(c.learning_progress("m").abs() < 1e-6);
        assert!((c.competence("m") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn curiosity_prefers_learnable_over_mastered() {
        let mut c = CuriosityEngine::new(6);
        for _ in 0..6 {
            c.record("mastered", true);
        }
        for s in [false, false, false, true, true, true] {
            c.record("learnable", s);
        }
        assert_eq!(c.next_goal(&["mastered", "learnable"]), Some("learnable"));
    }

    #[test]
    fn unexplored_goal_is_prioritized() {
        let mut c = CuriosityEngine::new(6);
        for _ in 0..6 {
            c.record("known", true);
        }
        assert_eq!(c.next_goal(&["known", "nuevo"]), Some("nuevo"));
    }
}
