//! # aion-docgen
//!
//! **Generación de documentos branded, 100% local.** Convierte un cuerpo Markdown en un
//! documento profesional con la marca de AION (o la del usuario) en tres salidas:
//! HTML, PDF (vía el Chromium headless de `aion-browser`) y DOCX editable (`docx-rs`).
//!
//! Pipeline: `Markdown → HTML (pulldown-cmark) → plantilla branded (minijinja, design tokens)
//! → PDF (CDP printToPDF)`. El render NO usa el LLM: es determinista y de coste cero.
//!
//! ```no_run
//! # async fn demo() -> Result<(), String> {
//! use aion_docgen::{DocRequest, PdfOptions, render_pdf};
//! let req = DocRequest::new("base", "Informe", "# Hola\n\nContenido **real**.");
//! let pdf: Vec<u8> = render_pdf(&req, &PdfOptions::default()).await?;
//! # Ok(()) }
//! ```

pub mod brand;
mod docx;
mod markdown;
pub mod offerta;
pub mod style;
pub mod style_extract;
mod template;

pub use aion_browser::PdfOptions;
pub use brand::BrandProfile;
pub use offerta::{
    build_offerta, render_offerta_html, render_offerta_pdf, Benefit, Card, CompareBar, Condition,
    OfferRow, OffertaContent, OffertaFacts,
};
pub use style::{presets as style_presets, DocStyle};
pub use style_extract::{extract_style, Extracted};

use serde::Serialize;

/// Datos del destinatario (preventivos/propuestas). Todos los campos son opcionales salvo
/// el nombre.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ClientInfo {
    pub name: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub address: String,
}

/// Metadatos del documento (lo que rodea al cuerpo: fecha, número, cliente…).
#[derive(Debug, Clone, Serialize, Default)]
pub struct DocMeta {
    pub subtitle: Option<String>,
    /// Fecha ya formateada para el idioma de salida (p. ej. "23 giugno 2026").
    pub date: String,
    /// Número correlativo (p. ej. "PREV-2026-031").
    pub number: Option<String>,
    pub client: Option<ClientInfo>,
}

/// Formato de salida solicitado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Pdf,
    Html,
    Docx,
    /// Markdown crudo (passthrough, sin plantilla).
    Markdown,
}

impl DocFormat {
    /// Resuelve el formato a partir de un sufijo/alias textual (entrada del agente/usuario).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "pdf" => Some(Self::Pdf),
            "html" | "htm" => Some(Self::Html),
            "docx" | "word" | "doc" => Some(Self::Docx),
            "md" | "markdown" | "txt" => Some(Self::Markdown),
            _ => None,
        }
    }

    /// Extensión de archivo canónica.
    pub fn ext(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Html => "html",
            Self::Docx => "docx",
            Self::Markdown => "md",
        }
    }
}

/// Petición de documento: plantilla + título + cuerpo Markdown + marca + metadatos.
#[derive(Debug, Clone)]
pub struct DocRequest {
    /// Nombre de plantilla (`"base"`, `"preventivo"`…). Ver [`available_templates`].
    pub template: String,
    pub title: String,
    pub body_markdown: String,
    pub brand: BrandProfile,
    pub meta: DocMeta,
}

impl DocRequest {
    /// Crea una petición con marca AION por defecto y metadatos vacíos.
    pub fn new(
        template: impl Into<String>,
        title: impl Into<String>,
        body_markdown: impl Into<String>,
    ) -> Self {
        Self {
            template: template.into(),
            title: title.into(),
            body_markdown: body_markdown.into(),
            brand: BrandProfile::default(),
            meta: DocMeta::default(),
        }
    }
}

/// Contexto que se inyecta en la plantilla (lo que ve minijinja).
#[derive(Serialize)]
struct RenderCtx<'a> {
    brand: &'a BrandProfile,
    lang: &'a str,
    title: &'a str,
    subtitle: Option<&'a str>,
    date: &'a str,
    number: Option<&'a str>,
    client: Option<&'a ClientInfo>,
    /// Cuerpo ya convertido a HTML (se inyecta con `|safe`).
    body: String,
}

/// Renderiza el documento a **HTML** branded (sin lanzar el navegador). Útil para previsualizar
/// o para pasarlo a [`render_pdf`].
pub fn render_html(req: &DocRequest) -> Result<String, String> {
    if !template::AVAILABLE.contains(&req.template.as_str()) {
        return Err(format!(
            "plantilla «{}» desconocida (disponibles: {})",
            req.template,
            template::AVAILABLE.join(", ")
        ));
    }
    let body = markdown::to_html(&req.body_markdown);
    let ctx = RenderCtx {
        brand: &req.brand,
        lang: &req.brand.lang,
        title: &req.title,
        subtitle: req.meta.subtitle.as_deref(),
        date: &req.meta.date,
        number: req.meta.number.as_deref(),
        client: req.meta.client.as_ref(),
        body,
    };
    template::render(&req.template, &ctx)
}

/// Renderiza el documento a **PDF** branded usando el Chromium headless local.
pub async fn render_pdf(req: &DocRequest, opts: &PdfOptions) -> Result<Vec<u8>, String> {
    let html = render_html(req)?;
    aion_browser::html_to_pdf(&html, opts)
        .await
        .map_err(|e| e.to_string())
}

/// Renderiza el documento a **DOCX** editable (Word/Pages).
pub fn render_docx(req: &DocRequest) -> Result<Vec<u8>, String> {
    docx::render(
        &req.title,
        &req.brand.company,
        &req.brand.accent,
        &req.body_markdown,
    )
}

/// Lista de plantillas disponibles.
pub fn available_templates() -> &'static [&'static str] {
    template::AVAILABLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_includes_brand_and_body() {
        let mut req = DocRequest::new("base", "Mi Informe", "# Sección\n\nTexto **fuerte**.");
        req.brand.company = "ProntoClick".into();
        req.brand.accent = "#0FB5BA".into();
        let html = render_html(&req).expect("render html");
        assert!(html.contains("ProntoClick"), "marca presente");
        assert!(html.contains("Mi Informe"), "título presente");
        assert!(
            html.contains("<strong>fuerte</strong>"),
            "markdown convertido"
        );
        assert!(html.contains("#0FB5BA"), "acento de marca aplicado");
        assert!(
            html.to_lowercase().contains("<!doctype html>"),
            "documento completo"
        );
    }

    #[test]
    fn preventivo_renders_client_and_signature() {
        let mut req = DocRequest::new(
            "preventivo",
            "Preventivo",
            "| A | B |\n|---|---|\n| 1 | 2 |",
        );
        req.meta.client = Some(ClientInfo {
            name: "Mario Rossi".into(),
            company: "Acme S.r.l.".into(),
            email: "mario@acme.it".into(),
            address: "Via Roma 1".into(),
        });
        req.meta.number = Some("PREV-2026-031".into());
        let html = render_html(&req).expect("render preventivo");
        assert!(html.contains("Acme S.r.l."), "cliente presente");
        assert!(html.contains("PREV-2026-031"), "número presente");
        assert!(html.contains("Firma per accettazione"), "bloque de firma");
        assert!(html.contains("<table>"), "tabla GFM");
    }

    #[test]
    fn unknown_template_is_rejected() {
        let req = DocRequest::new("inexistente", "X", "y");
        assert!(render_html(&req).is_err());
    }

    #[test]
    fn docx_bytes_are_a_zip() {
        let req = DocRequest::new("base", "Doc", "# Hola\n\n- uno\n- dos\n");
        let bytes = render_docx(&req).expect("docx");
        // Un .docx es un ZIP: empieza por "PK".
        assert!(
            bytes.starts_with(b"PK"),
            "debe ser un contenedor OOXML (ZIP)"
        );
        assert!(bytes.len() > 200);
    }

    #[test]
    fn docformat_parsing() {
        assert_eq!(DocFormat::parse("PDF"), Some(DocFormat::Pdf));
        assert_eq!(DocFormat::parse("word"), Some(DocFormat::Docx));
        assert_eq!(DocFormat::parse("zzz"), None);
        assert_eq!(DocFormat::Pdf.ext(), "pdf");
    }
}
