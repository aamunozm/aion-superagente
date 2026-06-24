//! Markdown → HTML con `pulldown-cmark` (CommonMark + extensiones GFM).
//!
//! Habilitamos tablas, tachado, listas de tareas y notas al pie: justo lo que un
//! preventivo/informe necesita. El HTML resultante es el *cuerpo*; la plantilla branded
//! ([`crate::template`]) lo envuelve con la cabecera, el CSS de marca y el pie.

use pulldown_cmark::{html, Options, Parser};

/// Convierte un cuerpo Markdown en una cadena HTML (sin `<html>`/`<body>`: solo el contenido).
pub fn to_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(markdown, opts);
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, parser);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_and_tables() {
        let md = "# Título\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
        let html = to_html(md);
        assert!(html.contains("<h1>"), "debe haber encabezado: {html}");
        assert!(
            html.contains("<table>"),
            "debe renderizar tablas GFM: {html}"
        );
        assert!(html.contains("<td>1</td>"));
    }

    #[test]
    fn renders_lists_and_emphasis() {
        let html = to_html("- uno\n- **dos**\n");
        assert!(html.contains("<ul>"));
        assert!(html.contains("<strong>dos</strong>"));
    }
}
