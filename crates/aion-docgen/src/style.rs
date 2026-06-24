//! **Sistema de estilos de documento (themes).**
//!
//! Separa el ESTILO (paleta, tipografías, radios) del CONTENIDO y de la IDENTIDAD de marca:
//! - [`crate::BrandProfile`] = quién eres (empresa, logo, contacto, legal).
//! - [`DocStyle`] = cómo se ve (colores, fuentes, esquinas) — intercambiable a gusto.
//! - El contenido (p. ej. [`crate::OffertaContent`]) = qué dice.
//!
//! Así el MISMO documento se renderiza con muchos looks distintos (no siempre igual), el
//! usuario elige un preset (galería tipo Canva), guarda los suyos, o extrae uno de un
//! documento de referencia (ver `style_extract`, roadmap). Los tokens viajan a la plantilla
//! como variables CSS.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Pila de fuentes con fallback offline (no dependemos de Google Fonts: render local).
pub const SANS: &str =
    "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, Helvetica, Arial, sans-serif";
pub const SERIF: &str = "Georgia, Cambria, \"Times New Roman\", Times, serif";
pub const MONO: &str = "ui-monospace, \"SF Mono\", Menlo, Consolas, monospace";

/// Tokens visuales de un documento. Se inyectan como variables CSS en la plantilla.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocStyle {
    /// Nombre legible del estilo (para la galería y al guardar).
    pub name: String,
    /// Color oscuro: cajas hero/beneficios, cabeceras de tabla, tinta de títulos.
    pub ink: String,
    /// Acento focal (barras de sección, kickers, pills, total).
    pub accent: String,
    /// Fondo de página.
    pub paper: String,
    /// Texto de cuerpo.
    pub text: String,
    /// Texto secundario / notas.
    pub muted: String,
    /// Líneas/bordes finos.
    pub hair: String,
    /// Fondo suave de tarjetas/cajas claras.
    pub soft: String,
    /// Fondo del callout destacado (tinte cálido por defecto).
    pub cream: String,
    /// Pila de fuentes del cuerpo.
    pub font: String,
    /// Pila de fuentes de los titulares (puede ser serif/mono para un look distinto).
    pub font_display: String,
    /// Radio de esquinas en px (0 = recto/editorial, 12 = muy redondeado).
    pub radius: u8,
    /// Titulares de sección en MAYÚSCULAS (look editorial/bold).
    #[serde(default)]
    pub caps_headings: bool,
}

impl Default for DocStyle {
    fn default() -> Self {
        // "Slate · oro" — el look corporativo cálido (estilo ProntoClick).
        Self {
            name: "Slate · oro".into(),
            ink: "#2f4858".into(),
            accent: "#c69a24".into(),
            paper: "#ffffff".into(),
            text: "#1f2937".into(),
            muted: "#6b7280".into(),
            hair: "#e6e8ea".into(),
            soft: "#f4f6f7".into(),
            cream: "#fbf6e9".into(),
            font: SANS.into(),
            font_display: SANS.into(),
            radius: 10,
            caps_headings: false,
        }
    }
}

impl DocStyle {
    /// Carga un estilo desde disco; fail-soft al default si falta o es ilegible.
    pub fn load(path: impl AsRef<Path>) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persiste el estilo (escritura atómica tmp → rename).
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        let json = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }
}

/// **Galería de presets** (la base de "elegir estilo como en Canva"). Looks bien distintos
/// entre sí: corporativo cálido, tech moderno, editorial serif, bold de alto contraste.
pub fn presets() -> Vec<DocStyle> {
    vec![
        DocStyle::default(),
        DocStyle {
            name: "Plasma · teal".into(),
            ink: "#0f172a".into(),
            accent: "#0fb5ba".into(),
            paper: "#ffffff".into(),
            text: "#1a1815".into(),
            muted: "#6b6760".into(),
            hair: "#e9e7e2".into(),
            soft: "#f5f5f3".into(),
            cream: "#e9fbfb".into(),
            font: SANS.into(),
            font_display: SANS.into(),
            radius: 12,
            caps_headings: false,
        },
        DocStyle {
            name: "Editoriale · serif".into(),
            ink: "#1f1d1a".into(),
            accent: "#b45309".into(),
            paper: "#fbf7ef".into(),
            text: "#2b2b2b".into(),
            muted: "#7a7468".into(),
            hair: "#e7e0d4".into(),
            soft: "#f3ede1".into(),
            cream: "#f6eedd".into(),
            font: SERIF.into(),
            font_display: SERIF.into(),
            radius: 3,
            caps_headings: true,
        },
        DocStyle {
            name: "Notte · bold".into(),
            ink: "#0f1729".into(),
            accent: "#2563eb".into(),
            paper: "#ffffff".into(),
            text: "#0f172a".into(),
            muted: "#64748b".into(),
            hair: "#e2e8f0".into(),
            soft: "#f1f5f9".into(),
            cream: "#eef2ff".into(),
            font: SANS.into(),
            font_display: MONO.into(),
            radius: 0,
            caps_headings: true,
        },
    ]
}

/// Busca un preset por nombre (case-insensitive). `None` si no existe.
pub fn by_name(name: &str) -> Option<DocStyle> {
    presets()
        .into_iter()
        .find(|s| s.name.eq_ignore_ascii_case(name.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_distinct() {
        let p = presets();
        assert!(p.len() >= 4, "varios estilos");
        // Paletas realmente distintas (no el mismo look).
        let accents: std::collections::HashSet<_> = p.iter().map(|s| s.accent.clone()).collect();
        assert_eq!(accents.len(), p.len(), "acentos únicos por estilo");
    }

    #[test]
    fn by_name_finds_preset() {
        assert!(by_name("plasma · teal").is_some());
        assert!(by_name("inexistente").is_none());
    }

    #[test]
    fn roundtrip_save_load() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("aion-style-test-{}.json", std::process::id()));
        let s = DocStyle {
            name: "Mío".into(),
            accent: "#ff0066".into(),
            ..DocStyle::default()
        };
        s.save(&p).expect("save");
        let back = DocStyle::load(&p);
        assert_eq!(back.name, "Mío");
        assert_eq!(back.accent, "#ff0066");
        let _ = std::fs::remove_file(&p);
    }
}
