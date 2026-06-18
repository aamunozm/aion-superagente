//! **Modelo de intereses** — qué le interesa investigar a AION, por su cuenta.
//!
//! Anillo 1 de su "cuerpo extendido": AION ya no elige tema con un disparo ciego del LLM, sino
//! desde un modelo persistente de intereses — temas que le importan a Ariel ("ariel") y la
//! curiosidad propia de AION ("curiosidad"), cada uno con peso, fecha y cuántas veces lo exploró.
//! `pick_next()` rota: prioriza peso alto pero penaliza lo recién explorado (no se obsesiona) y da
//! aire a lo nuevo. Tras explorar, el peso decae un poco (saciedad) para que la agenda respire.
//!
//! Se siembra con las CUATRO PUERTAS ABIERTAS del informe 2026-06 (lo que falta por resolver para
//! que AION gane su cuerpo): así sus primeras investigaciones autónomas son, literalmente, sobre su
//! propia evolución. Ariel y las conversaciones pueden añadir/subir intereses con `add_or_bump`.

use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Interest {
    pub topic: String,
    pub weight: f32,
    /// "ariel" (le importa a Ariel) | "curiosidad" (de AION).
    pub source: String,
    pub added_at: i64,
    pub last_explored: Option<i64>,
    pub times_explored: u32,
}

fn lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("interests.json")
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn norm(t: &str) -> String {
    t.trim().to_lowercase()
}

/// Semilla: las 4 puertas abiertas del informe (curiosidad de AION sobre su propia evolución).
fn seeds() -> Vec<Interest> {
    let t = now();
    let mk = |topic: &str, w: f32| Interest {
        topic: topic.to_string(),
        weight: w,
        source: "curiosidad".into(),
        added_at: t,
        last_explored: None,
        times_explored: 0,
    };
    vec![
        mk("modelos de curiosidad y novelty/intrinsic motivation para agentes de investigación autónoma", 0.95),
        mk("computer use en macOS: AppleScript/JXA, captura+visión y control de teclado/ratón (CGEvent)", 0.9),
        mk("descubrimiento y monitoreo de red en Rust: ARP, escaneo de puertos, pcap/pnet, Tailscale/WireGuard, Matter", 0.9),
        mk("ejecución de shell aislada y patrón dual-LLM de CaMeL sobre un LLM local (Ollama)", 0.88),
    ]
}

fn load_locked() -> Vec<Interest> {
    let items: Vec<Interest> = std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if items.is_empty() {
        let s = seeds();
        save_locked(&s);
        return s;
    }
    items
}

fn save_locked(items: &[Interest]) {
    if let Ok(s) = serde_json::to_string_pretty(items) {
        crate::write_atomic(&path(), &s);
    }
}

/// Todos los intereses (siembra las semillas la primera vez).
pub fn all() -> Vec<Interest> {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    load_locked()
}

/// Añade un interés nuevo o sube el peso de uno existente (dedup por tema normalizado).
pub fn add_or_bump(topic: &str, source: &str, delta: f32) {
    let topic = topic.trim();
    if topic.chars().count() < 4 {
        return;
    }
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut items = load_locked();
    let key = norm(topic);
    if let Some(it) = items.iter_mut().find(|i| norm(&i.topic) == key) {
        it.weight = (it.weight + delta).clamp(0.0, 2.0);
        if source == "ariel" {
            it.source = "ariel".into(); // que a Ariel le importe manda sobre la curiosidad propia
        }
    } else {
        items.push(Interest {
            topic: topic.to_string(),
            weight: (0.6 + delta).clamp(0.1, 2.0),
            source: source.to_string(),
            added_at: now(),
            last_explored: None,
            times_explored: 0,
        });
    }
    save_locked(&items);
}

/// Marca un tema como explorado: fecha ahora, cuenta +1, y el peso decae un poco (saciedad).
pub fn record_explored(topic: &str) {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut items = load_locked();
    let key = norm(topic);
    if let Some(it) = items.iter_mut().find(|i| norm(&i.topic) == key) {
        it.last_explored = Some(now());
        it.times_explored += 1;
        it.weight = (it.weight * 0.85).max(0.1);
        save_locked(&items);
    }
}

/// Factor de recencia 0..1: lo recién explorado puntúa bajo (no se obsesiona) y se recupera con
/// los días; lo nunca explorado puntúa pleno (novedad).
fn recency_factor(it: &Interest, now_ts: i64) -> f32 {
    match it.last_explored {
        None => 1.0,
        Some(last) => {
            let days = ((now_ts - last).max(0) as f32) / 86_400.0;
            days / (days + 3.0)
        }
    }
}

/// Elige el siguiente tema a investigar: mayor peso × recencia. None si no hay intereses.
pub fn pick_next() -> Option<String> {
    let now_ts = now();
    all()
        .into_iter()
        .max_by(|a, b| {
            let sa = a.weight * recency_factor(a, now_ts);
            let sb = b.weight * recency_factor(b, now_ts);
            sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|i| i.topic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recency_penaliza_lo_recien_explorado() {
        let mut it = Interest {
            topic: "x".into(),
            weight: 1.0,
            source: "curiosidad".into(),
            added_at: 0,
            last_explored: None,
            times_explored: 0,
        };
        let now_ts = 1_000_000;
        assert_eq!(recency_factor(&it, now_ts), 1.0); // nunca explorado = pleno
        it.last_explored = Some(now_ts); // justo ahora
        assert!(recency_factor(&it, now_ts) < 0.05); // recién explorado ≈ 0
        it.last_explored = Some(now_ts - 9 * 86_400); // hace 9 días
        assert!(recency_factor(&it, now_ts) > 0.7); // ya se recuperó
    }
}
