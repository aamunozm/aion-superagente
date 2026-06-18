//! Herramienta `skillbook` — acceso del agente a la memoria procedimental de AION.
//!
//! Permite al agente ReAct listar, buscar, guardar, inspeccionar y eliminar
//! procedimientos reutilizables almacenados en el [`SkillBook`].

use aion_orchestrator::{Tool, ToolCategory};
use aion_skills::{Procedure, ProcedureStep, SkillBook}; // VersionInfo se usa internamente en version_report
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// SkillBookTool
// ---------------------------------------------------------------------------

/// Herramienta que expone el [`SkillBook`] al agente.
///
/// # Comandos (input)
///
/// | Sintaxis | Acción |
/// |---|---|
/// | `list` | Lista todos los procedimientos con id, nombre, versión, reputación y éxitos. |
/// | `find ::: <query>` | Encuentra los procedimientos más relevantes para la consulta. |
/// | `save ::: <id> ::: <name> ::: <desc> ::: tool1=input1\|tool2=input2` | Crea y guarda un procedimiento. |
/// | `stats ::: <id>` | Muestra las estadísticas detalladas de un procedimiento. |
/// | `remove ::: <id>` | Elimina un procedimiento. |
/// | `upgrade ::: <id> ::: tool1=input1\|tool2=input2` | Reemplaza los pasos, sube la versión y reinicia fallos. |
/// | `versions` | Muestra versión, reputación y llamadas totales de todos los procedimientos. |
pub struct SkillBookTool {
    book: Arc<Mutex<SkillBook>>,
}

impl SkillBookTool {
    pub fn new(book: Arc<Mutex<SkillBook>>) -> Self {
        Self { book }
    }
}

#[async_trait]
impl Tool for SkillBookTool {
    fn name(&self) -> &str {
        "skillbook"
    }

    fn description(&self) -> &str {
        "Gestiona la memoria procedimental de AION (SkillBook): lista, busca, guarda, \
         inspecciona, actualiza y elimina procedimientos reutilizables. \
         Comandos: 'list' | 'find ::: <query>' | \
         'save ::: <id> ::: <nombre> ::: <desc> ::: tool1=input1|tool2=input2' | \
         'stats ::: <id>' | 'remove ::: <id>' | \
         'upgrade ::: <id> ::: tool1=input1|tool2=input2' | 'versions'"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Intelligence
    }

    async fn run(&self, input: &str) -> Result<String, String> {
        let input = input.trim();

        // Divide por el separador ":::" con espacios opcionales
        let parts: Vec<&str> = input.splitn(5, ":::").map(|s| s.trim()).collect();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            // ── list ─────────────────────────────────────────────────────────
            "list" => {
                let book = self.book.lock().await;
                let all = book.all();
                if all.is_empty() {
                    return Ok("El SkillBook está vacío. Usa 'save' para añadir procedimientos.".into());
                }
                let mut out = format!("## SkillBook — {} procedimiento(s)\n\n", all.len());
                out.push_str("| id | nombre | v | reputación | éxitos |\n");
                out.push_str("|---|---|---|---|---|\n");
                for p in all {
                    out.push_str(&format!(
                        "| {} | {} | {} | {:.2} | {} |\n",
                        p.id,
                        p.name,
                        p.version,
                        p.reputation(),
                        p.success_count
                    ));
                }
                Ok(out)
            }

            // ── find ::: query ───────────────────────────────────────────────
            "find" => {
                let query = parts.get(1).copied().unwrap_or("").trim();
                if query.is_empty() {
                    return Err("Uso: 'find ::: <consulta>'".into());
                }
                let book = self.book.lock().await;
                let relevant = book.find_relevant(query, 5);
                if relevant.is_empty() {
                    return Ok(format!(
                        "No se encontraron procedimientos relevantes para «{}».",
                        query
                    ));
                }
                let mut out = format!(
                    "## Procedimientos relevantes para «{}»\n\n",
                    query
                );
                for (i, p) in relevant.iter().enumerate() {
                    out.push_str(&format!(
                        "### {}. {} (id: `{}`)\n{}\nReputación: {:.2} | Éxitos: {} | Versión: {}\n",
                        i + 1,
                        p.name,
                        p.id,
                        p.description,
                        p.reputation(),
                        p.success_count,
                        p.version
                    ));
                    if !p.tags.is_empty() {
                        out.push_str(&format!("Etiquetas: {}\n", p.tags.join(", ")));
                    }
                    out.push('\n');
                }
                Ok(out)
            }

            // ── save ::: id ::: name ::: desc ::: tool1=input1|tool2=input2 ─
            "save" => {
                let id = parts.get(1).copied().unwrap_or("").trim();
                let name = parts.get(2).copied().unwrap_or("").trim();
                let description = parts.get(3).copied().unwrap_or("").trim();
                let steps_raw = parts.get(4).copied().unwrap_or("").trim();

                if id.is_empty() || name.is_empty() || description.is_empty() {
                    return Err(
                        "Uso: 'save ::: <id> ::: <nombre> ::: <descripción> ::: tool1=input1|tool2=input2'".into(),
                    );
                }

                // Parsea los pasos: "tool1=input1|tool2=input2"
                let steps: Vec<ProcedureStep> = if steps_raw.is_empty() {
                    Vec::new()
                } else {
                    steps_raw
                        .split('|')
                        .filter_map(|s| {
                            let s = s.trim();
                            if s.is_empty() {
                                return None;
                            }
                            let (tool, input_tpl) = s
                                .split_once('=')
                                .map(|(t, i)| (t.trim().to_string(), i.trim().to_string()))
                                .unwrap_or_else(|| (s.to_string(), String::new()));
                            Some(ProcedureStep {
                                description: format!("Invocar {}", tool),
                                tool,
                                input_template: input_tpl,
                            })
                        })
                        .collect()
                };

                let n_steps = steps.len();
                let proc = Procedure::new(id, name, description).with_steps(steps);

                let mut book = self.book.lock().await;
                book.upsert(proc);

                Ok(format!(
                    "Procedimiento '{}' guardado en el SkillBook ({} paso(s)).",
                    id, n_steps
                ))
            }

            // ── stats ::: id ─────────────────────────────────────────────────
            "stats" => {
                let id = parts.get(1).copied().unwrap_or("").trim();
                if id.is_empty() {
                    return Err("Uso: 'stats ::: <id>'".into());
                }
                let book = self.book.lock().await;
                match book.all().iter().find(|p| p.id == id) {
                    None => Err(format!("No existe el procedimiento '{}'.", id)),
                    Some(p) => {
                        let total = p.success_count + p.failure_count;
                        let mut out = format!(
                            "## SkillBook — stats de `{}`\n\n\
                             **Nombre**: {}\n\
                             **Versión**: {}\n\
                             **Descripción**: {}\n\
                             **Reputación**: {:.4}\n\
                             **Éxitos**: {} / {} ({:.1}%)\n\
                             **Creado**: {}\n\
                             **Último uso exitoso**: {}\n",
                            p.id,
                            p.name,
                            p.version,
                            p.description,
                            p.reputation(),
                            p.success_count,
                            total,
                            if total > 0 {
                                p.success_count as f64 / total as f64 * 100.0
                            } else {
                                0.0
                            },
                            p.created_at,
                            if p.last_used_at > 0 {
                                p.last_used_at.to_string()
                            } else {
                                "nunca".into()
                            }
                        );
                        if !p.tags.is_empty() {
                            out.push_str(&format!("**Etiquetas**: {}\n", p.tags.join(", ")));
                        }
                        if !p.steps.is_empty() {
                            out.push_str(&format!("\n**Pasos** ({}):\n", p.steps.len()));
                            for (i, step) in p.steps.iter().enumerate() {
                                out.push_str(&format!(
                                    "  {}. [{}] {} — `{}`\n",
                                    i + 1,
                                    step.tool,
                                    step.description,
                                    step.input_template
                                ));
                            }
                        }
                        Ok(out)
                    }
                }
            }

            // ── remove ::: id ────────────────────────────────────────────────
            "remove" => {
                let id = parts.get(1).copied().unwrap_or("").trim();
                if id.is_empty() {
                    return Err("Uso: 'remove ::: <id>'".into());
                }
                let mut book = self.book.lock().await;
                if book.remove(id) {
                    Ok(format!("Procedimiento '{}' eliminado del SkillBook.", id))
                } else {
                    Err(format!("No existe el procedimiento '{}'.", id))
                }
            }

            // ── upgrade ::: id ::: tool1=input1|tool2=input2 ────────────────
            "upgrade" => {
                let id = parts.get(1).copied().unwrap_or("").trim();
                let steps_raw = parts.get(2).copied().unwrap_or("").trim();

                if id.is_empty() {
                    return Err(
                        "Uso: 'upgrade ::: <id> ::: tool1=input1|tool2=input2'".into(),
                    );
                }

                // Parsea pasos con el mismo formato que 'save'
                let steps: Vec<ProcedureStep> = if steps_raw.is_empty() {
                    Vec::new()
                } else {
                    steps_raw
                        .split('|')
                        .filter_map(|s| {
                            let s = s.trim();
                            if s.is_empty() {
                                return None;
                            }
                            let (tool, input_tpl) = s
                                .split_once('=')
                                .map(|(t, i)| (t.trim().to_string(), i.trim().to_string()))
                                .unwrap_or_else(|| (s.to_string(), String::new()));
                            Some(ProcedureStep {
                                description: format!("Invocar {}", tool),
                                tool,
                                input_template: input_tpl,
                            })
                        })
                        .collect()
                };

                let n_steps = steps.len();
                let mut book = self.book.lock().await;
                if book.upgrade(id, steps, "actualizado vía skillbook_tool") {
                    Ok(format!(
                        "Procedimiento '{}' actualizado: {} paso(s), versión incrementada, fallos reiniciados.",
                        id, n_steps
                    ))
                } else {
                    Err(format!(
                        "No existe el procedimiento '{}'. Usa 'save' para crearlo primero.",
                        id
                    ))
                }
            }

            // ── versions ─────────────────────────────────────────────────────
            "versions" => {
                let book = self.book.lock().await;
                let report = book.version_report();
                if report.is_empty() {
                    return Ok("El SkillBook está vacío.".into());
                }
                let mut out = format!("## SkillBook — versiones ({} procedimiento(s))\n\n", report.len());
                out.push_str("| id | versión | reputación | llamadas totales |\n");
                out.push_str("|---|---|---|---|\n");
                for v in &report {
                    out.push_str(&format!(
                        "| {} | {} | {:.4} | {} |\n",
                        v.id, v.version, v.reputation, v.total_calls
                    ));
                }
                Ok(out)
            }

            // ── comando desconocido ──────────────────────────────────────────
            _ => Err(format!(
                "Comando desconocido: '{}'. Comandos válidos: list | find | save | stats | remove | upgrade | versions",
                cmd
            )),
        }
    }
}
