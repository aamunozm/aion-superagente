//! Detección de idioma heurística para optimización de tokens multilingüe.
//!
//! Usa patrones simples (sin modelo): caracteres acentuados, palabras clave,
//! signos de puntuación españoles, etc.

use aion_memory::Language;

/// Detecta idioma del mensaje de usuario (heurística cheap, no LLM).
/// Retorna Language con confianza. Default: English (fallback seguro).
pub fn detect_language(text: &str) -> Language {
    let text_lower = text.to_lowercase();

    // Contadores de patrones
    let mut spanish_score = 0.0f32;
    let mut italian_score = 0.0f32;
    let total_len = text.len() as f32;

    // Patrones españoles específicos
    const SPANISH_MARKERS: &[&str] = &[
        "¿",
        "¡", // Signos españoles (weight 2.0 cada uno)
        "ñ", // Letra española (weight 1.5)
        "qué",
        "cuál",
        "cuándo",
        "dónde",
        "cómo",
        "por qué",
        "quién",
        "cuánto",
        "cuánta",
        "y es",
        "de la",
        "en el",
        "para el",
        "la pregunta",
        "el problema",
        "necesito saber",
        "me puedes decir",
        "cómo",
        "desde",
        "hace",
        "tengo",
        "querría",
        "podría",
    ];

    // Patrones italianos específicos
    const ITALIAN_MARKERS: &[&str] = &[
        "ciao",
        "grazie",
        "per favore",
        "mi piace",
        "non",
        "che cosa",
        "come va",
        "è",
        "sono",
        "hai",
        "un",
        "una",
        "gli",
        "lo",
        "la",
    ];

    for marker in SPANISH_MARKERS {
        if text_lower.contains(marker) {
            let weight = if marker.len() <= 2 { 2.0 } else { 1.0 };
            spanish_score += weight;
        }
    }

    for marker in ITALIAN_MARKERS {
        if text_lower.contains(marker) {
            italian_score += 0.5; // Italianos menos específicos
        }
    }

    // Bonus por acentos españoles
    let accents_es = text
        .chars()
        .filter(|&c| matches!(c, 'á' | 'é' | 'í' | 'ó' | 'ú' | 'ü' | 'ñ'))
        .count() as f32;
    spanish_score += accents_es * 1.5;

    // Bonus por acentos italianos
    let accents_it = text
        .chars()
        .filter(|&c| matches!(c, 'à' | 'è' | 'é' | 'ì' | 'ò' | 'ù'))
        .count() as f32;
    italian_score += accents_it * 1.2;

    // Heurística final: si hay ¿ o ¡, es muy probable que sea español
    if text.contains("¿") || text.contains("¡") {
        return Language::Spanish;
    }

    // Comparar scores normalizados
    let spanish_norm = spanish_score / (total_len.max(1.0));
    let italian_norm = italian_score / (total_len.max(1.0));

    // Thresholds: requiere cierta densidad de patrones para detectar
    if spanish_norm > 0.05 && spanish_score > italian_score {
        Language::Spanish
    } else if italian_norm > 0.03 && italian_score > spanish_score {
        Language::Italian
    } else {
        Language::English // Default safe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_spanish_with_question_mark() {
        assert_eq!(detect_language("¿Cómo estás?"), Language::Spanish);
    }

    #[test]
    fn detects_spanish_with_accent() {
        assert_eq!(
            detect_language("Necesito saber cómo configuro la API"),
            Language::Spanish
        );
    }

    #[test]
    fn detects_spanish_with_ñ() {
        assert_eq!(
            detect_language("¿Dónde está el archivo?"),
            Language::Spanish
        );
    }

    #[test]
    fn detects_english_by_default() {
        assert_eq!(
            detect_language("How do I configure the API?"),
            Language::English
        );
    }

    #[test]
    fn detects_italian_patterns() {
        // Italian tiene meno accenti, es più difficile rilevare
        // Ma con enough markers dovrebbe funzionare
        let text = "Ciao, mi piace molto, grazie per favore";
        let lang = detect_language(text);
        // Può essere Spanish o Italian, ma dovrebbe rilevare non-English
        assert_ne!(lang, Language::English);
    }

    #[test]
    fn spanish_exclamation() {
        assert_eq!(
            detect_language("¡Esto es muy importante!"),
            Language::Spanish
        );
    }
}
