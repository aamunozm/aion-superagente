//! Markdown → DOCX editable (Word/Pages) con `docx-rs`, sin depender de `textutil` ni de
//! herramientas del SO. Para entregables que el cliente quiere editar.
//!
//! Cobertura pragmática: título de marca, encabezados, párrafos con **negrita**/*cursiva*/
//! `código`, listas (viñeta/numeradas) y tablas GFM. Suficiente para preventivos e informes.

use docx_rs::*;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use std::sync::atomic::{AtomicU64, Ordering};

fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Un fragmento de texto con su formato inline.
#[derive(Clone, Default)]
struct Span {
    text: String,
    bold: bool,
    italic: bool,
    code: bool,
}

/// Bloques de alto nivel que volcamos al documento en orden.
enum Block {
    Para(Paragraph),
    Table(Table),
}

/// Convierte el cuerpo Markdown en un DOCX y devuelve los bytes (.docx).
pub fn render(
    title: &str,
    brand_company: &str,
    accent_hex: &str,
    markdown: &str,
) -> Result<Vec<u8>, String> {
    let mut blocks: Vec<Block> = Vec::new();

    // Cabecera de marca.
    blocks.push(Block::Para(
        Paragraph::new().add_run(Run::new().add_text(title).bold().size(40)),
    ));
    let accent = accent_hex.trim_start_matches('#').to_string();
    blocks.push(Block::Para(
        Paragraph::new().add_run(Run::new().add_text(brand_company).size(20).color(accent)),
    ));
    blocks.push(Block::Para(Paragraph::new()));

    // ── Estado del parser ──────────────────────────────────────────────
    let mut spans: Vec<Span> = Vec::new();
    let mut bold = 0u32;
    let mut italic = 0u32;
    let mut code = false;
    let mut heading: Option<HeadingLevel> = None;
    // Listas: cada nivel lleva su contador (None = viñeta).
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    // Tablas.
    let mut in_table = false;
    let mut in_head = false;
    let mut rows: Vec<TableRow> = Vec::new();
    let mut cur_cells: Vec<TableCell> = Vec::new();
    let mut cell_text = String::new();

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);

    let flush_para = |spans: &mut Vec<Span>,
                      heading: Option<HeadingLevel>,
                      prefix: Option<String>|
     -> Option<Paragraph> {
        if spans.is_empty() && prefix.is_none() {
            return None;
        }
        let mut p = Paragraph::new();
        if let Some(px) = prefix {
            p = p.add_run(Run::new().add_text(px));
        }
        let h_size = heading.map(heading_size);
        for s in spans.drain(..) {
            if s.text.is_empty() {
                continue;
            }
            let mut r = Run::new().add_text(s.text);
            if s.bold || heading.is_some() {
                r = r.bold();
            }
            if s.italic {
                r = r.italic();
            }
            if let Some(sz) = h_size {
                r = r.size(sz);
            }
            if s.code {
                r = r.fonts(RunFonts::new().ascii("Courier New"));
            }
            p = p.add_run(r);
        }
        Some(p)
    };

    for ev in Parser::new_ext(markdown, opts) {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => heading = Some(level),
            Event::End(TagEnd::Heading(_)) => {
                if let Some(p) = flush_para(&mut spans, heading, None) {
                    blocks.push(Block::Para(p));
                }
                heading = None;
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                if !in_table {
                    if let Some(p) = flush_para(&mut spans, None, None) {
                        blocks.push(Block::Para(p));
                    }
                }
            }
            Event::Start(Tag::List(start)) => list_stack.push(start),
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {}
            Event::End(TagEnd::Item) => {
                let depth = list_stack.len().saturating_sub(1);
                let indent = "    ".repeat(depth);
                let prefix = match list_stack.last_mut() {
                    Some(Some(n)) => {
                        let cur = *n;
                        *n += 1;
                        format!("{indent}{cur}. ")
                    }
                    _ => format!("{indent}•  "),
                };
                if let Some(p) = flush_para(&mut spans, None, Some(prefix)) {
                    blocks.push(Block::Para(p));
                }
            }
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic = italic.saturating_sub(1),
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::Start(Tag::BlockQuote(_)) => italic += 1,
            Event::End(TagEnd::BlockQuote(_)) => italic = italic.saturating_sub(1),
            Event::Start(Tag::CodeBlock(_)) => code = true,
            Event::End(TagEnd::CodeBlock) => {
                code = false;
                if let Some(p) = flush_para(&mut spans, None, None) {
                    blocks.push(Block::Para(p));
                }
            }
            // ── Tablas ──
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                rows.clear();
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
                if !rows.is_empty() {
                    blocks.push(Block::Table(Table::new(std::mem::take(&mut rows))));
                }
            }
            Event::Start(Tag::TableHead) => in_head = true,
            Event::End(TagEnd::TableHead) => in_head = false,
            Event::Start(Tag::TableRow) => cur_cells.clear(),
            Event::End(TagEnd::TableRow) => {
                rows.push(TableRow::new(std::mem::take(&mut cur_cells)));
            }
            Event::Start(Tag::TableCell) => cell_text.clear(),
            Event::End(TagEnd::TableCell) => {
                let mut run = Run::new().add_text(cell_text.trim());
                if in_head {
                    run = run.bold();
                }
                cur_cells.push(TableCell::new().add_paragraph(Paragraph::new().add_run(run)));
            }
            Event::Text(t) => {
                if in_table {
                    cell_text.push_str(&t);
                } else {
                    spans.push(Span {
                        text: t.to_string(),
                        bold: bold > 0,
                        italic: italic > 0,
                        code,
                    });
                }
            }
            Event::Code(t) => {
                if in_table {
                    cell_text.push_str(&t);
                } else {
                    spans.push(Span {
                        text: t.to_string(),
                        bold: bold > 0,
                        italic: italic > 0,
                        code: true,
                    });
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_table {
                    cell_text.push(' ');
                } else {
                    spans.push(Span {
                        text: " ".into(),
                        ..Default::default()
                    });
                }
            }
            _ => {}
        }
    }

    // Construye el documento y empaqueta a bytes (vía archivo temporal: pack necesita Write+Seek).
    let mut docx = Docx::new();
    for b in blocks {
        docx = match b {
            Block::Para(p) => docx.add_paragraph(p),
            Block::Table(t) => docx.add_table(t),
        };
    }

    let tmp = std::env::temp_dir().join(format!(
        "aion-docx-{}-{}.docx",
        std::process::id(),
        next_id()
    ));
    let file =
        std::fs::File::create(&tmp).map_err(|e| format!("no pude crear el DOCX temporal: {e}"))?;
    docx.build()
        .pack(file)
        .map_err(|e| format!("no pude empaquetar el DOCX: {e}"))?;
    let bytes = std::fs::read(&tmp).map_err(|e| format!("no pude leer el DOCX: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(bytes)
}

fn heading_size(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 36,
        HeadingLevel::H2 => 30,
        HeadingLevel::H3 => 26,
        _ => 24,
    }
}
