//! **Skill experta: AUDITORÍA SEO.** On-demand (no corre sola): es un entregable profesional.
//!
//! Lee el **HTML crudo** (`fetch_raw` → title/meta/canonical/og/headings/JSON-LD…) Y **renderiza
//! con el navegador headless** (`fetch_rendered`) para los sitios JS/SPA que el fetch estático no
//! ve. Puntúa cada dimensión técnica de SEO y compone un informe en Markdown que aion-docgen
//! convierte en un PDF con la marca. El agente lo dispara cuando Ariel pide una auditoría.

use aion_browser::WebClient;

/// Un control SEO con su veredicto. `pts`/`max` alimentan la puntuación global.
struct Check {
    label: String,
    icon: &'static str, // ✓ / ⚠ / ✗
    detail: String,
    pts: u32,
    max: u32,
}

pub struct SeoReport {
    pub url: String,
    pub score: u32,
    pub title: String,
    pub markdown: String,
}

// ── Parseo ligero de HTML (sin dependencias nuevas) ─────────────────────────────

/// Devuelve cada etiqueta «<tag …>» (su texto entre `<` y el primer `>`).
fn tags<'a>(html: &'a str, tag: &str) -> Vec<&'a str> {
    let needle = format!("<{}", tag.to_lowercase());
    let low = html.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = low[from..].find(&needle) {
        let s = from + i;
        // que sea «<meta » o «<meta>» y no «<metadata»
        let after = low[s + needle.len()..].chars().next().unwrap_or(' ');
        if after == ' ' || after == '>' || after == '/' || after == '\n' || after == '\t' {
            if let Some(e) = html[s..].find('>') {
                out.push(&html[s..s + e + 1]);
                from = s + e + 1;
                continue;
            }
        }
        from = s + needle.len();
    }
    out
}

/// Extrae `attr="valor"` (o `attr='valor'`) de una etiqueta, case-insensitive en el nombre.
fn attr(tag: &str, name: &str) -> Option<String> {
    let low = tag.to_lowercase();
    let key = format!("{}=", name.to_lowercase());
    let i = low.find(&key)? + key.len();
    let rest = &tag[i..];
    let q = rest.chars().next()?;
    if q == '"' || q == '\'' {
        let end = rest[1..].find(q)?;
        Some(rest[1..1 + end].trim().to_string())
    } else {
        // sin comillas: hasta espacio o >
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

/// Contenido del primer `<meta>` cuyo `key_attr` valga `key_val` (p. ej. name=description).
fn meta(html_tags: &[&str], key_attr: &str, key_val: &str) -> Option<String> {
    for t in html_tags {
        if attr(t, key_attr).map(|v| v.eq_ignore_ascii_case(key_val)) == Some(true) {
            if let Some(c) = attr(t, "content") {
                if !c.trim().is_empty() {
                    return Some(c.trim().to_string());
                }
            }
        }
    }
    None
}

fn count(html: &str, tag: &str) -> usize {
    tags(html, tag).len()
}

/// Decodifica las entidades HTML más comunes (numéricas `&#8211;` y nombradas `&amp;`) para que el
/// texto del sitio se lea natural en el informe, no como «&#8211;».
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '&' {
            if let Some(semi) = bytes[i..].iter().position(|&c| c == ';') {
                let ent: String = bytes[i + 1..i + semi].iter().collect();
                let rep = match ent.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" | "#39" | "#x27" => Some('\''),
                    "nbsp" => Some(' '),
                    "copy" => Some('©'),
                    "reg" => Some('®'),
                    "#8211" | "#x2013" => Some('–'),
                    "#8212" | "#x2014" => Some('—'),
                    "#8217" | "#x2019" => Some('\u{2019}'),
                    "#8216" | "#x2018" => Some('\u{2018}'),
                    "#8220" | "#x201C" => Some('\u{201C}'),
                    "#8221" | "#x201D" => Some('\u{201D}'),
                    _ => ent
                        .strip_prefix('#')
                        .and_then(|n| n.parse::<u32>().ok())
                        .and_then(char::from_u32),
                };
                if let Some(c) = rep {
                    out.push(c);
                    i += semi + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn first_title(html: &str) -> String {
    let low = html.to_lowercase();
    if let Some(a) = low.find("<title") {
        if let Some(s) = html[a..].find('>') {
            let from = a + s + 1;
            if let Some(e) = html[from..].to_lowercase().find("</title>") {
                return decode_entities(html[from..from + e].trim());
            }
        }
    }
    String::new()
}

fn ck(label: &str, icon: &'static str, detail: String, pts: u32, max: u32) -> Check {
    Check {
        label: label.into(),
        icon,
        detail,
        pts,
        max,
    }
}

/// Ejecuta la auditoría SEO de una URL (HTML crudo + render headless + puntuación + informe).
pub async fn audit(url: &str) -> Result<SeoReport, String> {
    let url = if url.starts_with("http") {
        url.trim().to_string()
    } else {
        format!("https://{}", url.trim())
    };
    let web = WebClient::new();
    let html = web
        .fetch_raw(&url)
        .await
        .map_err(|e| format!("no pude descargar la página: {e}"))?;
    // Render headless (para sitios JS): si falla, seguimos solo con el HTML crudo.
    let rendered = web.fetch_rendered(&url, 30000).await.unwrap_or_default();
    let words = rendered.split_whitespace().filter(|w| w.len() > 1).count();

    let metas = tags(&html, "meta");
    let links = tags(&html, "link");
    let title = first_title(&html);
    let desc = meta(&metas, "name", "description").unwrap_or_default();
    let viewport = meta(&metas, "name", "viewport").unwrap_or_default();
    let robots = meta(&metas, "name", "robots").unwrap_or_default();
    let og_title = meta(&metas, "property", "og:title").unwrap_or_default();
    let og_desc = meta(&metas, "property", "og:description").unwrap_or_default();
    let canonical = links
        .iter()
        .find(|t| attr(t, "rel").map(|r| r.eq_ignore_ascii_case("canonical")) == Some(true))
        .and_then(|t| attr(t, "href"))
        .unwrap_or_default();
    let lang = {
        let low = html.to_lowercase();
        low.find("<html")
            .and_then(|i| html[i..].find('>').map(|e| &html[i..i + e + 1]))
            .and_then(|t| attr(t, "lang"))
            .unwrap_or_default()
    };
    let jsonld = html.to_lowercase().contains("application/ld+json");
    let h1 = count(&html, "h1");
    let h2 = count(&html, "h2");
    let h3 = count(&html, "h3");
    let imgs = tags(&html, "img");
    let imgs_no_alt = imgs
        .iter()
        .filter(|t| attr(t, "alt").map(|a| a.trim().is_empty()).unwrap_or(true))
        .count();
    let https = url.starts_with("https://");

    let mut checks: Vec<Check> = Vec::new();
    // HTTPS
    checks.push(if https {
        ck("HTTPS", "✓", "el sitio usa conexión segura.".into(), 8, 8)
    } else {
        ck(
            "HTTPS",
            "✗",
            "el sitio NO usa HTTPS (penaliza ranking y confianza).".into(),
            0,
            8,
        )
    });
    // Title
    let tl = title.chars().count();
    checks.push(if title.is_empty() {
        ck(
            "Title",
            "✗",
            "falta la etiqueta «title» (crítico).".into(),
            0,
            16,
        )
    } else if (30..=65).contains(&tl) {
        ck(
            "Title",
            "✓",
            format!("«{title}» ({tl} car., longitud óptima)."),
            16,
            16,
        )
    } else {
        ck(
            "Title",
            "⚠",
            format!("«{title}» ({tl} car.; ideal 30–65)."),
            8,
            16,
        )
    });
    // Meta description
    let dl = desc.chars().count();
    checks.push(if desc.is_empty() {
        ck(
            "Meta description",
            "✗",
            "falta la meta description (Google muestra texto aleatorio).".into(),
            0,
            16,
        )
    } else if (110..=165).contains(&dl) {
        ck(
            "Meta description",
            "✓",
            format!("presente y bien dimensionada ({dl} car.)."),
            16,
            16,
        )
    } else {
        ck(
            "Meta description",
            "⚠",
            format!("presente pero {dl} car. (ideal 110–165)."),
            8,
            16,
        )
    });
    // H1
    checks.push(match h1 {
        1 => ck(
            "Encabezado H1",
            "✓",
            "exactamente un «H1» en el código.".into(),
            10,
            10,
        ),
        0 => ck(
            "Encabezado H1",
            "⚠",
            "ningún «H1» en el HTML de origen (si es JS, los buscadores pueden no verlo).".into(),
            3,
            10,
        ),
        n => ck(
            "Encabezado H1",
            "⚠",
            format!("{n} «H1» (debería haber 1)."),
            5,
            10,
        ),
    });
    // Estructura de encabezados
    checks.push(if h2 + h3 > 0 {
        ck(
            "Estructura (H2/H3)",
            "✓",
            format!("{h2} H2 y {h3} H3 dan jerarquía de contenido."),
            6,
            6,
        )
    } else {
        ck(
            "Estructura (H2/H3)",
            "⚠",
            "sin H2/H3 en el código (contenido plano o renderizado por JS).".into(),
            2,
            6,
        )
    });
    // Viewport / móvil
    checks.push(if !viewport.is_empty() {
        ck(
            "Móvil (viewport)",
            "✓",
            "declara viewport (responsive).".into(),
            10,
            10,
        )
    } else {
        ck(
            "Móvil (viewport)",
            "✗",
            "falta «meta viewport» (mala experiencia móvil).".into(),
            0,
            10,
        )
    });
    // Idioma
    checks.push(if !lang.is_empty() {
        ck(
            "Idioma (lang)",
            "✓",
            format!("declara lang=\"{lang}\"."),
            5,
            5,
        )
    } else {
        ck(
            "Idioma (lang)",
            "⚠",
            "el «html» no declara idioma.".into(),
            0,
            5,
        )
    });
    // Canonical
    checks.push(if !canonical.is_empty() {
        ck(
            "URL canónica",
            "✓",
            "define rel=canonical (evita contenido duplicado).".into(),
            5,
            5,
        )
    } else {
        ck("URL canónica", "⚠", "sin etiqueta canonical.".into(), 0, 5)
    });
    // Open Graph
    checks.push(if !og_title.is_empty() || !og_desc.is_empty() {
        ck(
            "Open Graph (redes)",
            "✓",
            "tiene etiquetas og: (buena vista al compartir).".into(),
            5,
            5,
        )
    } else {
        ck(
            "Open Graph (redes)",
            "⚠",
            "sin Open Graph: al compartir en redes se ve pobre.".into(),
            0,
            5,
        )
    });
    // Datos estructurados
    checks.push(if jsonld {
        ck(
            "Datos estructurados",
            "✓",
            "incluye JSON-LD (Schema.org).".into(),
            5,
            5,
        )
    } else {
        ck(
            "Datos estructurados",
            "⚠",
            "sin datos estructurados (JSON-LD) para rich results.".into(),
            0,
            5,
        )
    });
    // Profundidad de contenido (renderizado)
    checks.push(if words >= 300 {
        ck(
            "Contenido",
            "✓",
            format!("~{words} palabras tras renderizar (suficiente)."),
            10,
            10,
        )
    } else if words >= 80 {
        ck(
            "Contenido",
            "⚠",
            format!("~{words} palabras (algo escaso para posicionar)."),
            5,
            10,
        )
    } else {
        ck(
            "Contenido",
            "✗",
            format!("~{words} palabras visibles (muy poco contenido indexable)."),
            0,
            10,
        )
    });
    // Imágenes con alt
    checks.push(if imgs.is_empty() {
        ck(
            "Alt de imágenes",
            "⚠",
            "no se detectaron imágenes en el HTML de origen.".into(),
            3,
            5,
        )
    } else if imgs_no_alt == 0 {
        ck(
            "Alt de imágenes",
            "✓",
            format!("las {} imágenes tienen alt.", imgs.len()),
            5,
            5,
        )
    } else {
        ck(
            "Alt de imágenes",
            "⚠",
            format!(
                "{imgs_no_alt}/{} imágenes sin alt (accesibilidad/SEO).",
                imgs.len()
            ),
            2,
            5,
        )
    });

    let got: u32 = checks.iter().map(|c| c.pts).sum();
    let total: u32 = checks.iter().map(|c| c.max).sum();
    let score = if total > 0 { got * 100 / total } else { 0 };

    // ── Informe Markdown (→ PDF con la marca) ──
    let veredicto = match score {
        85..=100 => "Excelente — base SEO sólida.",
        70..=84 => "Buena, con mejoras claras.",
        50..=69 => "Mejorable: hay varias carencias importantes.",
        _ => "Deficiente: faltan fundamentos de SEO.",
    };
    let mut md = String::new();
    md.push_str(&format!(
        "## Resumen ejecutivo\n\n**Puntuación SEO: {score}/100** — {veredicto}\n\nSitio analizado: {url}\n\n"
    ));
    // Solo escapamos `|` (separador de tabla). Los textos de detalle NO usan `<>` a propósito
    // (se nombran las etiquetas en texto plano) para no pelear con el render de Markdown.
    let esc = |s: &str| s.replace('|', "\\|");
    md.push_str("## Resultados\n\n| | Control | Detalle |\n|---|---|---|\n");
    for c in &checks {
        md.push_str(&format!(
            "| {} | **{}** | {} |\n",
            c.icon,
            c.label,
            esc(&c.detail)
        ));
    }
    // Acciones prioritarias = los controles fallidos/parciales.
    let mut acc: Vec<&Check> = checks.iter().filter(|c| c.pts < c.max).collect();
    acc.sort_by_key(|c| c.pts); // primero los más graves
    if !acc.is_empty() {
        md.push_str("\n## Acciones prioritarias\n\n");
        for (i, c) in acc.iter().take(8).enumerate() {
            md.push_str(&format!(
                "{}. **{}** — {}\n",
                i + 1,
                c.label,
                esc(&c.detail)
            ));
        }
    }
    md.push_str(
        "\n*Auditoría técnica on-page generada localmente por AION (HTML de origen + render headless). \
         No incluye factores off-page (backlinks) ni rendimiento de servidor.*\n",
    );

    Ok(SeoReport {
        url,
        score,
        title,
        markdown: md,
    })
}

/// Audita la URL, compone el PDF con la marca (estilo predeterminado) y lo guarda+abre en el
/// Escritorio. Devuelve `(ruta, score, url_final)`. Lo usan el fast-path del agente y el tool.
pub async fn audit_to_desktop(url: &str) -> Result<(String, u32, String), String> {
    let report = audit(url).await?;
    let host: String = report
        .url
        .replace("https://", "")
        .replace("http://", "")
        .trim_end_matches('/')
        .to_string();
    let safe: String = host
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let title = format!(
        "Auditoría SEO — {}",
        host.chars().take(60).collect::<String>()
    );
    let mut brand = aion_docgen::BrandProfile::load(crate::agent_tools::brand_profile_path());
    let st = crate::serve::resolve_default_style();
    brand.ink = st.ink;
    brand.accent = st.accent;
    let mut req = aion_docgen::DocRequest::new("base", &title, &report.markdown);
    req.meta.date = crate::agent_tools::human_date(&brand.lang);
    req.meta.subtitle = Some(format!("Puntuación {}/100", report.score));
    req.brand = brand;
    let bytes = aion_docgen::render_pdf(&req, &aion_docgen::PdfOptions::default()).await?;
    let home =
        std::env::var("HOME").map_err(|_| "no encuentro tu carpeta de usuario".to_string())?;
    let path = std::path::Path::new(&home).join("Desktop").join(format!(
        "Auditoria SEO {}.pdf",
        safe.chars().take(50).collect::<String>()
    ));
    std::fs::write(&path, &bytes).map_err(|e| format!("no pude escribir el informe: {e}"))?;
    crate::agent_tools::open_file(&path, false);
    Ok((path.display().to_string(), report.score, report.url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_title_meta_y_cuenta_encabezados() {
        let html = r#"<html lang="it"><head>«title»Hola Mundo</title>
            <meta name="description" content="una descripción"/>
            <meta property="og:title" content="OG"/>
            <link rel="canonical" href="https://x.it/"/></head>
            <body>«H1»A</h1><h2>B</h2><img src="a.jpg"/><img src="b.jpg" alt="ok"/></body></html>"#;
        assert_eq!(first_title(html), "Hola Mundo");
        let metas = tags(html, "meta");
        assert_eq!(
            meta(&metas, "name", "description").as_deref(),
            Some("una descripción")
        );
        assert_eq!(meta(&metas, "property", "og:title").as_deref(), Some("OG"));
        assert_eq!(count(html, "h1"), 1);
        assert_eq!(count(html, "h2"), 1);
        assert_eq!(tags(html, "img").len(), 2);
    }
}
