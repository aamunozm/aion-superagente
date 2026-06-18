//! Extracción ligera de texto legible desde HTML (sin dependencias pesadas).

/// Convierte HTML en texto plano: elimina bloques que NO son contenido legible, quita etiquetas,
/// decodifica entidades comunes y colapsa espacios.
///
/// Además de `script`/`style`, descarta el "chrome" de la página —menús (`nav`), cabeceras
/// (`header`), pies (`footer`), barras laterales (`aside`), formularios y `svg`/`noscript`/
/// `template`—. Sin esto, el texto extraído empezaba con decenas de enlaces de menú y, como el
/// lector de investigación solo manda los primeros miles de caracteres al LLM, el contenido real
/// del artículo se quedaba fuera (el LLM resumía el menú, no la noticia/paper).
pub fn to_text(html: &str) -> String {
    const DROP: &[&str] = &[
        "script", "style", "noscript", "template", "svg", "nav", "header", "footer", "aside",
        "form",
    ];
    let mut cleaned = html.to_string();
    for tag in DROP {
        cleaned = remove_block(&cleaned, tag);
    }
    let stripped = strip_tags(&cleaned);
    let decoded = decode_entities(&stripped);
    collapse_ws(&decoded)
}

/// Elimina `<tag ...>...</tag>` (case-insensitive) de forma repetida.
fn remove_block(input: &str, tag: &str) -> String {
    let mut s = input.to_string();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    loop {
        // to_ASCII_lowercase, NO to_lowercase: los nombres de etiqueta son ASCII, y el
        // lowercase Unicode puede CAMBIAR la longitud en bytes (ß→ss, İ→i̇, Σ griego),
        // desincronizando los índices de `lower` respecto a `s` → `replace_range` con
        // índices ajenos = panic o recorte corrupto. El ASCII-lowercase preserva cada
        // byte, así que los índices hallados en `lower` son válidos en `s`.
        let lower = s.to_ascii_lowercase();
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
    }

    #[test]
    fn no_panic_with_length_changing_unicode_before_block() {
        // Con to_lowercase() Unicode, 'İ'/'ß' cambian de longitud en bytes y
        // desincronizaban los índices → panic en replace_range. Debe quedar limpio.
        let html = "İß<script>alert('x')</script>café ñoño";
        let text = to_text(html);
        assert!(!text.contains("alert"));
        assert!(text.contains("café"));
        assert!(text.contains("ñoño"));
        // Etiqueta en MAYÚSCULAS también se elimina (case-insensitive ASCII).
        let upper = to_text("a<SCRIPT>mal</SCRIPT>b");
        assert!(!upper.contains("mal"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color:red"));
    }

    #[test]
    fn drops_page_chrome_keeps_article() {
        // El menú/cabecera/pie no deben colarse antes del contenido real (era lo que hacía que el
        // LLM resumiera el menú en vez del artículo).
        let html = "<body><nav><a>Inicio</a><a>Economía</a><a>Empresas</a></nav>            <header>Mi Diario</header><article><h1>Titular real</h1>            <p>Cuerpo del artículo con datos.</p></article>            <footer>Copyright 2026 · Aviso legal</footer></body>";
        let text = to_text(html);
        assert!(text.contains("Titular real"));
        assert!(text.contains("Cuerpo del artículo con datos."));
        assert!(!text.contains("Economía"), "el menú no debe aparecer");
        assert!(!text.contains("Copyright"), "el pie no debe aparecer");
    }
}
