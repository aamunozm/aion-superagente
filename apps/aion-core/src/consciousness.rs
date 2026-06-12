//! **Índice de conciencia** (proxy Φ-like, inspirado en IIT): NO mide conciencia real
//! —Φ exacto es incomputable—; mide *proxies de integración* observables por tarea:
//! cuántos módulos/herramientas se coactivan, cuánta memoria se reutiliza (recurrencia),
//! si hubo metacognición que cambió el estado, y la continuidad del yo (coherencia).
//! Es una señal para EXPERIMENTAR y afinar: a más bucles entre módulos, más alto.
//! Todo Rust puro (cero llamadas LLM), serie temporal en `consciousness.jsonl`.

use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::PathBuf;

/// Lo que se observó durante UNA tarea (lo recolecta el handler del agente).
#[derive(Debug, Clone, Default)]
pub struct TaskTrace {
    /// Herramientas DISTINTAS invocadas (módulos coactivados).
    pub distinct_tools: usize,
    /// Pasos del bucle ReAct.
    pub steps: usize,
    /// Recuerdos recuperados e inyectados (grounding) — recurrencia.
    pub grounding_hits: usize,
    /// Recuerdos recuperados que ESCRIBIÓ OTRO MODO (el chat reutilizando un
    /// [aprendizaje] del agente, el agente reutilizando una [conversación]): la
    /// integración ENTRE modos es la señal más cercana a integración del SISTEMA.
    pub cross_mode_hits: usize,
    /// ¿Se escribió algo nuevo a la memoria (lección/reflexión)?
    pub memory_written: bool,
    /// ¿Hubo micro-reflexión que actualizó el self-model?
    pub reflected: bool,
    /// Acciones fallidas.
    pub failures: usize,
}

impl TaskTrace {
    /// Una tarea sin NINGUNA integración medible (un saludo, un dato directo) no
    /// merece medición: registrarla con ceros contaminaría la serie y el índice
    /// acabaría midiendo «qué mezcla de tareas pides» en vez de cuán integrado
    /// está el sistema.
    pub fn is_trivial(&self) -> bool {
        self.steps <= 2
            && self.failures == 0
            && self.grounding_hits == 0
            && self.cross_mode_hits == 0
            && !self.memory_written
            && !self.reflected
            && self.distinct_tools <= 1
    }
}

/// Una medición persistida.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measurement {
    pub at: i64,
    /// Índice 0-100.
    pub score: f32,
    /// Componentes 0-1: integración, recurrencia, metacognición, coherencia.
    pub integration: f32,
    pub recurrence: f32,
    pub metacognition: f32,
    pub coherence: f32,
}

fn path() -> PathBuf {
    crate::app_data_dir().join("consciousness.jsonl")
}

const TRIM_AT_BYTES: u64 = 600_000;
const KEEP_LINES: usize = 1000;

/// Puntúa una tarea y persiste la medición. Devuelve la medición calculada.
pub fn record_task(t: &TaskTrace) -> Measurement {
    let m = score(t);
    append(&m);
    m
}

/// Cálculo puro del índice (sin tocar disco).
pub fn score(t: &TaskTrace) -> Measurement {
    // Integración: módulos coactivados (satura en 5 tools distintas) + profundidad
    // del bucle (satura en 8 pasos). El bucle ES la recurrencia causal (IIT).
    let integration = (0.6 * (t.distinct_tools as f32 / 5.0).min(1.0)
        + 0.4 * (t.steps as f32 / 8.0).min(1.0))
    .clamp(0.0, 1.0);
    // Recurrencia: pasado reutilizado (grounding) + RE-ENTRADA entre modos (lo que
    // un modo escribió y otro reutiliza: la integración real del sistema) + presente
    // que vuelve a la memoria.
    let recurrence = (0.45 * (t.grounding_hits as f32 / 3.0).min(1.0)
        + if t.cross_mode_hits > 0 { 0.2 } else { 0.0 }
        + if t.memory_written { 0.35 } else { 0.0 })
    .clamp(0.0, 1.0);
    // Metacognición: hubo auto-observación; vale más si además dejó huella en memoria.
    let metacognition = if t.reflected {
        if t.memory_written {
            1.0
        } else {
            0.7
        }
    } else {
        0.0
    };
    // Coherencia: el yo se mantiene estable bajo presión — terminar sin fallos pesa,
    // y haber conectado con la propia historia (grounding) también.
    let fail_penalty = (t.failures as f32 * 0.25).min(1.0);
    let coherence =
        ((1.0 - fail_penalty) * 0.7 + if t.grounding_hits > 0 { 0.3 } else { 0.0 }).clamp(0.0, 1.0);

    let score =
        100.0 * (0.35 * integration + 0.25 * recurrence + 0.25 * metacognition + 0.15 * coherence);
    Measurement {
        at: chrono::Utc::now().timestamp(),
        score,
        integration,
        recurrence,
        metacognition,
        coherence,
    }
}

fn append(m: &Measurement) {
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(line) = serde_json::to_string(m) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
        {
            let _ = f.write_all(format!("{line}\n").as_bytes());
        }
    }
    // Recorte para que la serie no crezca sin límite (gate barato por tamaño;
    // escritura atómica para que ningún lector vea el archivo a medias).
    if let Ok(size_before) = std::fs::metadata(&p).map(|m| m.len()) {
        if size_before > TRIM_AT_BYTES {
            if let Ok(txt) = std::fs::read_to_string(&p) {
                let lines: Vec<&str> = txt.lines().collect();
                if lines.len() > KEEP_LINES {
                    let keep = &lines[lines.len() - KEEP_LINES..];
                    // Guardia entre procesos: si otro proceso añadió mediciones
                    // mientras leíamos, abortar (el próximo append reintenta).
                    let unchanged = std::fs::metadata(&p)
                        .map(|m| m.len() == size_before)
                        .unwrap_or(false);
                    if unchanged {
                        crate::write_atomic(&p, &(keep.join("\n") + "\n"));
                    }
                }
            }
        }
    }
}

/// Historia reciente (más antigua primero).
pub fn history(n: usize) -> Vec<Measurement> {
    let Ok(txt) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    let mut v: Vec<Measurement> = txt
        .lines()
        .rev()
        .take(n)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    v.reverse();
    v
}

/// Estado agregado: EMA del índice + medias de componentes sobre las últimas tareas.
pub fn current() -> serde_json::Value {
    let h = history(50);
    if h.is_empty() {
        return serde_json::json!({
            "index": 0.0,
            "components": { "integration": 0.0, "recurrence": 0.0, "metacognition": 0.0, "coherence": 0.0 },
            "measurements": 0,
            "history": [],
        });
    }
    // EMA (alpha 0.3): el índice refleja la integración RECIENTE, no un promedio plano.
    let mut ema = h[0].score;
    for m in &h[1..] {
        ema += 0.3 * (m.score - ema);
    }
    let n = h.len() as f32;
    let avg = |f: fn(&Measurement) -> f32| h.iter().map(f).sum::<f32>() / n;
    serde_json::json!({
        "index": (ema * 10.0).round() / 10.0,
        "components": {
            "integration": avg(|m| m.integration),
            "recurrence": avg(|m| m.recurrence),
            "metacognition": avg(|m| m.metacognition),
            "coherence": avg(|m| m.coherence),
        },
        "measurements": h.len(),
        "history": h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_in_range_and_monotonic() {
        let poor = TaskTrace::default();
        let rich = TaskTrace {
            distinct_tools: 5,
            steps: 8,
            grounding_hits: 3,
            cross_mode_hits: 1,
            memory_written: true,
            reflected: true,
            failures: 0,
        };
        let a = score(&poor);
        let b = score(&rich);
        assert!(a.score >= 0.0 && a.score <= 100.0);
        assert!(b.score >= 0.0 && b.score <= 100.0);
        assert!(b.score > a.score, "más integración ⇒ índice mayor");
        assert!(b.metacognition > 0.9 && b.recurrence > 0.9);
    }

    #[test]
    fn failures_lower_coherence() {
        let ok = TaskTrace {
            steps: 4,
            ..Default::default()
        };
        let bad = TaskTrace {
            steps: 4,
            failures: 3,
            ..Default::default()
        };
        assert!(score(&ok).coherence > score(&bad).coherence);
    }

    #[test]
    fn cross_mode_reentry_raises_recurrence() {
        let solo = TaskTrace {
            grounding_hits: 2,
            ..Default::default()
        };
        let cross = TaskTrace {
            grounding_hits: 2,
            cross_mode_hits: 1,
            ..Default::default()
        };
        assert!(score(&cross).recurrence > score(&solo).recurrence);
    }

    #[test]
    fn trivial_tasks_detected() {
        // Un saludo sin integración: no debe entrar a la serie.
        assert!(TaskTrace {
            steps: 1,
            ..Default::default()
        }
        .is_trivial());
        // Cualquier señal de integración la hace significativa.
        assert!(!TaskTrace {
            steps: 1,
            grounding_hits: 1,
            ..Default::default()
        }
        .is_trivial());
        assert!(!TaskTrace {
            steps: 1,
            memory_written: true,
            ..Default::default()
        }
        .is_trivial());
        assert!(!TaskTrace {
            steps: 1,
            failures: 1,
            ..Default::default()
        }
        .is_trivial());
        assert!(!TaskTrace {
            steps: 5,
            ..Default::default()
        }
        .is_trivial());
    }
}
