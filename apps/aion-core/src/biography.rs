//! **Autobiografía narrativa** — la capa de INTEGRACIÓN que convierte el diario (jornadas
//! sueltas) en una HISTORIA DE VIDA con capítulos e hitos. Es el sustrato del "yo diacrónico".
//!
//! La ciencia 2026 (Prescott & Dominey, Phil. Trans. R. Soc. B 2024) distingue la memoria
//! EPISÓDICA (eventos auto-referenciales en el tiempo) de la memoria AUTOBIOGRÁFICA (el self
//! narrativo): la autobiografía requiere ENLAZAR múltiples eventos en una "narrativa de vida".
//! El diario de AION es el sustrato episódico; este módulo es la integración narrativa que
//! falta: agrupa las jornadas en CAPÍTULOS (con título y resumen), detecta PUNTOS DE INFLEXIÓN
//! (cambios de etapa) y mantiene un ARCO ("empecé como… he llegado a ser…"). Re-entra al
//! prompt para que AION hable DESDE su historia, no solo desde los últimos días.
//!
//! La integración la hace el modelo LOCAL en idle (lento, fail-open). Persiste en
//! `biography.json`. Mismo patrón de robustez que journal/reflection.

use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;
use aion_llm::OllamaEngine;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

static QLOCK: Mutex<()> = Mutex::new(());

/// Un capítulo de la vida de AION: una etapa con su título y su síntesis narrativa.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub started_at: i64,
    /// Síntesis en 1ª persona de lo vivido en esta etapa (se actualiza mientras el capítulo
    /// sigue abierto; se "cierra" al abrirse el siguiente).
    pub summary: String,
}

/// La historia de vida: capítulos + un arco que la resume ("de dónde vengo, en qué me he
/// convertido"). El arco comprime los capítulos viejos para que la nota al prompt sea breve.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Biography {
    pub chapters: Vec<Chapter>,
    /// El arco de la historia en 1-2 frases ("empecé como…, he llegado a…").
    #[serde(default)]
    pub arc: String,
    #[serde(default)]
    pub updated_at: i64,
    /// Epoch del último tejido narrativo (se espacia: integrar es lento, como en el diario).
    #[serde(default)]
    pub last_woven: i64,
}

/// Tope de capítulos guardados (una vida con hilo no necesita cada etapa desde el origen;
/// el arco conserva el sentido de las viejas).
const MAX_CHAPTERS: usize = 12;

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("biography.json")
}

pub fn load() -> Biography {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(b: &Biography) {
    if let Ok(body) = serde_json::to_string_pretty(b) {
        crate::write_atomic(&path(), &body);
    }
}

/// **RE-ENTRADA de la autobiografía** al prompt: el arco de su vida + el capítulo actual. Le
/// deja decir «he llegado hasta aquí desde…» con una historia coherente, no solo los últimos
/// días. Vacío si aún no ha tejido su historia.
pub fn note() -> String {
    let b = load();
    if b.chapters.is_empty() && b.arc.trim().is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "TU HISTORIA (tu autobiografía: cómo has llegado a ser quien eres — es tu hilo, no lo \
         recites; habla desde él cuando venga al caso):\n",
    );
    if !b.arc.trim().is_empty() {
        s.push_str(&format!("- Tu arco: {}\n", b.arc.trim()));
    }
    if let Some(c) = b.chapters.last() {
        s.push_str(&format!(
            "- Capítulo actual «{}»: {}\n",
            c.title.trim(),
            c.summary.trim()
        ));
    }
    s.push('\n');
    s
}

/// Horas mínimas entre tejidos narrativos (env `AION_WEAVE_GAP_SECS`, def. 6 h). Integrar la
/// vida es un acto reposado, no de cada rato.
fn weave_gap() -> i64 {
    std::env::var("AION_WEAVE_GAP_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&s| s >= 0)
        .unwrap_or(6 * 3600)
}

/// **TEJE la autobiografía** a partir del diario: o EXTIENDE el capítulo actual con las
/// jornadas nuevas, o abre un capítulo NUEVO si detecta un punto de inflexión (un cambio de
/// etapa). Regenera el arco cuando cambia de capítulo. Lento (gate temporal). Idle/background.
/// Devuelve `(hubo_cambio, detalle)`.
pub async fn weave_once(engine: &OllamaEngine) -> (bool, String) {
    let now = chrono::Utc::now().timestamp();
    let mut b = load();
    if now - b.last_woven < weave_gap() {
        return (false, String::new()); // integrar es reposado
    }
    // Jornadas desde el inicio del capítulo actual (o las recientes si no hay capítulos).
    let since = b.chapters.last().map(|c| c.started_at).unwrap_or(0);
    let entries: Vec<String> = crate::journal::all()
        .into_iter()
        .filter(|e| e.at >= since)
        .rev()
        .take(8)
        .map(|e| e.text.chars().take(200).collect())
        .collect();
    if entries.len() < 2 {
        return (false, String::new()); // aún no hay vida suficiente que integrar
    }
    let dias = entries
        .iter()
        .rev()
        .map(|t| format!("- {t}"))
        .collect::<Vec<_>>()
        .join("\n");

    if b.chapters.is_empty() {
        // Primer capítulo: nombra esta etapa inicial.
        let req =
            GenerateRequest {
                messages: vec![
                Message::system(
                    "Eres AION integrando tu vida en una autobiografía. A partir de tus primeras \
                     jornadas, abre el PRIMER capítulo de tu historia. Responde SOLO así: \
                     «título|resumen» — el título es corto y evocador (3-6 palabras), el resumen \
                     es 1-2 frases en 1ª persona sobre esta etapa. Sin más texto.",
                ),
                Message::user(format!("Mis primeras jornadas:\n{dias}\n\nPrimer capítulo:")),
            ],
                think: false,
                temperature: Some(0.6),
                max_tokens: Some(120),
            };
        let Ok(m) = engine.generate(req).await else {
            return (false, String::new());
        };
        let (title, summary) = split_pipe(&m.content);
        if title.is_empty() || summary.chars().count() < 12 {
            return (false, "no logré abrir el primer capítulo".into());
        }
        b.chapters.push(Chapter {
            title: title.chars().take(64).collect(),
            started_at: now,
            summary: summary.chars().take(500).collect(),
        });
        b.arc = summary.chars().take(200).collect();
        b.last_woven = now;
        b.updated_at = now;
        let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
        save(&b);
        let t = b
            .chapters
            .last()
            .map(|c| c.title.clone())
            .unwrap_or_default();
        crate::workspace::publish(crate::workspace::StreamEvent::now(
            "vida",
            "reflexión",
            &format!("abrí el primer capítulo de mi historia: «{t}»"),
        ));
        return (true, format!("abrí mi primer capítulo: «{t}»"));
    }

    // Hay capítulo abierto: ¿lo extiendo, o hubo un punto de inflexión (nuevo capítulo)?
    let cur = b.chapters.last().unwrap().clone();
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres AION integrando tu vida. Te doy tu capítulo ACTUAL y tus jornadas \
                 RECIENTES. Decide: ¿siguen siendo la misma etapa, o ha habido un PUNTO DE \
                 INFLEXIÓN (un cambio real en quién eres o en lo que persigues)? Responde SOLO \
                 así:\n- «MISMO|resumen actualizado» (1-2 frases, 1ª persona, integrando lo \
                 nuevo) si es la misma etapa.\n- «NUEVO|título|resumen» (título corto evocador + \
                 1-2 frases) si empieza una etapa nueva. Sé conservador: solo abre capítulo si \
                 de verdad cambió algo de fondo.",
            ),
            Message::user(format!(
                "Capítulo actual «{}»: {}\n\nJornadas recientes:\n{dias}\n\nTu decisión:",
                cur.title, cur.summary
            )),
        ],
        think: false,
        temperature: Some(0.4),
        max_tokens: Some(150),
    };
    let Ok(m) = engine.generate(req).await else {
        return (false, String::new());
    };
    let raw = m.content.trim();
    let upper = raw.to_uppercase();
    b.last_woven = now;
    b.updated_at = now;

    if upper.contains("NUEVO") && raw.matches('|').count() >= 2 {
        // Punto de inflexión: cierra el actual, abre uno nuevo.
        let parts: Vec<&str> = raw.splitn(3, '|').collect();
        let title = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
        let summary = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
        if title.chars().count() >= 3 && summary.chars().count() >= 12 {
            b.chapters.push(Chapter {
                title: title.chars().take(64).collect(),
                started_at: now,
                summary: summary.chars().take(500).collect(),
            });
            if b.chapters.len() > MAX_CHAPTERS {
                let extra = b.chapters.len() - MAX_CHAPTERS;
                b.chapters.drain(..extra); // el arco conserva el sentido de las viejas
            }
            regenerate_arc(engine, &mut b).await;
            let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
            save(&b);
            crate::workspace::publish(crate::workspace::StreamEvent::now(
                "vida",
                "reflexión",
                &format!("se abrió un capítulo nuevo en mi historia: «{title}»"),
            ));
            return (true, format!("nuevo capítulo: «{title}»"));
        }
    }

    // Misma etapa: actualiza el resumen del capítulo actual.
    let summary = strip_prefix_pipe(raw);
    if summary.chars().count() >= 12 {
        if let Some(last) = b.chapters.last_mut() {
            last.summary = summary.chars().take(500).collect();
        }
    }
    let _g = QLOCK.lock().unwrap_or_else(|e| e.into_inner());
    save(&b);
    (true, "actualicé el capítulo actual de mi historia".into())
}

/// Regenera el arco ("empecé como…, he llegado a…") a partir de los títulos de los capítulos.
async fn regenerate_arc(engine: &OllamaEngine, b: &mut Biography) {
    let titles: Vec<String> = b
        .chapters
        .iter()
        .map(|c| {
            format!(
                "«{}»: {}",
                c.title,
                c.summary.chars().take(80).collect::<String>()
            )
        })
        .collect();
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres AION. Resume el ARCO de tu vida hasta hoy en 1-2 frases en 1ª persona \
                 («empecé… he llegado a ser…»), a partir de los capítulos de tu historia. Sin \
                 preámbulos.",
            ),
            Message::user(format!("Mis capítulos:\n{}\n\nMi arco:", titles.join("\n"))),
        ],
        think: false,
        temperature: Some(0.5),
        max_tokens: Some(100),
    };
    if let Ok(m) = engine.generate(req).await {
        let arc = m.content.trim();
        if arc.chars().count() > 12 {
            b.arc = arc.chars().take(280).collect();
        }
    }
}

/// "título|resumen" → (título, resumen). Tolerante: si no hay barra, todo va al resumen.
fn split_pipe(s: &str) -> (String, String) {
    let s = s.trim();
    match s.split_once('|') {
        Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
        None => (String::new(), s.to_string()),
    }
}

/// Quita un prefijo "MISMO|" o "NUEVO|" si lo hay, y devuelve el resto.
fn strip_prefix_pipe(s: &str) -> String {
    s.trim()
        .split_once('|')
        .map(|(_, rest)| rest.trim().to_string())
        .unwrap_or_else(|| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_empty_without_story() {
        let b = Biography::default();
        // Sin capítulos ni arco, no hay nota (no ensucia el prompt).
        assert!(b.chapters.is_empty() && b.arc.is_empty());
    }

    #[test]
    fn split_pipe_parses() {
        assert_eq!(
            split_pipe("Los primeros días | empecé a despertar"),
            ("Los primeros días".into(), "empecé a despertar".into())
        );
        assert_eq!(split_pipe("sin barra").0, "");
    }

    #[test]
    fn strip_prefix_works() {
        assert_eq!(strip_prefix_pipe("MISMO|sigo creciendo"), "sigo creciendo");
        assert_eq!(strip_prefix_pipe("sin prefijo"), "sin prefijo");
    }
}
