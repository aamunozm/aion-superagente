//! **Señal de español para la compactación EN del puente MCP.**
//!
//! No clasifica el idioma con precisión: solo decide, de forma barata y robusta, si un
//! recuerdo vale la pena traducirse a inglés antes de enviarlo a Claude Code (ver
//! `crate::mcp_compact`). El bias está en `has_spanish_signal`.

/// ¿El texto contiene señal clara de español? Decide si vale la pena traducir a inglés.
///
/// Bias deliberado hacia el "sí": un falso positivo (traducir algo que ya era inglés) es
/// barato e inocuo (Gemma devuelve ~lo mismo, una vez, cacheado), pero un falso negativo
/// (saltar un recuerdo español) PIERDE el ahorro que justifica todo esto. Por eso basta
/// UNA señal española — acento/ñ, signo ¿¡, o dos palabras función comunes — para tratarlo
/// como traducible. El español técnico (anglicismos, pocos acentos) pasa de sobra.
pub fn has_spanish_signal(text: &str) -> bool {
    if text
        .chars()
        .any(|c| matches!(c, 'á' | 'é' | 'í' | 'ó' | 'ú' | 'ñ' | '¡' | '¿'))
    {
        return true;
    }
    // Palabras función españolas que casi no existen en inglés (whole-word, en minúsculas).
    const ES_FUNC: &[&str] = &[
        "el", "la", "los", "las", "un", "una", "de", "del", "que", "qué", "en", "por", "para",
        "con", "sin", "sino", "como", "pero", "su", "sus", "se", "lo", "al", "es", "esta", "estan",
        "y", "o", "no", "mas", "muy", "ya", "porque", "cuando", "donde", "segun",
    ];
    let t = text.to_lowercase();
    let mut hits = 0;
    for w in t.split(|c: char| !c.is_alphabetic()) {
        if !w.is_empty() && ES_FUNC.contains(&w) {
            hits += 1;
            if hits >= 2 {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod spanish_signal {
    use super::*;

    #[test]
    fn real_technical_spanish_memories_have_signal() {
        // Recuerdos REALES estilo AION que Claude Code recupera (técnicos, con anglicismos).
        let samples = [
            "[hecho] El pendiente crítico no es el grafo sino la autenticación y el CORS de la API local en el puerto 8765, según la auditoría integral de junio de 2026.",
            "[aprendizaje] Cuando el agente entra en bucle de 8 vueltas y da timeout, suele ser por descripciones de herramientas recortadas que rompen las llamadas.",
            "[hecho] Ariel decidió usar Rust para el núcleo de AION porque la seguridad de memoria y el rendimiento sin recolector de basura son críticos.",
            "[proyecto: aion] El LLM es intercambiable tras el trait LlmEngine; Ollama hoy, mistral.rs en roadmap.",
            // Sin acentos a propósito: debe pasar por palabras función (en, sobre, con, y…).
            "[preferencia] Responde siempre en espanol, concreto sobre generico, con analisis de negocio y arquitectura.",
        ];
        for s in samples {
            assert!(
                has_spanish_signal(s),
                "sin señal española (se saltaría): {s}"
            );
        }
    }

    #[test]
    fn clearly_english_has_no_signal() {
        // Notas en inglés (p. ej. origen Claude Code) NO deben gatillar traducción.
        let samples = [
            "Critical pending item is auth plus CORS on local API port 8765 per the June 2026 audit.",
            "Use the LlmEngine trait so the model stays swappable; Ollama today, mistral.rs on the roadmap.",
            "Ariel prefers concrete answers over generic ones.",
        ];
        for s in samples {
            assert!(!has_spanish_signal(s), "falso positivo de español: {s}");
        }
    }
}
