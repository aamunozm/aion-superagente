//! Extracción ligera de texto legible desde HTML (sin dependencias pesadas).

/// Convierte HTML en texto plano: elimina script/style, quita etiquetas,
/// decodifica entidades comunes y colapsa espacios.
pub fn to_text(html: &str) -> String {
    let without_blocks = remove_block(&remove_block(html, "script"), "style");
    let stripped = strip_tags(&without_blocks);
    let decoded = decode_entities(&stripped);
    collapse_ws(&decoded)
}

/// Elimina `<tag ...>...</tag>` (case-insensitive) de forma repetida.
fn remove_block(input: &str, tag: &str) -> String {
    let mut s = input.to_string();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    loop {
        let lower = s.to_lowercase();
        let Some(start) = lower.find(&open) else {
            break;
        };
        let after = match lower[start..].find(&close) {
            Some(rel) => start + rel + close.len(),
            None => s.len(), // sin cierre: corta hasta el final
        };
        s.replace_range(start..after, " ");
    }
    s
}

/// Quita todas las etiquetas `<...>`.
fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Decodifica un conjunto mínimo de entidades HTML.
fn decode_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// Colapsa espacios/saltos repetidos en uno solo y recorta.
fn collapse_ws(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::to_text;

    #[test]
    fn extracts_readable_text() {
        let html = "<html><head><style>.x{color:red}</style>\
            <script>alert('hi')</script></head><body><h1>Hola &amp; chau</h1>\
            <p>Mundo   real</p></body></html>";
        let text = to_text(html);
        assert!(text.contains("Hola & chau"));
        assert!(text.contains("Mundo real"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color:red"));
    }
}
