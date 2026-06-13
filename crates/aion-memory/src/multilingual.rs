//! **Idiomas para optimización de tokens.**
//!
//! AION guarda su memoria en español y la sirve íntegra al modelo local (gratis). La
//! única optimización de idioma que aporta valor está en el puente MCP hacia agentes
//! externos de pago (Claude Code), donde la memoria se traduce a inglés (~40% menos
//! tokens). Esa traducción vive en `apps/aion-core` (`mcp_compact`); aquí solo se
//! define el tipo de idioma compartido que usa el detector heurístico.

use serde::{Deserialize, Serialize};

/// Idiomas reconocidos por el detector heurístico.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    Spanish,
    English,
    Italian,
    Other,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Spanish => "es",
            Language::English => "en",
            Language::Italian => "it",
            Language::Other => "other",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "es" | "español" | "spanish" => Language::Spanish,
            "en" | "english" | "inglés" => Language::English,
            "it" | "italian" | "italiano" => Language::Italian,
            _ => Language::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_detection() {
        assert_eq!(Language::from_str("es"), Language::Spanish);
        assert_eq!(Language::from_str("en"), Language::English);
        assert_eq!(Language::from_str("español"), Language::Spanish);
    }

    #[test]
    fn language_str_conversion() {
        assert_eq!(Language::Spanish.as_str(), "es");
        assert_eq!(Language::English.as_str(), "en");
        assert_eq!(Language::Italian.as_str(), "it");
    }
}
