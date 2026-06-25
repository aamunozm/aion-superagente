//! **Motor de flujos por GRAFO (DAG)** — la evolución de [`crate::workflow`] (lineal) a un grafo de
//! nodos conectados por aristas, con RAMAS y CONDICIONES. Es la base nativa del editor visual
//! (React Flow, MIT) que reemplaza al sistema lineal y a la idea de embeber n8n (ver memoria
//! `aion`: licencia OEM + infra Node/PG/Redis lo descartan; nativo = local-first y on-brand).
//!
//! Diseño:
//! - **Nodo** ([`Node`]) con `kind`: `trigger` (entrada), `action` (ejecuta una tool del agente),
//!   `condition` (bifurca según el valor entrante). Lleva posición `x,y` para el lienzo.
//! - **Arista** ([`Edge`]) `from → to` con etiqueta opcional `when` (`ok`/`err` tras una acción,
//!   `true`/`false` tras una condición). Sin etiqueta = siempre.
//! - **Ejecución**: desde el trigger, se sigue el grafo pasando el valor por las aristas; cada
//!   acción sustituye `{{in}}` por el valor entrante. Tope de pasos = anti-bucles.
//! - **Gobernanza** (igual que el motor lineal, fail-closed): en modo autónomo, una acción sensible
//!   (needs_confirm) NO se ejecuta: el flujo se detiene pidiendo tu aprobación.
//!
//! Persistencia: `flows.json` en el directorio de datos. Migración 1:1 desde los `Workflow`
//! lineales con [`from_workflow`].

use aion_orchestrator::ToolRegistry;
use serde::{Deserialize, Serialize};

/// Tipo de disparador del flujo (idéntico semánticamente al motor lineal).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerKind {
    #[default]
    Manual,
    Interval {
        minutes: u64,
    },
    Event {
        kind: String,
    },
}

/// Qué hace un nodo. `trigger` es el punto de entrada; `action` ejecuta una herramienta;
/// `condition` bifurca el flujo según el valor que le llega.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKind {
    /// Entrada del flujo.
    Trigger { trigger: TriggerKind },
    /// Ejecuta `tool` con `input` (en `input`, `{{in}}` = valor entrante de la arista).
    Action {
        tool: String,
        #[serde(default)]
        input: String,
    },
    /// Bifurca: evalúa un predicado simple sobre el valor entrante y enruta por `true`/`false`.
    /// `test`: `ok` (la entrada no es un error) · `nonempty` · `contains:TEXTO` · `equals:TEXTO`.
    Condition {
        #[serde(default = "default_test")]
        test: String,
    },
}

fn default_test() -> String {
    "nonempty".into()
}

/// Un nodo del lienzo: identidad + tipo + posición visual + título opcional.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(flatten)]
    pub kind: NodeKind,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

/// Una arista dirigida `from → to`. `when` etiqueta la salida: tras una acción `ok`/`err`; tras una
/// condición `true`/`false`; vacío = se sigue siempre.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub when: String,
}

/// Un flujo completo (grafo).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Flow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub last_run_ms: Option<u64>,
}

fn yes() -> bool {
    true
}

/// El trigger del flujo (primer nodo `Trigger`).
pub fn trigger_node(flow: &Flow) -> Option<&Node> {
    flow.nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Trigger { .. }))
}

/// ¿Toca ejecutar por intervalo? (igual que el motor lineal; solo `Interval`).
pub fn is_due(flow: &Flow, now_ms: u64) -> bool {
    if !flow.enabled {
        return false;
    }
    match trigger_node(flow).map(|n| &n.kind) {
        Some(NodeKind::Trigger {
            trigger: TriggerKind::Interval { minutes },
        }) => {
            let period = minutes.saturating_mul(60_000).max(60_000);
            match flow.last_run_ms {
                None => true,
                Some(last) => now_ms.saturating_sub(last) >= period,
            }
        }
        _ => false,
    }
}

/// Resultado de un nodo ejecutado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResult {
    pub node_id: String,
    pub tool: String,
    pub input: String,
    pub output: String,
    pub ok: bool,
    #[serde(default)]
    pub needs_approval: bool,
}

/// Resultado de ejecutar el grafo entero.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowRun {
    pub flow_id: String,
    pub steps: Vec<NodeResult>,
    pub ok: bool,
    #[serde(default)]
    pub stopped_for_approval: bool,
}

/// Evalúa el predicado de una condición sobre el valor entrante.
fn eval_test(test: &str, value: &str, last_ok: bool) -> bool {
    let t = test.trim();
    if t == "ok" {
        return last_ok;
    }
    if t == "nonempty" {
        return !value.trim().is_empty();
    }
    if let Some(needle) = t.strip_prefix("contains:") {
        return value.to_lowercase().contains(&needle.trim().to_lowercase());
    }
    if let Some(expected) = t.strip_prefix("equals:") {
        return value.trim().eq_ignore_ascii_case(expected.trim());
    }
    // Predicado desconocido → no bifurca (seguro).
    false
}

/// Aristas salientes de un nodo que coinciden con la etiqueta `label` (o sin etiqueta = siempre).
fn next_edges<'a>(flow: &'a Flow, from: &str, label: &str) -> Vec<&'a Edge> {
    flow.edges
        .iter()
        .filter(|e| e.from == from && (e.when.is_empty() || e.when.eq_ignore_ascii_case(label)))
        .collect()
}

/// Ejecuta el grafo desde su trigger, siguiendo las aristas y pasando el valor de salida de cada
/// nodo al siguiente (`{{in}}`). Tope de pasos = anti-bucles. Gobernanza fail-closed igual que el
/// motor lineal: en autónomo, una acción sensible detiene el flujo pidiendo aprobación.
pub async fn run(flow: &Flow, tools: &ToolRegistry, allow_sensitive: bool) -> FlowRun {
    let mut steps: Vec<NodeResult> = Vec::new();
    let mut ok = true;
    let mut stopped = false;

    let Some(trigger) = trigger_node(flow) else {
        return FlowRun {
            flow_id: flow.id.clone(),
            steps,
            ok: false,
            stopped_for_approval: false,
        };
    };

    // Frontera de ejecución: (nodo, valor entrante). Empezamos por lo que sigue al trigger.
    let mut frontier: Vec<(String, String)> = next_edges(flow, &trigger.id, "")
        .into_iter()
        .map(|e| (e.to.clone(), String::new()))
        .collect();

    let max_steps = flow.nodes.len().saturating_mul(4).max(16);
    let mut budget = max_steps;

    while let Some((node_id, incoming)) = frontier.pop() {
        if budget == 0 {
            break; // anti-bucles
        }
        budget -= 1;
        let Some(node) = flow.nodes.iter().find(|n| n.id == node_id) else {
            continue;
        };
        match &node.kind {
            NodeKind::Trigger { .. } => {
                // Un trigger alcanzado por arista: solo propaga.
                for e in next_edges(flow, &node.id, "") {
                    frontier.push((e.to.clone(), incoming.clone()));
                }
            }
            NodeKind::Action { tool, input } => {
                let resolved = input.replace("{{in}}", &incoming);
                let Some(t) = tools.get(tool) else {
                    steps.push(NodeResult {
                        node_id: node.id.clone(),
                        tool: tool.clone(),
                        input: resolved,
                        output: format!("herramienta desconocida: '{tool}'"),
                        ok: false,
                        needs_approval: false,
                    });
                    ok = false;
                    continue;
                };
                if !allow_sensitive {
                    if let Some(desc) = t.needs_confirm(&resolved) {
                        steps.push(NodeResult {
                            node_id: node.id.clone(),
                            tool: tool.clone(),
                            input: resolved,
                            output: format!("pendiente de tu aprobación: {desc}"),
                            ok: false,
                            needs_approval: true,
                        });
                        stopped = true;
                        continue; // no seguimos esta rama, pero otras ramas pendientes siguen
                    }
                }
                let t0 = std::time::Instant::now();
                let res = t.run(&resolved).await;
                tools.record_call(tool, res.is_ok(), t0.elapsed().as_millis() as u64);
                let (out, good) = match res {
                    Ok(o) => (o, true),
                    Err(e) => (format!("error: {e}"), false),
                };
                steps.push(NodeResult {
                    node_id: node.id.clone(),
                    tool: tool.clone(),
                    input: resolved,
                    output: out.clone(),
                    ok: good,
                    needs_approval: false,
                });
                if !good {
                    ok = false;
                }
                // Enruta por ok/err; si no hay aristas etiquetadas, las sin etiqueta sirven de «ok».
                let label = if good { "ok" } else { "err" };
                for e in next_edges(flow, &node.id, label) {
                    frontier.push((e.to.clone(), out.clone()));
                }
            }
            NodeKind::Condition { test } => {
                let last_ok = !incoming.starts_with("error:");
                let branch = eval_test(test, &incoming, last_ok);
                let label = if branch { "true" } else { "false" };
                steps.push(NodeResult {
                    node_id: node.id.clone(),
                    tool: format!("condition:{test}"),
                    input: incoming.clone(),
                    output: label.to_string(),
                    ok: true,
                    needs_approval: false,
                });
                for e in next_edges(flow, &node.id, label) {
                    frontier.push((e.to.clone(), incoming.clone()));
                }
            }
        }
    }

    FlowRun {
        flow_id: flow.id.clone(),
        steps,
        ok: ok && !stopped,
        stopped_for_approval: stopped,
    }
}

// ─── Migración desde el motor lineal ──────────────────────────────────────────

/// Convierte un [`crate::workflow::Workflow`] lineal en un [`Flow`] de grafo equivalente: un nodo
/// trigger + una cadena de nodos acción encadenados (`{{prev}}` → `{{in}}`). Así no se pierde nada
/// de lo ya creado al pasar al editor visual.
pub fn from_workflow(wf: &crate::workflow::Workflow) -> Flow {
    let trig = match &wf.trigger {
        crate::workflow::Trigger::Manual => TriggerKind::Manual,
        crate::workflow::Trigger::Interval { minutes } => {
            TriggerKind::Interval { minutes: *minutes }
        }
        crate::workflow::Trigger::Event { kind } => TriggerKind::Event { kind: kind.clone() },
    };
    let mut nodes = vec![Node {
        id: "trigger".into(),
        kind: NodeKind::Trigger { trigger: trig },
        title: "Inicio".into(),
        x: 0.0,
        y: 0.0,
    }];
    let mut edges = Vec::new();
    let mut prev = "trigger".to_string();
    for (i, step) in wf.steps.iter().enumerate() {
        let nid = format!("n{i}");
        nodes.push(Node {
            id: nid.clone(),
            kind: NodeKind::Action {
                tool: step.tool.clone(),
                input: step.input.replace("{{prev}}", "{{in}}"),
            },
            title: step.tool.clone(),
            x: 0.0,
            y: (i as f64 + 1.0) * 120.0,
        });
        edges.push(Edge {
            id: format!("e{i}"),
            from: prev.clone(),
            to: nid.clone(),
            when: String::new(),
        });
        prev = nid;
    }
    Flow {
        id: wf.id.clone(),
        name: wf.name.clone(),
        description: wf.description.clone(),
        nodes,
        edges,
        enabled: wf.enabled,
        last_run_ms: wf.last_run_ms,
    }
}

// ─── Persistencia ─────────────────────────────────────────────────────────────

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("flows.json")
}

pub fn load() -> Vec<Flow> {
    match std::fs::read_to_string(path()) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save(list: &[Flow]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(list)?;
    crate::write_atomic(&path(), &json);
    Ok(())
}

pub fn upsert(mut list: Vec<Flow>, flow: Flow) -> Vec<Flow> {
    if let Some(slot) = list.iter_mut().find(|f| f.id == flow.id) {
        *slot = flow;
    } else {
        list.push(flow);
    }
    list
}

pub fn remove(list: Vec<Flow>, id: &str) -> Vec<Flow> {
    list.into_iter().filter(|f| f.id != id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aion_orchestrator::{CalculatorTool, ToolRegistry};
    use std::sync::Arc;

    fn registry() -> ToolRegistry {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        r
    }

    fn node(id: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
            kind,
            title: String::new(),
            x: 0.0,
            y: 0.0,
        }
    }
    fn edge(id: &str, from: &str, to: &str, when: &str) -> Edge {
        Edge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            when: when.into(),
        }
    }

    #[tokio::test]
    async fn grafo_lineal_encadena_valores() {
        // trigger → (2+3=5) → ({{in}}*10=50)
        let flow = Flow {
            id: "f1".into(),
            name: "cadena".into(),
            description: String::new(),
            enabled: true,
            last_run_ms: None,
            nodes: vec![
                node(
                    "t",
                    NodeKind::Trigger {
                        trigger: TriggerKind::Manual,
                    },
                ),
                node(
                    "a",
                    NodeKind::Action {
                        tool: "calculator".into(),
                        input: "2+3".into(),
                    },
                ),
                node(
                    "b",
                    NodeKind::Action {
                        tool: "calculator".into(),
                        input: "{{in}}*10".into(),
                    },
                ),
            ],
            edges: vec![edge("e1", "t", "a", ""), edge("e2", "a", "b", "ok")],
        };
        let run = run(&flow, &registry(), true).await;
        assert!(run.ok, "{:?}", run.steps);
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.steps[0].output, "5");
        assert_eq!(run.steps[1].input, "5*10");
        assert_eq!(run.steps[1].output, "50");
    }

    #[tokio::test]
    async fn condicion_bifurca_por_contenido() {
        // trigger → acción(=5) → condición(contains:5) → rama true (acción que duplica)
        let flow = Flow {
            id: "f2".into(),
            name: "bifurca".into(),
            description: String::new(),
            enabled: true,
            last_run_ms: None,
            nodes: vec![
                node(
                    "t",
                    NodeKind::Trigger {
                        trigger: TriggerKind::Manual,
                    },
                ),
                node(
                    "a",
                    NodeKind::Action {
                        tool: "calculator".into(),
                        input: "2+3".into(),
                    },
                ),
                node(
                    "c",
                    NodeKind::Condition {
                        test: "contains:5".into(),
                    },
                ),
                node(
                    "yes",
                    NodeKind::Action {
                        tool: "calculator".into(),
                        input: "{{in}}*2".into(),
                    },
                ),
                node(
                    "no",
                    NodeKind::Action {
                        tool: "calculator".into(),
                        input: "0".into(),
                    },
                ),
            ],
            edges: vec![
                edge("e1", "t", "a", ""),
                edge("e2", "a", "c", "ok"),
                edge("e3", "c", "yes", "true"),
                edge("e4", "c", "no", "false"),
            ],
        };
        let run = run(&flow, &registry(), true).await;
        // Tomó la rama true: 5*2 = 10. La rama false (0) no se ejecutó.
        let outs: Vec<&str> = run.steps.iter().map(|s| s.output.as_str()).collect();
        assert!(outs.contains(&"10"), "esperaba 10 en {outs:?}");
        assert!(
            !outs.contains(&"0"),
            "la rama false NO debía correr: {outs:?}"
        );
    }

    #[test]
    fn migra_workflow_lineal_a_grafo() {
        use crate::workflow::{Trigger, Workflow, WorkflowStep};
        let wf = Workflow {
            id: "w".into(),
            name: "lineal".into(),
            description: String::new(),
            trigger: Trigger::Interval { minutes: 30 },
            enabled: true,
            last_run_ms: None,
            steps: vec![
                WorkflowStep {
                    tool: "calculator".into(),
                    input: "1+1".into(),
                },
                WorkflowStep {
                    tool: "calculator".into(),
                    input: "{{prev}}+1".into(),
                },
            ],
        };
        let f = from_workflow(&wf);
        // trigger + 2 acciones, 2 aristas; {{prev}} migrado a {{in}}.
        assert_eq!(f.nodes.len(), 3);
        assert_eq!(f.edges.len(), 2);
        assert!(matches!(
            trigger_node(&f).unwrap().kind,
            NodeKind::Trigger {
                trigger: TriggerKind::Interval { minutes: 30 }
            }
        ));
        let has_in = f
            .nodes
            .iter()
            .any(|n| matches!(&n.kind, NodeKind::Action { input, .. } if input.contains("{{in}}")));
        assert!(has_in, "el {{prev}} debe migrarse a {{in}}");
    }

    #[test]
    fn upsert_remove_y_due() {
        let f = Flow {
            id: "a".into(),
            name: "A".into(),
            enabled: true,
            nodes: vec![node(
                "t",
                NodeKind::Trigger {
                    trigger: TriggerKind::Interval { minutes: 60 },
                },
            )],
            ..Default::default()
        };
        let list = upsert(Vec::new(), f.clone());
        assert_eq!(list.len(), 1);
        let list = upsert(
            list,
            Flow {
                name: "A2".into(),
                ..f.clone()
            },
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "A2");
        // Nunca corrió → toca.
        assert!(is_due(&list[0], 10_000_000));
        let list = remove(list, "a");
        assert!(list.is_empty());
    }
}
