//! **Perfil de marca** — el conocimiento de marca reutilizable que se aplica a cada
//! documento. Es el equivalente *estructurado* al "SKILL.md de marca" de las Agent Skills
//! de Anthropic: en vez de re-explicar la identidad en cada conversación, vive aquí una vez
//! y se inyecta en la plantilla. Persiste como JSON (lo guarda quien lo use, p. ej.
//! `apps/aion-core` en `~/.aion/brand_profile.json`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Identidad de marca aplicada a los documentos generados.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandProfile {
    /// Nombre que encabeza el documento.
    pub company: String,
    /// Lema breve bajo el nombre (puede ir vacío).
    pub tagline: String,
    /// Color de acento focal (hex). Por defecto el plasma teal de AION.
    pub accent: String,
    /// Color base / tinta (hex).
    pub ink: String,
    /// Logo embebido como `data:` URI (base64) para que el PDF sea offline y autocontenido.
    /// `None` ⇒ se usa el nombre como wordmark.
    pub logo_data_uri: Option<String>,
    /// Idioma de salida (ISO, p. ej. "es", "it"). Informa al `lang` del HTML.
    pub lang: String,
    /// Pie legal (se repite en el footer del documento).
    pub legal_footer: String,
    /// Web y email de contacto (footer).
    pub website: String,
    pub email: String,
    /// Contadores de numeración por tipo de documento (p. ej. "preventivo" → 30).
    /// Se usa para emitir números correlativos tipo `PREV-2026-031`.
    #[serde(default)]
    pub doc_counters: BTreeMap<String, u32>,
}

impl Default for BrandProfile {
    fn default() -> Self {
        // Identidad AION (design tokens reales: plasma teal sobre slate-900).
        Self {
            company: "AION".into(),
            tagline: "Inteligencia local con mente observable".into(),
            accent: "#0FB5BA".into(),
            ink: "#0F172A".into(),
            logo_data_uri: None,
            lang: "es".into(),
            legal_footer: "Generado localmente por AION · Documento confidencial".into(),
            website: "".into(),
            email: "".into(),
            doc_counters: BTreeMap::new(),
        }
    }
}

impl BrandProfile {
    /// Carga el perfil desde disco; si no existe o es ilegible, devuelve el perfil por defecto
    /// (fail-soft: nunca rompe la generación por un perfil ausente).
    pub fn load(path: impl AsRef<Path>) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persiste el perfil a disco (escritura atómica: tmp → rename).
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        let json = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// Reserva y devuelve el siguiente número correlativo para un tipo de documento.
    /// Formato `PREFIJO-AÑO-NNN` (p. ej. `PREV-2026-031`). Incrementa el contador en memoria;
    /// el llamante decide cuándo persistir con [`save`](Self::save).
    pub fn next_number(&mut self, doc_type: &str, prefix: &str, year: i32) -> String {
        let n = self.doc_counters.entry(doc_type.to_string()).or_insert(0);
        *n += 1;
        format!("{prefix}-{year}-{:03}", *n)
    }
}
