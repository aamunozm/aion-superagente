//! Motor de FLUJOS DE TRABAJO (estilo n8n) para AION.
//!
//! Un flujo encadena PASOS, cada uno ejecuta una herramienta del agente con una entrada.
//! La salida de un paso se puede inyectar en el siguiente con el marcador `{{prev}}`.
//! Disparadores: manual, por intervalo (vida autónoma) o por evento.
//!
//! Gobernanza (fail-closed, honesto): un flujo autónomo NO ejecuta pasos sensibles
//! (los que piden confirmación humana: enviar mensajes, controlar el ratón, comprar…).
//! Esos pasos se marcan «pendiente de aprobación» y el flujo se detiene ahí, en vez de
//! actuar sin tu OK. Las herramientas de solo-lectura/cálculo corren sin fricción.
//!
//! Persistencia: `workflows.json` en el directorio de datos.

use aion_orchestrator::ToolRegistry;
use serde::{Deserialize, Serialize};

/// Qué dispara un flujo.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Solo cuando Ariel lo lanza a mano.
    #[default]
    Manual,
    /// Cada N minutos, durante la vida autónoma (cuando Ariel está inactivo).
    Interval { minutes: u64 },
    /// Cuando ocurre un evento con esta etiqueta (p. ej. "nuevo_documento").
    Event { kind: String },
}

/// Un paso del flujo: ejecuta `tool` con `input`. `{{prev}}` en `input` se sustituye
/// por la salida del paso anterior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub tool: String,
    #[serde(default)]
    pub input: String,
}

/// Un flujo de trabajo completo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub trigger: Trigger,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Última ejecución (ms epoch). Lo usa el planificador para no repetir antes de tiempo.
    #[serde(default)]
    pub last_run_ms: Option<u64>,
}

fn default_true() -> bool {
    true
}

/// Milisegundos epoch ahora (helper sin pánico).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// ¿Toca ejecutar este flujo por su disparador de intervalo? Solo aplica a `Interval`;
/// `Manual`/`Event` nunca los dispara el planificador.
pub fn is_due(wf: &Workflow, now: u64) -> bool {
    if !wf.enabled {
        return false;
    }
    match &wf.trigger {
        Trigger::Interval { minutes } => {
            let period = minutes.saturating_mul(60_000).max(60_000);
            match wf.last_run_ms {
                None => true, // nunca corrió → toca ya
                Some(last) => now.saturating_sub(last) >= period,
            }
        }
        _ => false,
    }
}

/// Resultado de ejecutar un paso.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub tool: String,
    pub input: String,
    pub output: String,
    pub ok: bool,
    /// El paso era sensible (needs_confirm) y no se ejecutó de forma autónoma.
    #[serde(default)]
    pub needs_approval: bool,
}

/// Resultado de ejecutar un flujo entero.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub workflow_id: String,
    pub steps: Vec<StepResult>,
    pub ok: bool,
    /// Se detuvo en un paso que requiere tu aprobación (gobernanza fail-closed).
    #[serde(default)]
    pub stopped_for_approval: bool,
}

/// Ejecuta un flujo de forma SECUENCIAL contra el registro de herramientas.
///
/// `allow_sensitive`: si es false (vida autónoma), los pasos que piden confirmación se
/// marcan como pendientes y el flujo se detiene ahí (no actúa sin tu OK). Si es true
/// (lanzado por Ariel desde la UI, que ya implica su intención), se ejecutan.
pub async fn run(wf: &Workflow, tools: &ToolRegistry, allow_sensitive: bool) -> WorkflowRun {
    let mut results: Vec<StepResult> = Vec::new();
    let mut prev = String::new();
    let mut ok = true;
    let mut stopped = false;

    for step in &wf.steps {
        let input = step.input.replace("{{prev}}", &prev);
        let Some(tool) = tools.get(&step.tool) else {
            results.push(StepResult {
                tool: step.tool.clone(),
                input,
                output: format!("herramienta desconocida: '{}'", step.tool),
                ok: false,
                needs_approval: false,
            });
            ok = false;
            break;
        };
        // Gobernanza: paso sensible sin permiso → se detiene pidiendo aprobación.
        if !allow_sensitive {
            if let Some(desc) = tool.needs_confirm(&input) {
                results.push(StepResult {
                    tool: step.tool.clone(),
                    input,
                    output: format!("pendiente de tu aprobación: {desc}"),
                    ok: false,
                    needs_approval: true,
                });
                stopped = true;
                break;
            }
        }
        let t0 = std::time::Instant::now();
        let res = tool.run(&input).await;
        tools.record_call(&step.tool, res.is_ok(), t0.elapsed().as_millis() as u64);
        match res {
            Ok(out) => {
                prev = out.clone();
                results.push(StepResult {
                    tool: step.tool.clone(),
                    input,
                    output: out,
                    ok: true,
                    needs_approval: false,
                });
            }
            Err(e) => {
                results.push(StepResult {
                    tool: step.tool.clone(),
                    input,
                    output: format!("error: {e}"),
                    ok: false,
                    needs_approval: false,
                });
                ok = false;
                break; // el flujo se corta en el primer fallo (semántica simple y predecible)
            }
        }
    }

    WorkflowRun {
        workflow_id: wf.id.clone(),
        steps: results,
        ok: ok && !stopped,
        stopped_for_approval: stopped,
    }
}

// ─── Persistencia ───────────────────────────────────────────────────────────

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("workflows.json")
}

pub fn load() -> Vec<Workflow> {
    match std::fs::read_to_string(path()) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save(list: &[Workflow]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(list)?;
    crate::write_atomic(&path(), &json);
    Ok(())
}

/// Inserta o reemplaza un flujo por id. Devuelve la lista resultante.
pub fn upsert(mut list: Vec<Workflow>, wf: Workflow) -> Vec<Workflow> {
    if let Some(slot) = list.iter_mut().find(|w| w.id == wf.id) {
        *slot = wf;
    } else {
        list.push(wf);
    }
    list
}

pub fn remove(list: Vec<Workflow>, id: &str) -> Vec<Workflow> {
    list.into_iter().filter(|w| w.id != id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aion_orchestrator::CalculatorTool;
    use std::sync::Arc;

    fn registry() -> ToolRegistry {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        r
    }

    #[tokio::test]
    async fn runs_sequential_and_chains_prev() {
        let wf = Workflow {
            id: "w1".into(),
            name: "cálculo encadenado".into(),
            description: String::new(),
            trigger: Trigger::Manual,
            enabled: true,
            last_run_ms: None,
            steps: vec![
                WorkflowStep {
                    tool: "calculator".into(),
                    input: "2+3".into(),
                },
                // El segundo paso usa la salida del primero (5) → 5*10 = 50.
                WorkflowStep {
                    tool: "calculator".into(),
                    input: "{{prev}}*10".into(),
                },
            ],
        };
        let run = run(&wf, &registry(), true).await;
        assert!(run.ok, "el flujo debería completarse");
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.steps[0].output, "5");
        assert_eq!(run.steps[1].input, "5*10");
        assert_eq!(run.steps[1].output, "50");
    }

    #[tokio::test]
    async fn unknown_tool_fails_cleanly() {
        let wf = Workflow {
            id: "w2".into(),
            name: "x".into(),
            description: String::new(),
            trigger: Trigger::Manual,
            enabled: true,
            last_run_ms: None,
            steps: vec![WorkflowStep {
                tool: "no_existe".into(),
                input: "".into(),
            }],
        };
        let run = run(&wf, &registry(), true).await;
        assert!(!run.ok);
        assert!(run.steps[0].output.contains("desconocida"));
    }

    #[test]
    fn interval_due_logic() {
        let mut wf = Workflow {
            id: "i".into(),
            name: "I".into(),
            description: String::new(),
            trigger: Trigger::Interval { minutes: 60 },
            enabled: true,
            last_run_ms: None,
            steps: vec![],
        };
        // Nunca corrió → toca.
        assert!(is_due(&wf, 10_000_000));
        // Corrió hace 30 min → aún no (periodo 60 min).
        wf.last_run_ms = Some(10_000_000 - 30 * 60_000);
        assert!(!is_due(&wf, 10_000_000));
        // Corrió hace 90 min → toca.
        wf.last_run_ms = Some(10_000_000 - 90 * 60_000);
        assert!(is_due(&wf, 10_000_000));
        // Manual → el planificador no lo dispara (aunque esté vencido y activo).
        let manual = Workflow {
            trigger: Trigger::Manual,
            last_run_ms: None,
            ..wf.clone()
        };
        assert!(!is_due(&manual, 10_000_000));
        // Desactivado → nunca.
        wf.enabled = false;
        assert!(!is_due(&wf, 10_000_000));
    }

    #[test]
    fn upsert_and_remove() {
        let wf = Workflow {
            id: "a".into(),
            name: "A".into(),
            description: String::new(),
            trigger: Trigger::Manual,
            enabled: true,
            last_run_ms: None,
            steps: vec![],
        };
        let list = upsert(Vec::new(), wf.clone());
        assert_eq!(list.len(), 1);
        // Reemplazo por id (no duplica).
        let list = upsert(
            list,
            Workflow {
                name: "A2".into(),
                ..wf.clone()
            },
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "A2");
        let list = remove(list, "a");
        assert!(list.is_empty());
    }
}
