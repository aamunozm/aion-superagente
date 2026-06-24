//! **Gráficos SVG on-brand** (vector, nítidos en PDF, cero dependencias, offline).
//!
//! Investigación 2026: en PDF, SVG = vector nítido y texto seleccionable; `<canvas>` (Chart.js)
//! = bitmap borroso. Por eso los gráficos de AION son **SVG hecho a mano**, teñidos con los
//! tokens del [`crate::DocStyle`] (acento/tinta/líneas/fuente) para mantener SIEMPRE el estilo.

use crate::DocStyle;

/// Color semántico por puntuación (rojo malo · acento medio · verde bueno), respetando la marca.
fn score_color(score: u32, style: &DocStyle) -> &str {
    if score >= 75 {
        "#2f9e6f"
    } else if score >= 50 {
        // El acento de la marca para el rango medio (se mantiene on-brand).
        style.accent.as_str()
    } else {
        "#c0594e"
    }
}

/// **Medidor circular (donut)** de una puntuación 0–100 con el número al centro. Moderno y limpio:
/// pista neutra + arco con extremos redondeados del color semántico, tipografía de la marca.
pub fn score_gauge(score: u32, label: &str, style: &DocStyle) -> String {
    let score = score.min(100);
    let (cx, cy, r, sw) = (80.0_f64, 80.0_f64, 60.0_f64, 14.0_f64);
    let circ = 2.0 * std::f64::consts::PI * r;
    let arc = circ * (score as f64 / 100.0);
    let color = score_color(score, style);
    format!(
        r##"<svg class="gauge" viewBox="0 0 160 184" width="150" xmlns="http://www.w3.org/2000/svg">
  <circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{track}" stroke-width="{sw}"/>
  <circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{color}" stroke-width="{sw}" stroke-linecap="round"
    stroke-dasharray="{arc:.1} {rest:.1}" transform="rotate(-90 {cx} {cy})"/>
  <text x="{cx}" y="{ty}" text-anchor="middle" font-family="{font}" font-weight="800" font-size="40" fill="{ink}">{score}</text>
  <text x="{cx}" y="{ty2}" text-anchor="middle" font-family="{font}" font-weight="600" font-size="13" fill="{muted}">/ 100</text>
  <text x="{cx}" y="176" text-anchor="middle" font-family="{font}" font-weight="700" font-size="12" fill="{ink}" letter-spacing="0.04em">{label}</text>
</svg>"##,
        track = style.hair,
        rest = (circ - arc),
        font = svg_font(style),
        ink = style.ink,
        muted = style.muted,
        ty = cy + 6.0,
        ty2 = cy + 26.0,
        label = esc(label),
    )
}

/// Una barra de la comparativa: etiqueta, valor, % (0–100) y tono (red/gold/green).
pub struct Bar {
    pub label: String,
    pub value: String,
    pub pct: u8,
    pub tone: String,
}

/// **Barras horizontales** (comparativa de costes / progreso) en SVG, on-brand. Filas con
/// etiqueta a la izquierda, barra al centro (pista + relleno redondeado) y valor a la derecha.
pub fn hbars(bars: &[Bar], style: &DocStyle) -> String {
    if bars.is_empty() {
        return String::new();
    }
    let (w, row_h, pad_top) = (560.0_f64, 30.0_f64, 6.0_f64);
    let label_w = 165.0_f64;
    let value_w = 95.0_f64;
    let track_x = label_w + 8.0;
    let track_w = w - label_w - value_w - 16.0;
    let h = pad_top * 2.0 + row_h * bars.len() as f64;
    let mut rows = String::new();
    for (i, b) in bars.iter().enumerate() {
        let y = pad_top + row_h * i as f64 + row_h / 2.0;
        let pct = (b.pct.min(100)) as f64 / 100.0;
        let fill_w = (track_w * pct).max(8.0);
        let color = match b.tone.as_str() {
            "red" => "#c0594e",
            "green" => "#2f9e6f",
            _ => style.accent.as_str(),
        };
        let strong = b.tone == "green";
        rows.push_str(&format!(
            r##"<text x="0" y="{ty}" font-family="{font}" font-size="13" font-weight="{lw}" fill="{lc}">{label}</text>
  <rect x="{tx}" y="{by}" width="{tw:.0}" height="12" rx="6" fill="{track}"/>
  <rect x="{tx}" y="{by}" width="{fw:.0}" height="12" rx="6" fill="{color}"/>
  <text x="{w}" y="{ty}" text-anchor="end" font-family="{font}" font-size="13" font-weight="{vw}" fill="{vc}">{value}</text>
"##,
            ty = y + 4.0,
            by = y - 6.0,
            font = svg_font(style),
            label = esc(&b.label),
            lw = if strong { 700 } else { 400 },
            lc = if strong { style.ink.as_str() } else { style.text.as_str() },
            tx = track_x,
            tw = track_w,
            track = style.soft,
            fw = fill_w,
            color = color,
            w = w,
            value = esc(&b.value),
            vw = if strong { 800 } else { 600 },
            vc = if strong { style.ink.as_str() } else { style.text.as_str() },
        ));
    }
    format!(
        r#"<svg class="hbars" viewBox="0 0 {w} {h}" width="100%" xmlns="http://www.w3.org/2000/svg">{rows}</svg>"#
    )
}

fn svg_font(style: &DocStyle) -> String {
    // En SVG, comillas dobles rompen el atributo; usa la primera familia sin comillas.
    style
        .font_display
        .split(',')
        .next()
        .unwrap_or("sans-serif")
        .replace('"', "")
        .trim()
        .to_string()
}

/// Escapa para texto SVG (sin `<`, `>`, `&`).
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gauge_y_barras_generan_svg_valido() {
        let st = DocStyle::default();
        let g = score_gauge(50, "SEO", &st);
        assert!(g.contains("<svg") && g.contains("50") && g.contains("</svg>"));
        let b = hbars(
            &[
                Bar {
                    label: "Agenzia".into(),
                    value: "€450".into(),
                    pct: 100,
                    tone: "red".into(),
                },
                Bar {
                    label: "Noi".into(),
                    value: "€200".into(),
                    pct: 40,
                    tone: "green".into(),
                },
            ],
            &st,
        );
        assert!(b.contains("<svg") && b.contains("Agenzia") && b.contains("</svg>"));
    }
}
