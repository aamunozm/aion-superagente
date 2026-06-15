//! **Metacognición adaptativa — el pilar de INTELIGENCIA.**
//!
//! AION ya sabía PENSAR profundo (self-consistency, think-mode) — pero lo disparaba por
//! PALABRAS CLAVE ("analiza", "por qué"…), no por si de verdad dudaba de SU PROPIA respuesta.
//! Eso falla en los dos sentidos: gasta cómputo en preguntas fáciles que «suenan» difíciles, y
//! responde a la ligera preguntas difíciles que «suenan» triviales.
//!
//! Este módulo añade la señal que faltaba: **estimar la confianza en la respuesta concreta**
//! (no en el tema) y usarla para (1) ESCALAR el esfuerzo solo cuando hace falta (cómputo
//! adaptativo: un segundo candidato + juez si AION duda), y (2) ser HONESTO de forma calibrada
//! cuando, aun tras escalar, sigue inseguro — decir «no estoy del todo seguro» en vez de fingir
//! certeza. Es la frontera 2026 del "saber cuándo pensar más y cuándo admitir que no se sabe".
//!
//! Barato (un juez de 1 token) y fail-open: si el juez no responde, asume confianza NEUTRA (no
//! escala ni matiza) — nunca degrada el camino normal.

use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;

/// Confianza neutra por defecto (escala 1–5): ante fallo del juez, ni escalamos ni matizamos.
pub const NEUTRAL: u8 = 3;
/// Umbral de DUDA: confianza ≤ esto → escalar el esfuerzo (segundo candidato + juez).
pub const ESCALATE_AT: u8 = 2;
/// Umbral de HONESTIDAD: confianza ≤ esto (tras escalar) → matizar con franqueza la respuesta.
/// Más estricto que escalar: escalar es invisible/barato; matizar se VE, así que pedimos duda real.
pub const HEDGE_AT: u8 = 1;

/// **Auto-confianza**: AION juzga, en 1–5, cuán seguro está de que SU respuesta sea correcta,
/// clara y útil para la pregunta. Es la pieza metacognitiva clave: mirar la propia respuesta con
/// ojo crítico antes de darla por buena. Un único token, temperatura 0. Fail-open a `NEUTRAL`.
pub async fn self_confidence(engine: &dyn LlmEngine, question: &str, answer: &str) -> u8 {
    // Acota la respuesta para el juez (no necesita el texto entero para calibrar).
    let ans: String = answer.chars().take(700).collect();
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres un evaluador severo de TU PROPIA respuesta. Del 1 al 5, ¿cuán seguro estás \
                 de que esta respuesta es CORRECTA, precisa y realmente útil para la pregunta? \
                 1 = podría estar inventando o equivocándome; 5 = certeza fundada. Sé honesto y \
                 exigente: si afirmas datos que no puedes verificar, baja la nota. Responde SOLO \
                 un dígito del 1 al 5.",
            ),
            Message::user(format!(
                "Pregunta: {question}\n\nRespuesta: {ans}\n\nTu confianza (1-5, SOLO el dígito):"
            )),
        ],
        think: false,
        temperature: Some(0.0),
        // ≥10: los modelos de razonamiento (gemma4-reason) emiten un token inicial (espacio/
        // salto) antes del dígito; con un presupuesto de 2 la respuesta salía VACÍA y el juez
        // caía siempre a NEUTRAL. Con holgura, emite el dígito y la calibración funciona.
        max_tokens: Some(12),
    };
    match engine.generate(req).await {
        Ok(m) => parse_score(&m.content).unwrap_or(NEUTRAL),
        Err(_) => NEUTRAL,
    }
}

/// Extrae el primer dígito 1–5 de la respuesta del juez (tolerante a preámbulos/ruido).
fn parse_score(s: &str) -> Option<u8> {
    s.chars()
        .find_map(|c| c.to_digit(10))
        .map(|d| d as u8)
        .filter(|&d| (1..=5).contains(&d))
}

/// **Honestidad calibrada**: si tras pensar AION sigue muy inseguro (`confidence ≤ HEDGE_AT`),
/// devuelve un prefijo breve que reconoce la duda sin dejar de ayudar. Si no, `None` (no añade
/// ruido a respuestas que sí domina). Esto es decir la verdad sobre el propio estado epistémico.
pub fn hedge(confidence: u8) -> Option<&'static str> {
    if confidence <= HEDGE_AT {
        Some("Te respondo, pero con honestidad: no estoy del todo seguro de esto — tómalo como una aproximación, no como un dato firme.\n\n")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_score_extracts_digit() {
        assert_eq!(parse_score("4"), Some(4));
        assert_eq!(parse_score(" 5."), Some(5));
        assert_eq!(parse_score("confianza: 2"), Some(2));
        assert_eq!(parse_score("1/5"), Some(1));
        assert_eq!(parse_score("9"), None); // fuera de rango
        assert_eq!(parse_score("ninguno"), None);
    }

    #[test]
    fn hedge_only_when_very_unsure() {
        assert!(hedge(1).is_some());
        assert!(hedge(2).is_none()); // 2 escala, pero no matiza (escalar es invisible)
        assert!(hedge(3).is_none());
        assert!(hedge(5).is_none());
    }

    #[test]
    fn thresholds_are_coherent() {
        // Matizar es más estricto que escalar: primero piensa más, solo si aun así dudas, lo dices.
        assert!(HEDGE_AT <= ESCALATE_AT);
        assert!(ESCALATE_AT < NEUTRAL);
    }
}
