//! **Señal de idioma para la compactación EN del puente MCP.**
//!
//! No clasifica el idioma con precisión: solo decide, de forma barata y robusta, si un
//! recuerdo vale la pena traducirse a inglés antes de enviarlo a Claude Code (ver
//! `crate::mcp_compact`). Ariel es chileno viviendo en Italia, así que su memoria puede
//! estar en **español o italiano** — ambos cuestan ~40% más tokens que el inglés. El gate
//! público es `needs_english_translation`; el bias (hacia traducir) vive en cada detector.

/// ¿El texto contiene señal clara de español? Decide si vale la pena traducir a inglés.
///
/// Bias deliberado hacia el "sí": un falso positivo (traducir algo que ya era inglés) es
/// barato e inocuo (Gemma devuelve ~lo mismo, una vez, cacheado), pero un falso negativo
/// (saltar un recuerdo español) PIERDE el ahorro que justifica todo esto. Por eso basta
/// UNA señal española — acento/ñ, signo ¿¡, o dos palabras función comunes — para tratarlo
/// como traducible. El español técnico (anglicismos, pocos acentos) pasa de sobra.
#[allow(dead_code)]
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

/// ¿El texto contiene señal clara de italiano? Mismo bias e idea que el español: Ariel vive
/// en Italia y puede guardar recuerdos en italiano, igual de caros en tokens que el español.
#[allow(dead_code)]
pub fn has_italian_signal(text: &str) -> bool {
    // Acentos GRAVES à è ì ò ù: típicos del italiano y ausentes del español (que usa los
    // agudos á í ó ú y la ñ). Una sola aparición ya es señal fuerte e inequívoca.
    if text
        .chars()
        .any(|c| matches!(c, 'à' | 'è' | 'ì' | 'ò' | 'ù'))
    {
        return true;
    }
    // Palabras función italianas (whole-word, minúsculas). Algunas coinciden con el español
    // (la, un, una, con…) o con el inglés (come): por eso se exigen DOS, como en el español.
    const IT_FUNC: &[&str] = &[
        "il", "lo", "la", "gli", "le", "di", "del", "della", "dello", "dei", "degli", "delle",
        "che", "chi", "non", "per", "con", "una", "uno", "un", "sono", "anche", "come", "questo",
        "questa", "quello", "quella", "nel", "nella", "alla", "dal", "sul", "ma", "se", "ed", "e",
        "ho", "ha", "hanno", "essere", "fare", "quando", "dove", "ci", "si", "tra", "fra", "su",
    ];
    let t = text.to_lowercase();
    let mut hits = 0;
    for w in t.split(|c: char| !c.is_alphabetic()) {
        if !w.is_empty() && IT_FUNC.contains(&w) {
            hits += 1;
            if hits >= 2 {
                return true;
            }
        }
    }
    false
}

/// Gate del puente MCP: ¿vale la pena traducir esto a inglés? Sí si hay señal de español o
/// de italiano (las dos lenguas de Ariel), ambas más caras en tokens que el inglés. Bias a
/// traducir: un falso positivo (texto ya inglés) solo cuesta una traducción cacheada inocua.
#[allow(dead_code)]
pub fn needs_english_translation(text: &str) -> bool {
    has_spanish_signal(text) || has_italian_signal(text)
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
    fn real_technical_italian_memories_have_signal() {
        // Recuerdos REALES estilo AION en italiano (Ariel vive en Italia).
        let samples = [
            "[fatto] Il punto critico non è il grafo ma l'autenticazione e il CORS dell'API locale sulla porta 8765, secondo l'audit di giugno 2026.",
            "[preferenza] Rispondi sempre in italiano, conciso, con analisi di business e architettura.",
            // Sin acentos a propósito: debe pasar por palabras función (il, e, per, di…).
            "[progetto: aion] Il modello LLM e intercambiabile dietro il trait LlmEngine; Ollama oggi, mistral.rs nella roadmap.",
        ];
        for s in samples {
            assert!(
                has_italian_signal(s),
                "sin señal italiana (se saltaría): {s}"
            );
            assert!(
                needs_english_translation(s),
                "gate no detecta italiano: {s}"
            );
        }
    }

    #[test]
    fn clearly_english_has_no_signal() {
        // Notas en inglés (p. ej. origen Claude Code) NO deben gatillar traducción en NINGÚN
        // idioma: ni español ni italiano ni el gate combinado.
        let samples = [
            "Critical pending item is auth plus CORS on local API port 8765 per the June 2026 audit.",
            "Use the LlmEngine trait so the model stays swappable; Ollama today, mistral.rs on the roadmap.",
            "Ariel prefers concrete answers over generic ones.",
        ];
        for s in samples {
            assert!(!has_spanish_signal(s), "falso positivo de español: {s}");
            assert!(!has_italian_signal(s), "falso positivo de italiano: {s}");
            assert!(
                !needs_english_translation(s),
                "el gate combinado da falso positivo en inglés: {s}"
            );
        }
    }
}
