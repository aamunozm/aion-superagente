//! **Compactación para el puente MCP con Claude Code.**
//!
//! AION es local-first: el chat con Gemma corre on-device y sus tokens son *gratis*. Cuando un
//! agente externo (Claude Code) consulta la memoria vía MCP, ese texto entra en el contexto de un
//! modelo de pago, así que se sirve TRUNCADO a un presupuesto de caracteres (brief 180,
//! `aion_memory_search` 300, grafo 160…) — el ahorro real del puente es el **retrieval bajo
//! demanda** (servir solo lo relevante), no más.
//!
//! **Histórico:** hubo una capa de traducción ES→EN aquí (warmer, caché, QE back-translation).
//! Se RETIRÓ (2026-06-24) tras auditar su ahorro REAL: ~1% sobre las rutas traducibles, porque las
//! salidas están topadas (traducir solo densificaba, no abarataba) — no compensaba su complejidad
//! ni el coste de arranque del modelo de traducción. Hoy la memoria se sirve en su idioma original,
//! simplemente truncada al presupuesto.

/// Trunca `s` a lo sumo `max_chars` SIN cortar a media palabra: si el corte cae pasada la
/// mitad, retrocede al último espacio. Barato y suficiente para los snippets del puente.
pub fn take_words(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    match truncated.rfind(' ') {
        // `rfind` da índice de byte; el espacio es 1 byte → el slice cae en frontera de char.
        Some(i) if i >= truncated.len() / 2 => truncated[..i].to_string(),
        _ => truncated,
    }
}

/// Sirve un recuerdo al puente truncado al presupuesto de caracteres (en su idioma original).
pub fn compact_take(content: &str, max_chars: usize) -> String {
    take_words(content, max_chars)
}

/// Igual que [`compact_take`]: trunca al presupuesto. (Antes generaba un resumen denso en inglés;
/// retirado junto con la traducción.) Usado por el brief y `aion_memory_search`.
pub fn compact_dense(content: &str, max_chars: usize) -> String {
    take_words(content, max_chars)
}

/// Sirve contenido al puente tal cual (identidad). Se conserva como punto único por si en el
/// futuro se reintroduce algún post-proceso del puente; hoy no transforma.
pub fn compact_for_bridge(content: &str) -> String {
    content.to_string()
}

/// Bloque de *grounding* de biblioteca para el puente: hoy se sirve sin transformar (la estructura
/// y la prosa quedan intactas). Se conserva la firma para los llamadores.
pub fn compact_grounding(blob: &str) -> String {
    blob.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_words_does_not_split_words() {
        // Cabe entero → intacto.
        assert_eq!(take_words("hola mundo", 100), "hola mundo");
        // Trunca retrocediendo al último espacio (no parte la palabra) y no excede el corte.
        let s = "uno dos tres cuatro cinco seis";
        let t = take_words(s, 14);
        assert!(t.chars().count() <= 14, "no excede el presupuesto");
        assert!(!t.ends_with(' '), "no deja espacio colgando");
        assert!(s.starts_with(&t), "es un prefijo del original");
    }

    #[test]
    fn compact_take_truncates_to_budget() {
        let content = "Este es un recuerdo en español que excede claramente el corte impuesto.";
        let out = compact_take(content, 30);
        assert!(out.chars().count() <= 30);
        assert!(content.starts_with(&out));
    }

    #[test]
    fn compact_dense_truncates_to_budget() {
        let content = "Recuerdo largo en español: ".to_string() + &"dato ".repeat(40);
        let out = compact_dense(&content, 60);
        assert!(out.chars().count() <= 60);
        assert!(content.starts_with(&out));
    }

    #[test]
    fn compact_grounding_is_identity() {
        let blob =
            "Conocimiento de TU BIBLIOTECA:\n[1] (fuente: manual.pdf) Pasaje sobre garantía.";
        assert_eq!(compact_grounding(blob), blob);
    }
}
