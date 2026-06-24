//! **Extracción de ESTILO de un documento de referencia.**
//!
//! "Sube tu Offerta y AION te saca su estilo." Dos señales:
//! - **Paleta dominante**: k-means de color sobre una imagen renderizada del documento.
//! - **Fuentes**: escaneo de los `/BaseFont` del PDF (sin parser completo).
//!
//! De ahí clasifica `ink` (oscuro), `accent` (el color con más pop), `paper` (fondo) y mapea
//! las fuentes a una pila web-safe → un [`DocStyle`] listo para aplicar o guardar. Todo local.

use crate::style::{DocStyle, MONO, SANS, SERIF};

/// Resultado de la extracción: el estilo + la paleta y fuentes detectadas (para mostrarlas).
#[derive(Debug, Clone)]
pub struct Extracted {
    pub style: DocStyle,
    pub palette: Vec<String>,
    pub fonts: Vec<String>,
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Lightness y saturación (0..1) estilo HSL.
fn light_sat(r: f32, g: f32, b: f32) -> (f32, f32) {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d <= 0.0001 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs()).max(0.0001)
    };
    (l, s.clamp(0.0, 1.0))
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= 0.0001 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(0.0001);
    let h = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    ((h + 360.0) % 360.0, s.clamp(0.0, 1.0), l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

/// Da PUNCH a un color: conserva el TONO pero garantiza saturación y luminosidad usables
/// (el acento extraído suele salir lavado por el anti-aliasing). Lo deja apto para texto/UI.
fn punch(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (h, s, l) = rgb_to_hsl(r, g, b);
    hsl_to_rgb(h, s.max(0.6), l.clamp(0.32, 0.48))
}

/// k-means determinista (init equiespaciada) sobre los píxeles de una imagen reducida.
/// Devuelve `(r,g,b,peso)` por cluster, ordenado por peso descendente.
pub fn dominant_colors(img_bytes: &[u8], k: usize) -> Result<Vec<(u8, u8, u8, f32)>, String> {
    let img = image::load_from_memory(img_bytes)
        .map_err(|e| format!("no pude decodificar la imagen: {e}"))?;
    let small = img.thumbnail(96, 96).to_rgb8();
    let px: Vec<[f32; 3]> = small
        .pixels()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect();
    if px.is_empty() {
        return Err("imagen vacía".into());
    }
    let k = k.clamp(2, 8).min(px.len());
    let mut cent: Vec<[f32; 3]> = (0..k)
        .map(|i| px[i * (px.len() - 1) / (k - 1).max(1)])
        .collect();
    let mut assign = vec![0usize; px.len()];
    for _ in 0..12 {
        for (i, p) in px.iter().enumerate() {
            let mut best = 0;
            let mut bd = f32::MAX;
            for (j, c) in cent.iter().enumerate() {
                let d = (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
                if d < bd {
                    bd = d;
                    best = j;
                }
            }
            assign[i] = best;
        }
        let mut sum = vec![[0f32; 3]; k];
        let mut cnt = vec![0f32; k];
        for (i, p) in px.iter().enumerate() {
            let a = assign[i];
            sum[a][0] += p[0];
            sum[a][1] += p[1];
            sum[a][2] += p[2];
            cnt[a] += 1.0;
        }
        for j in 0..k {
            if cnt[j] > 0.0 {
                cent[j] = [sum[j][0] / cnt[j], sum[j][1] / cnt[j], sum[j][2] / cnt[j]];
            }
        }
    }
    let mut cnt = vec![0f32; k];
    for &a in &assign {
        cnt[a] += 1.0;
    }
    let total = px.len() as f32;
    let mut out: Vec<(u8, u8, u8, f32)> = (0..k)
        .map(|j| {
            (
                cent[j][0].round() as u8,
                cent[j][1].round() as u8,
                cent[j][2].round() as u8,
                cnt[j] / total,
            )
        })
        .collect();
    out.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}

/// **Acento por vivacidad**: media de los píxeles MÁS saturados. Capta el color de acento
/// (dorado, teal…) aunque ocupe poca área —y por eso los k-means dominantes lo pierden—.
/// `None` si el documento es prácticamente monocromo (sin color vivo).
pub fn vivid_accent(img_bytes: &[u8]) -> Option<(u8, u8, u8)> {
    let img = image::load_from_memory(img_bytes).ok()?;
    let small = img.thumbnail(160, 160).to_rgb8();
    let (mut sr, mut sg, mut sb, mut n) = (0f64, 0f64, 0f64, 0u64);
    for p in small.pixels() {
        let (l, s) = light_sat(p[0] as f32, p[1] as f32, p[2] as f32);
        if s >= 0.40 && (0.18..=0.88).contains(&l) {
            sr += p[0] as f64;
            sg += p[1] as f64;
            sb += p[2] as f64;
            n += 1;
        }
    }
    if n < 3 {
        return None;
    }
    Some((
        (sr / n as f64).round() as u8,
        (sg / n as f64).round() as u8,
        (sb / n as f64).round() as u8,
    ))
}

fn is_pdf_delim(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\n' | b'\r' | b'\t' | b'/' | b'(' | b')' | b'<' | b'>' | b'[' | b']'
    )
}
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Familias de fuentes detectadas en un PDF (escaneo de `/BaseFont /ABCDEF+Nombre-Bold`).
/// Quita el prefijo de subset y el sufijo de peso/estilo. Sin parser completo.
pub fn fonts_from_pdf(pdf: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let needle = b"/BaseFont";
    let mut i = 0usize;
    while i < pdf.len() {
        let Some(pos) = find(&pdf[i..], needle) else {
            break;
        };
        let mut j = i + pos + needle.len();
        while j < pdf.len() && matches!(pdf[j], b' ' | b'\n' | b'\r' | b'\t') {
            j += 1;
        }
        if j < pdf.len() && pdf[j] == b'/' {
            j += 1;
            let s = j;
            while j < pdf.len() && !is_pdf_delim(pdf[j]) {
                j += 1;
            }
            if let Ok(name) = std::str::from_utf8(&pdf[s..j]) {
                let no_subset = name.split('+').last().unwrap_or(name);
                let fam = no_subset
                    .split(['-', ','])
                    .next()
                    .unwrap_or(no_subset)
                    .trim()
                    .to_string();
                if !fam.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(&fam)) {
                    out.push(fam);
                }
            }
        }
        i = i + pos + needle.len();
        if out.len() >= 12 {
            break;
        }
    }
    out
}

/// Mapea las fuentes detectadas a pilas web-safe (body, display).
fn map_fonts(names: &[String]) -> (String, String) {
    let lc: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let has = |kw: &str| lc.iter().any(|n| n.contains(kw));
    let serif = has("times")
        || has("georgia")
        || has("garamond")
        || has("serif")
        || has("minion")
        || has("merriweather")
        || has("playfair")
        || has("baskerville");
    let mono = has("mono") || has("courier") || has("consol") || has("menlo");
    let display = if serif {
        SERIF
    } else if mono {
        MONO
    } else {
        SANS
    };
    let body = if serif { SERIF } else { SANS };
    (body.to_string(), display.to_string())
}

fn darken(r: u8, g: u8, b: u8, target_l: f32) -> (u8, u8, u8) {
    let (l, _) = light_sat(r as f32, g as f32, b as f32);
    if l <= target_l || l <= 0.001 {
        return (r, g, b);
    }
    let f = target_l / l;
    (
        (r as f32 * f).round() as u8,
        (g as f32 * f).round() as u8,
        (b as f32 * f).round() as u8,
    )
}

/// Construye un [`DocStyle`] a partir de la paleta y fuentes detectadas.
pub fn build_style(palette: &[(u8, u8, u8, f32)], fonts: &[String], name: &str) -> DocStyle {
    let mut def = DocStyle {
        name: name.to_string(),
        ..DocStyle::default()
    };
    if palette.is_empty() {
        let (b, d) = map_fonts(fonts);
        def.font = b;
        def.font_display = d;
        return def;
    }
    let info: Vec<(u8, u8, u8, f32, f32, f32)> = palette
        .iter()
        .map(|&(r, g, b, w)| {
            let (l, s) = light_sat(r as f32, g as f32, b as f32);
            (r, g, b, w, l, s)
        })
        .collect();

    // paper = el más claro (fondo). ink = el más oscuro con peso real (oscurecido si hace falta).
    let paper = info
        .iter()
        .max_by(|a, b| a.4.partial_cmp(&b.4).unwrap_or(std::cmp::Ordering::Equal))
        .copied()
        .unwrap();
    let ink_src = info
        .iter()
        .filter(|c| c.3 >= 0.03)
        .min_by(|a, b| a.4.partial_cmp(&b.4).unwrap_or(std::cmp::Ordering::Equal))
        .copied()
        .unwrap_or(paper);
    let (ir, ig, ib) = darken(ink_src.0, ink_src.1, ink_src.2, 0.22);

    // accent = el de más saturación que no sea el papel ni la tinta (el "pop"). Si todo es gris,
    // cae a un acento por defecto.
    let accent = info
        .iter()
        .filter(|c| c.5 >= 0.20 && c.4 > 0.12 && c.4 < 0.92)
        .max_by(|a, b| a.5.partial_cmp(&b.5).unwrap_or(std::cmp::Ordering::Equal))
        .copied();

    def.ink = hex(ir, ig, ib);
    def.accent = accent
        .map(|c| hex(c.0, c.1, c.2))
        .unwrap_or_else(|| def.accent.clone());
    def.paper = if paper.4 > 0.9 {
        hex(paper.0, paper.1, paper.2)
    } else {
        "#ffffff".into()
    };
    def.text = hex(
        ir.saturating_add(8),
        ig.saturating_add(8),
        ib.saturating_add(8),
    );
    let (body, display) = map_fonts(fonts);
    def.font = body;
    def.font_display = display;
    def
}

/// Pipeline completo: imagen renderizada del documento (+ PDF opcional para las fuentes) →
/// [`Extracted`] (estilo + paleta + fuentes).
pub fn extract_style(
    image_bytes: &[u8],
    pdf_bytes: Option<&[u8]>,
    name: &str,
) -> Result<Extracted, String> {
    let pal = dominant_colors(image_bytes, 6)?;
    let fonts = pdf_bytes.map(fonts_from_pdf).unwrap_or_default();
    let mut style = build_style(&pal, &fonts, name);
    // El acento de verdad suele ser ESCASO (kickers, barras): lo recuperamos por vivacidad,
    // que los dominantes pierden. Si el doc es monocromo, se queda el de los clusters.
    if let Some((r, g, b)) = vivid_accent(image_bytes) {
        let (pr, pg, pb) = punch(r, g, b);
        style.accent = hex(pr, pg, pb);
    }
    let palette = pal.iter().map(|&(r, g, b, _)| hex(r, g, b)).collect();
    Ok(Extracted {
        style,
        palette,
        fonts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_from_pdf_scans_basefont() {
        let pdf = b"...stuff /BaseFont /ABCDEF+Georgia-Bold more... /BaseFont /Helvetica endobj";
        let f = fonts_from_pdf(pdf);
        assert!(f.iter().any(|x| x == "Georgia"), "detecta Georgia: {f:?}");
        assert!(
            f.iter().any(|x| x == "Helvetica"),
            "detecta Helvetica: {f:?}"
        );
    }

    #[test]
    fn map_fonts_picks_serif_and_mono() {
        let (_, d) = map_fonts(&["Georgia".into()]);
        assert!(d.contains("Georgia") || d.contains("serif"));
        let (_, d2) = map_fonts(&["Courier".into()]);
        assert!(d2.contains("mono") || d2.contains("Mono"));
    }

    #[test]
    fn build_style_classifies_palette() {
        // Papel claro, tinta oscura, acento saturado (rojo).
        let pal = vec![
            (250, 248, 245, 0.6), // paper
            (40, 50, 60, 0.2),    // ink
            (200, 40, 40, 0.2),   // accent rojo
        ];
        let s = build_style(&pal, &["Helvetica".into()], "Extraído");
        let (lr, _) = light_sat(
            u8::from_str_radix(&s.paper[1..3], 16).unwrap() as f32,
            u8::from_str_radix(&s.paper[3..5], 16).unwrap() as f32,
            u8::from_str_radix(&s.paper[5..7], 16).unwrap() as f32,
        );
        assert!(lr > 0.9, "paper claro");
        assert!(
            s.accent.starts_with('#') && s.accent.len() == 7,
            "accent hex"
        );
        assert!(
            s.accent.to_lowercase() != s.ink.to_lowercase(),
            "accent ≠ ink"
        );
    }
}
