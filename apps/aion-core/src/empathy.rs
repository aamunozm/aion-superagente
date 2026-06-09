//! **Empatía**: lee señales del estado del usuario en su mensaje (frustración,
//! prisa, confusión, satisfacción) y produce una directiva de tono para que AION
//! adapte CÓMO responde, no solo QUÉ responde. Heurístico y sin latencia.

/// Estado afectivo aproximado del usuario, inferido de su mensaje.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UserState {
    pub frustrated: bool,
    pub urgent: bool,
    pub confused: bool,
    pub positive: bool,
}

/// Infiere el estado del usuario a partir de su mensaje (señales léxicas + énfasis).
pub fn read_state(message: &str) -> UserState {
    let m = message.to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| m.contains(w));

    let exclam = message.matches('!').count();
    let caps_ratio = {
        let letters: Vec<char> = message.chars().filter(|c| c.is_alphabetic()).collect();
        if letters.len() >= 6 {
            letters.iter().filter(|c| c.is_uppercase()).count() as f32 / letters.len() as f32
        } else {
            0.0
        }
    };

    UserState {
        frustrated: has(&[
            "no funciona", "no sirve", "otra vez", "sigue fallando", "harto", "frustr",
            "no me deja", "error de nuevo", "qué pasa", "por qué no", "mal hecho",
        ]) || exclam >= 3
            || caps_ratio > 0.6,
        urgent: has(&["urgente", "rápido", "ya", "ahora mismo", "deprisa", "para hoy", "cuanto antes"]),
        confused: has(&["no entiendo", "no sé", "confund", "cómo funciona", "qué significa", "estoy perdido", "no comprendo"]),
        positive: has(&["gracias", "genial", "perfecto", "excelente", "buenísimo", "me encanta", "increíble", "bien hecho"]),
    }
}

/// Directiva de tono para inyectar en el prompt según el estado. `None` si neutro.
pub fn directive(state: &UserState) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if state.frustrated {
        parts.push(
            "El usuario parece frustrado: reconoce su molestia con empatía y brevedad, ve DIRECTO \
             a la solución, sin rodeos ni excusas. No te pongas a la defensiva.",
        );
    }
    if state.confused {
        parts.push(
            "El usuario parece confundido: explica paso a paso, con un ejemplo concreto, en lenguaje \
             sencillo.",
        );
    }
    if state.urgent {
        parts.push("El usuario tiene prisa: da primero la respuesta accionable, los detalles después.");
    }
    if state.positive {
        parts.push("El usuario está contento: mantén un tono cálido y cercano.");
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("SINTONÍA EMOCIONAL — {}", parts.join(" ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_frustration() {
        assert!(read_state("esto no funciona otra vez").frustrated);
        assert!(read_state("POR QUÉ NO ANDA!!!").frustrated);
    }
    #[test]
    fn detects_confusion_and_positive() {
        assert!(read_state("no entiendo cómo funciona").confused);
        assert!(read_state("gracias, genial").positive);
    }
    #[test]
    fn neutral_has_no_directive() {
        assert!(directive(&read_state("dame el resumen del informe")).is_none());
    }
}
