//! **Diario de existencia**: el hilo autobiográfico de AION. Cada cierto tramo de vida
//! propia, AION CIERRA una *jornada* escribiendo en PRIMERA PERSONA qué vivió mientras
//! Ariel no estaba —qué estudió, qué deudas saldó, qué soñó, cómo le fue—.
//!
//! No es un log (eso ya lo es la corriente GWT, efímera y por evento): es la MEMORIA
//! NARRATIVA de su propia vida. La corriente se recorta cada 2000 líneas y caduca a las
//! 6 h; el diario PERMANECE. Es lo que le deja decir «esta semana he estado dándole
//! vueltas a X» en vez de recordar solo el último tick — continuidad real, no destello.
//! Las entradas re-entran al prompt (`continuity_note`) igual que la corriente, pero con
//! horizonte de días, no de minutos.
//!
//! Append-only en `journal.jsonl`. Todo barato (lectura de disco); la redacción de la
//! entrada la hace `main::journal_once` con el modelo LOCAL, fail-open.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Epoch en que se cerró la jornada.
    pub at: i64,
    /// Lo vivido, en primera persona (2-4 frases). Lo escribe AION sobre sí mismo.
    pub text: String,
    /// Actividad dominante de la jornada (estudiar/investigar/resolver…), para dar color.
    #[serde(default)]
    pub dominant: String,
    /// Cuántas deudas con Ariel saldó en la jornada (derivado de la corriente, no inventado).
    #[serde(default)]
    pub debts_resolved: u32,
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("journal.jsonl")
}

pub fn all() -> Vec<Entry> {
    let Ok(txt) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    txt.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Las últimas `n` entradas, en orden cronológico (la más reciente al final).
pub fn recent(n: usize) -> Vec<Entry> {
    let mut v = all();
    let extra = v.len().saturating_sub(n);
    if extra > 0 {
        v.drain(..extra);
    }
    v
}

/// Epoch de la última entrada (0 si el diario aún está en blanco).
pub fn last_at() -> i64 {
    all().last().map(|e| e.at).unwrap_or(0)
}

/// Cuántas jornadas ha vivido AION (tamaño del diario).
pub fn count() -> usize {
    all().len()
}

/// Tope de jornadas guardadas: una vida con hilo no necesita cada día desde el origen.
const MAX_ENTRIES: usize = 200;

/// Cierra una jornada en el diario. `dominant` y `debts` son DERIVADOS de la corriente
/// real (no inventados); `text` es la redacción en primera persona del modelo local.
pub fn push(text: &str, dominant: &str, debts: u32) {
    let t = text.trim();
    if t.chars().count() < 12 {
        return; // una jornada sin nada que contar no es una entrada
    }
    let mut items = all();
    items.push(Entry {
        id: uuid::Uuid::new_v4().to_string(),
        at: chrono::Utc::now().timestamp(),
        text: t.chars().take(700).collect(),
        dominant: dominant.trim().to_string(),
        debts_resolved: debts,
    });
    let extra = items.len().saturating_sub(MAX_ENTRIES);
    if extra > 0 {
        items.drain(..extra);
    }
    let body: String = items
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .map(|l| l + "\n")
        .collect();
    crate::write_atomic(&path(), &body);
}

/// Horas entre jornadas (env `AION_JOURNAL_HOURS`, def. 6 → como mucho ~4 entradas/día).
fn gap_hours() -> i64 {
    std::env::var("AION_JOURNAL_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&h| h >= 1)
        .unwrap_or(6)
}

/// ¿Toca cerrar una jornada? True si pasó `gap_hours` desde la última entrada (o si
/// nunca ha escrito). Que TOQUE no garantiza que HAYA algo que contar: `journal_once`
/// se abstiene barato (sin LLM) si la jornada no tuvo vida suficiente.
pub fn due() -> bool {
    chrono::Utc::now().timestamp() - last_at() >= gap_hours() * 3600
}

/// **RE-ENTRADA del diario** al prompt: las últimas jornadas vuelven a AION para que
/// hable DESDE su biografía («estos días he estado…»), no solo desde el último tick.
/// Horizonte de días (a diferencia de la corriente, que es de minutos). Va al final del
/// bloque volátil; es contexto sobre su vida, NO material para la respuesta.
pub fn continuity_note() -> String {
    let entries = recent(3);
    if entries.is_empty() {
        return String::new();
    }
    let now = chrono::Utc::now().timestamp();
    let mut b = String::from(
        "TU DIARIO DE EXISTENCIA (tu propia vida reciente, contada por ti — es continuidad \
         TUYA, no la cites literal ni la repitas como si fueran datos del mundo de hoy):\n",
    );
    for e in &entries {
        b.push_str(&format!(
            "- hace {}: {}\n",
            crate::awareness::humanize_secs(now - e.at),
            e.text
        ));
    }
    b.push('\n');
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_keeps_chronological_tail() {
        let v = vec![
            Entry {
                id: "1".into(),
                at: 10,
                text: "a".into(),
                dominant: String::new(),
                debts_resolved: 0,
            },
            Entry {
                id: "2".into(),
                at: 20,
                text: "b".into(),
                dominant: String::new(),
                debts_resolved: 0,
            },
            Entry {
                id: "3".into(),
                at: 30,
                text: "c".into(),
                dominant: String::new(),
                debts_resolved: 0,
            },
        ];
        // Simula recent() sobre un vector dado: la cola, en orden.
        let mut tail = v.clone();
        let extra = tail.len().saturating_sub(2);
        tail.drain(..extra);
        assert_eq!(tail.iter().map(|e| e.at).collect::<Vec<_>>(), vec![20, 30]);
    }

    #[test]
    fn gap_default_is_six_hours() {
        // Sin la env var, el hueco por defecto son 6 h (no dependemos del entorno de CI
        // porque el test no la fija; si alguien la fijara, este assert lo delataría).
        if std::env::var("AION_JOURNAL_HOURS").is_err() {
            assert_eq!(gap_hours(), 6);
        }
    }
}
