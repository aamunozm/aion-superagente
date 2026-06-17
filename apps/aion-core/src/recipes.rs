//! **Recetas (macros)** — Facultad 4, vía SEGURA. AION compone una secuencia de sus
//! herramientas YA existentes en una "receta" con nombre, reutilizable y parametrizable.
//! Adquiere capacidad nueva (combinaciones) SIN escribir código de bajo nivel ni abrir
//! superficie de I/O nueva: cada paso usa una herramienta que ya tiene su propia gobernanza.
//!
//! GOBERNANZA (clave): una receta NUNCA ejecuta una herramienta que requiere tu
//! confirmación (HITL) — eso evitaría que una macro burle tu OK. Se comprueba al GUARDAR
//! (sobre la plantilla) y, fail-closed, también al EJECUTAR (sobre la entrada real).
//!
//! Append/overwrite en `recipes.jsonl`. Todo barato (lectura de disco); ejecutar una
//! receta solo encadena herramientas que ya existen.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Un paso de la receta: una herramienta existente + su plantilla de entrada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Nombre EXACTO de una herramienta ya registrada.
    pub tool: String,
    /// Plantilla de entrada: admite `{{nombre}}` (un parámetro de la receta) y `{{N}}`
    /// (la salida del paso N, 1-indexado). Se sustituyen al ejecutar.
    pub input: String,
}

/// Una receta: secuencia con nombre de pasos, reutilizable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String,
    pub description: String,
    pub steps: Vec<Step>,
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("recipes.jsonl")
}

pub fn all() -> Vec<Recipe> {
    std::fs::read_to_string(path())
        .ok()
        .map(|t| {
            t.lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn get(name: &str) -> Option<Recipe> {
    let n = name.trim();
    all().into_iter().find(|r| r.name.eq_ignore_ascii_case(n))
}

fn save_all(items: &[Recipe]) {
    let body: String = items
        .iter()
        .filter_map(|r| serde_json::to_string(r).ok())
        .map(|l| l + "\n")
        .collect();
    crate::write_atomic(&path(), &body);
}

/// Añade o reemplaza una receta por nombre (case-insensitive).
pub fn upsert(r: Recipe) {
    let mut v = all();
    v.retain(|x| !x.name.eq_ignore_ascii_case(&r.name));
    v.push(r);
    save_all(&v);
}

/// Catálogo `(nombre, descripción)` para inyectar al contexto del agente.
pub fn catalog() -> Vec<(String, String)> {
    all().into_iter().map(|r| (r.name, r.description)).collect()
}

/// Sustituye `{{nombre}}` (parámetros) y `{{N}}` (salida del paso N, 1-indexado) en una
/// plantilla. Se construyen los marcadores por concatenación para no pelear con el escape
/// de llaves de `format!`.
pub fn fill(template: &str, params: &HashMap<String, String>, outputs: &[String]) -> String {
    let mut out = template.to_string();
    for (k, v) in params {
        out = out.replace(&["{{", k.as_str(), "}}"].concat(), v);
    }
    for (i, o) in outputs.iter().enumerate() {
        out = out.replace(&["{{", &(i + 1).to_string(), "}}"].concat(), o);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_substitutes_params_and_step_outputs() {
        let mut p = HashMap::new();
        p.insert("ciudad".to_string(), "Milán".to_string());
        let outs = vec!["RESULTADO-1".to_string()];
        assert_eq!(
            fill("clima en {{ciudad}} -> {{1}}", &p, &outs),
            "clima en Milán -> RESULTADO-1"
        );
    }

    #[test]
    fn fill_leaves_unknown_markers_untouched() {
        let p = HashMap::new();
        assert_eq!(fill("hola {{x}}", &p, &[]), "hola {{x}}");
    }
}
