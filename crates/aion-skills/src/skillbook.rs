//! # SkillBook — memoria procedimental de AION
//!
//! Almacena procedimientos reutilizables (secuencias de tool-calls con plantillas)
//! que el agente puede descubrir, ejecutar y mejorar a lo largo del tiempo.
//! Cada ejecución actualiza contadores de éxito/fallo; la reputación Bayesiana
//! y la relevancia semántica permiten recuperar los procedimientos más útiles
//! para un contexto dado.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// ProcedureStep
// ---------------------------------------------------------------------------

/// Un paso dentro de un procedimiento: qué herramienta usar y cómo parametrizarla.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureStep {
    /// Nombre de la herramienta / skill a invocar (p.ej. `"web_search"`, `"read_file"`).
    pub tool: String,
    /// Plantilla de entrada con marcadores `{{variable}}` que el agente rellena en tiempo de ejecución.
    pub input_template: String,
    /// Descripción legible de qué hace este paso.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Procedure
// ---------------------------------------------------------------------------

/// Un procedimiento completo: secuencia de pasos reutilizable con métricas de uso.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Procedure {
    /// Identificador único (slug legible o UUID).
    pub id: String,
    /// Nombre corto del procedimiento.
    pub name: String,
    /// Descripción detallada de qué resuelve este procedimiento.
    pub description: String,
    /// Pasos ordenados que componen el procedimiento.
    pub steps: Vec<ProcedureStep>,
    /// Cuántas veces se ejecutó con éxito.
    pub success_count: u32,
    /// Cuántas veces falló.
    pub failure_count: u32,
    /// Versión del procedimiento (se incrementa al modificar los pasos).
    pub version: u32,
    /// Etiquetas para categorización y búsqueda.
    pub tags: Vec<String>,
    /// Unix timestamp de creación (segundos).
    pub created_at: u64,
    /// Unix timestamp del último uso exitoso (segundos).
    pub last_used_at: u64,
}

impl Procedure {
    /// Crea un procedimiento nuevo con id, nombre y descripción.
    /// `created_at` se fija al instante actual; el resto de contadores a cero.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let now = now_unix();
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            steps: Vec::new(),
            success_count: 0,
            failure_count: 0,
            version: 1,
            tags: Vec::new(),
            created_at: now,
            last_used_at: 0,
        }
    }

    /// Builder: asocia una lista de pasos al procedimiento.
    pub fn with_steps(mut self, steps: Vec<ProcedureStep>) -> Self {
        self.steps = steps;
        self
    }

    /// Incrementa la versión del procedimiento en 1.
    pub fn bump_version(&mut self) {
        self.version += 1;
    }

    /// Builder: asocia etiquetas al procedimiento.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Reputación Bayesiana con prior suavizado (8 éxitos, 10 totales).
    /// Devuelve un valor en (0.0, 1.0) donde 1.0 es excelente reputación.
    pub fn reputation(&self) -> f64 {
        let total = (self.success_count + self.failure_count) as f64;
        (self.success_count as f64 + 8.0) / (total + 10.0)
    }

    /// Relevancia semántica por solapamiento de palabras entre `query` y
    /// el nombre, descripción y etiquetas del procedimiento.
    /// Devuelve un valor en [0.0, 1.0].
    pub fn relevance_to(&self, query: &str) -> f64 {
        let query_words: std::collections::HashSet<String> = query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() > 2)
            .collect();

        if query_words.is_empty() {
            return 0.0;
        }

        // Corpus: nombre + descripción + etiquetas
        let corpus = format!("{} {} {}", self.name, self.description, self.tags.join(" "));
        let corpus_words: std::collections::HashSet<String> = corpus
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();

        let matches = query_words.intersection(&corpus_words).count();
        matches as f64 / query_words.len() as f64
    }
}

// ---------------------------------------------------------------------------
// VersionInfo
// ---------------------------------------------------------------------------

/// Información de versión resumida de un procedimiento.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Identificador del procedimiento.
    pub id: String,
    /// Versión actual del procedimiento.
    pub version: u32,
    /// Reputación Bayesiana actual (0.0–1.0).
    pub reputation: f64,
    /// Total de ejecuciones (éxitos + fallos).
    pub total_calls: u32,
}

// ---------------------------------------------------------------------------
// SkillBook
// ---------------------------------------------------------------------------

/// Memoria procedimental de AION: colección persistente de procedimientos.
pub struct SkillBook {
    /// Lista de procedimientos registrados.
    pub procedures: Vec<Procedure>,
    /// Ruta al fichero JSON de persistencia.
    pub path: PathBuf,
}

impl SkillBook {
    /// Carga el SkillBook desde disco. Si el fichero no existe, devuelve uno vacío.
    pub fn load(path: PathBuf) -> Self {
        let procedures = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|contents| serde_json::from_str(&contents).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self { procedures, path }
    }

    /// Serializa el SkillBook a disco en formato JSON indentado.
    pub fn save(&self) -> std::io::Result<()> {
        // Crea el directorio padre si no existe
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.procedures).map_err(std::io::Error::other)?;
        std::fs::write(&self.path, json)
    }

    /// Inserta un procedimiento nuevo o reemplaza el existente con el mismo `id`.
    /// Persiste inmediatamente.
    pub fn upsert(&mut self, proc: Procedure) {
        if let Some(existing) = self.procedures.iter_mut().find(|p| p.id == proc.id) {
            *existing = proc;
        } else {
            self.procedures.push(proc);
        }
        let _ = self.save();
    }

    /// Actualiza los contadores de éxito/fallo de un procedimiento.
    /// Si `success` es `true`, también actualiza `last_used_at`.
    /// Persiste inmediatamente.
    pub fn record_execution(&mut self, id: &str, success: bool) {
        if let Some(proc) = self.procedures.iter_mut().find(|p| p.id == id) {
            if success {
                proc.success_count += 1;
                proc.last_used_at = now_unix();
            } else {
                proc.failure_count += 1;
            }
            let _ = self.save();
        }
    }

    /// Devuelve los procedimientos más relevantes para `query`.
    /// Puntuación = relevancia × reputación. Solo devuelve los que superen 0.1.
    pub fn find_relevant(&self, query: &str, top_n: usize) -> Vec<&Procedure> {
        let mut scored: Vec<(&Procedure, f64)> = self
            .procedures
            .iter()
            .filter_map(|p| {
                let score = p.relevance_to(query) * p.reputation();
                if score > 0.1 {
                    Some((p, score))
                } else {
                    None
                }
            })
            .collect();

        // Orden descendente por puntuación
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(top_n).map(|(p, _)| p).collect()
    }

    /// Devuelve todos los procedimientos registrados.
    pub fn all(&self) -> &[Procedure] {
        &self.procedures
    }

    /// Número total de procedimientos.
    pub fn len(&self) -> usize {
        self.procedures.len()
    }

    /// `true` si no hay ningún procedimiento registrado.
    pub fn is_empty(&self) -> bool {
        self.procedures.is_empty()
    }

    /// Elimina el procedimiento con el `id` dado.
    /// Persiste si se encontró y devuelve `true`; devuelve `false` si no existía.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.procedures.len();
        self.procedures.retain(|p| p.id != id);
        let removed = self.procedures.len() < before;
        if removed {
            let _ = self.save();
        }
        removed
    }

    /// Actualiza los pasos de un procedimiento existente, incrementa su versión y
    /// reinicia el contador de fallos (arranque limpio con los nuevos pasos).
    /// Persiste inmediatamente si el procedimiento existe.
    /// Devuelve `true` si se encontró y actualizó, `false` si el `id` no existe.
    pub fn upgrade(&mut self, id: &str, new_steps: Vec<ProcedureStep>, reason: &str) -> bool {
        let _ = reason; // registrado en comentario; en el futuro podría persistirse como changelog
        if let Some(proc) = self.procedures.iter_mut().find(|p| p.id == id) {
            proc.steps = new_steps;
            proc.bump_version();
            proc.failure_count = 0;
            let _ = self.save();
            true
        } else {
            false
        }
    }

    /// Devuelve la información de versión de todos los procedimientos,
    /// ordenados por versión descendente (el más evolucionado primero).
    pub fn version_report(&self) -> Vec<VersionInfo> {
        let mut report: Vec<VersionInfo> = self
            .procedures
            .iter()
            .map(|p| VersionInfo {
                id: p.id.clone(),
                version: p.version,
                reputation: p.reputation(),
                total_calls: p.success_count + p.failure_count,
            })
            .collect();
        report.sort_by_key(|a| std::cmp::Reverse(a.version));
        report
    }

    /// Formatea los top-3 procedimientos relevantes para inyectarlos en el prompt del LLM.
    /// Devuelve `None` si no hay procedimientos con puntuación suficiente.
    pub fn format_for_prompt(&self, query: &str) -> Option<String> {
        let relevant = self.find_relevant(query, 3);
        if relevant.is_empty() {
            return None;
        }

        let mut out = String::from("## Procedimientos disponibles\n\n");
        for (i, proc) in relevant.iter().enumerate() {
            out.push_str(&format!(
                "### {}. {} (id: `{}`)\n{}\n",
                i + 1,
                proc.name,
                proc.id,
                proc.description
            ));
            if !proc.tags.is_empty() {
                out.push_str(&format!("Etiquetas: {}\n", proc.tags.join(", ")));
            }
            if !proc.steps.is_empty() {
                out.push_str("Pasos:\n");
                for (j, step) in proc.steps.iter().enumerate() {
                    out.push_str(&format!(
                        "  {}. [{}] {} — `{}`\n",
                        j + 1,
                        step.tool,
                        step.description,
                        step.input_template
                    ));
                }
            }
            out.push('\n');
        }
        Some(out)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_procedure() -> Procedure {
        Procedure::new(
            "buscar-web",
            "Búsqueda web",
            "Busca información en la web y resume los resultados",
        )
        .with_steps(vec![
            ProcedureStep {
                tool: "web_search".into(),
                input_template: "{{query}}".into(),
                description: "Ejecuta la búsqueda".into(),
            },
            ProcedureStep {
                tool: "summarize".into(),
                input_template: "{{results}}".into(),
                description: "Resume los resultados".into(),
            },
        ])
        .with_tags(vec!["web".into(), "búsqueda".into(), "información".into()])
    }

    #[test]
    fn reputation_prior() {
        let proc = sample_procedure();
        // Con cero usos: (0 + 8) / (0 + 10) = 0.8
        assert!((proc.reputation() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn reputation_with_successes() {
        let mut proc = sample_procedure();
        proc.success_count = 90;
        proc.failure_count = 10;
        // (90 + 8) / (100 + 10) = 98 / 110 ≈ 0.8909
        let rep = proc.reputation();
        assert!(rep > 0.88 && rep < 0.90);
    }

    #[test]
    fn relevance_exact_match() {
        let proc = sample_procedure();
        let score = proc.relevance_to("búsqueda web información");
        assert!(score > 0.0, "debe haber solapamiento");
    }

    #[test]
    fn relevance_no_match() {
        let proc = sample_procedure();
        let score = proc.relevance_to("cocinar pasta italiana");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn skillbook_upsert_and_find() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skillbook.json");
        let mut sb = SkillBook::load(path.clone());

        assert!(sb.is_empty());

        sb.upsert(sample_procedure());
        assert_eq!(sb.len(), 1);

        let found = sb.find_relevant("búsqueda web", 5);
        assert!(!found.is_empty());
    }

    #[test]
    fn skillbook_record_execution() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skillbook.json");
        let mut sb = SkillBook::load(path);

        sb.upsert(sample_procedure());
        sb.record_execution("buscar-web", true);
        sb.record_execution("buscar-web", false);

        let proc = sb.all().first().unwrap();
        assert_eq!(proc.success_count, 1);
        assert_eq!(proc.failure_count, 1);
        assert!(proc.last_used_at > 0);
    }

    #[test]
    fn skillbook_remove() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skillbook.json");
        let mut sb = SkillBook::load(path);

        sb.upsert(sample_procedure());
        assert!(sb.remove("buscar-web"));
        assert!(sb.is_empty());
        assert!(!sb.remove("buscar-web")); // segunda vez → false
    }

    #[test]
    fn format_for_prompt_returns_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skillbook.json");
        let mut sb = SkillBook::load(path);

        sb.upsert(sample_procedure());
        let prompt = sb.format_for_prompt("búsqueda web");
        assert!(prompt.is_some());
        let text = prompt.unwrap();
        assert!(text.contains("buscar-web"));
        assert!(text.contains("web_search"));
    }
}
