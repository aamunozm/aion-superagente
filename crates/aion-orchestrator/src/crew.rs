//! **Equipo multiagente** (jerarquía orquestador → especialistas).
//!
//! Un `Orchestrator` recibe una tarea, la **descompone** y **delega** cada parte
//! en el especialista más idóneo (investigador, programador, analista, redactor),
//! pasando el resultado de unos a otros (**colaboración**), y finalmente
//! **sintetiza** la respuesta del equipo. Toda la actividad se publica en el
//! `EventBus`, así que la jerarquía y la comunicación entre agentes son visibles.

use crate::react::ReActAgent;
use crate::tool::ToolRegistry;
use aion_kernel::events::{AionEvent, EventBus};
use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;
use aion_kernel::Result;

/// Un rol especialista: nombre + persona (cómo se comporta y qué herramientas usa).
pub struct Role {
    pub name: &'static str,
    pub persona: &'static str,
}

/// Plantilla de especialistas disponibles en el equipo.
pub const ROLES: &[Role] = &[
    Role {
        name: "investigador",
        persona: "Eres el INVESTIGADOR del equipo. Usa web_search y web_fetch para encontrar y \
                  verificar información actual; cita de dónde la sacas. Sé riguroso.",
    },
    Role {
        name: "programador",
        persona: "Eres el PROGRAMADOR del equipo. Si falta una capacidad de cálculo, créala con \
                  skill_forge (WASM en sandbox) y úsala con skill_invoke. Precisión total.",
    },
    Role {
        name: "analista",
        persona: "Eres el ANALISTA del equipo. Razona paso a paso, compara alternativas, evalúa \
                  riesgos y saca conclusiones fundamentadas.",
    },
    Role {
        name: "redactor",
        persona: "Eres el REDACTOR del equipo. Sintetiza el trabajo de los demás en una respuesta \
                  clara, completa y bien estructurada para el usuario.",
    },
];

fn persona_for(role: &str) -> &'static str {
    ROLES
        .iter()
        .find(|r| r.name == role)
        .map(|r| r.persona)
        .unwrap_or("Eres un especialista del equipo. Resuelve tu parte con rigor.")
}

/// Un paso del plan: qué especialista hace qué.
#[derive(Debug, Clone)]
pub struct Step {
    pub role: String,
    pub subtask: String,
}

/// Resultado de una ejecución del equipo.
#[derive(Debug, Clone)]
pub struct CrewRun {
    pub answer: String,
    pub steps: usize,
    /// Acciones fallidas de TODOS los especialistas (honestidad del self-model:
    /// un equipo que tropezó no debe puntuar como éxito limpio).
    pub failures: Vec<String>,
}

/// Orquestador del equipo: planifica, delega en especialistas y sintetiza.
pub struct Orchestrator<'a> {
    engine: &'a dyn LlmEngine,
    tools: &'a ToolRegistry,
    bus: EventBus,
    max_steps: usize,
}

impl<'a> Orchestrator<'a> {
    pub fn new(engine: &'a dyn LlmEngine, tools: &'a ToolRegistry, bus: EventBus) -> Self {
        Self {
            engine,
            tools,
            bus,
            max_steps: 4,
        }
    }

    fn say(&self, text: impl Into<String>) {
        self.bus.publish(AionEvent::ThoughtEmitted {
            agent: "orquestador".into(),
            text: text.into(),
        });
    }

    /// Ejecuta la tarea con el equipo completo.
    pub async fn run(&self, task: &str) -> Result<CrewRun> {
        self.say("Planificando y asignando especialistas…");
        let plan = self.plan(task).await;
        self.say(format!(
            "Plan: {} paso(s) → {}",
            plan.len(),
            plan.iter()
                .map(|s| s.role.as_str())
                .collect::<Vec<_>>()
                .join(" → ")
        ));

        // DELEGACIÓN secuencial con COLABORACIÓN: cada especialista ve lo que han
        // hecho los anteriores (memoria compartida del equipo).
        let mut shared = String::new();
        let mut failures: Vec<String> = Vec::new();
        for (i, step) in plan.iter().enumerate() {
            self.say(format!("→ Delego en «{}»: {}", step.role, step.subtask));
            let persona = persona_for(&step.role);
            let ctx = if shared.is_empty() {
                persona.to_string()
            } else {
                format!("{persona}\n\nTrabajo previo del equipo (úsalo):\n{shared}")
            };
            let agent = ReActAgent::new(self.engine, self.tools, self.bus.clone())
                .with_name(&step.role)
                .with_context(ctx)
                .with_max_steps(6);
            let run = agent.run(&step.subtask).await?;
            shared.push_str(&format!("[{}] {}\n\n", step.role, run.answer));
            failures.extend(run.failures.iter().map(|f| format!("[{}] {f}", step.role)));
            let _ = i;
        }

        // SÍNTESIS final por el orquestador.
        self.say("Sintetizando el trabajo del equipo…");
        let answer = self.synthesize(task, &shared).await;
        Ok(CrewRun {
            answer,
            steps: plan.len(),
            failures,
        })
    }

    /// Descompone la tarea y asigna un rol a cada paso (vía LLM, con fallback).
    async fn plan(&self, task: &str) -> Vec<Step> {
        let prompt = format!(
            "Eres el orquestador de un equipo de agentes. Roles disponibles: investigador, \
             programador, analista, redactor.\n\nTarea: {task}\n\n\
             Divídela en 1 a {} pasos. Asigna a cada paso UN rol. El último paso suele ser del \
             redactor para sintetizar. Responde SOLO un array JSON: \
             [{{\"role\":\"investigador\",\"subtask\":\"...\"}}]",
            self.max_steps
        );
        let resp = self
            .engine
            .generate(GenerateRequest {
                messages: vec![Message::user(prompt)],
                think: false,
                temperature: Some(0.3),
                max_tokens: Some(400),
            })
            .await;
        if let Ok(msg) = resp {
            if let Some(steps) = parse_plan(&msg.content) {
                if !steps.is_empty() {
                    return steps;
                }
            }
        }
        // Fallback: un analista resuelve y el redactor sintetiza.
        vec![
            Step {
                role: "analista".into(),
                subtask: task.into(),
            },
            Step {
                role: "redactor".into(),
                subtask: format!("Redacta la respuesta final para: {task}"),
            },
        ]
    }

    async fn synthesize(&self, task: &str, shared: &str) -> String {
        let prompt = format!(
            "Tarea original: {task}\n\nTrabajo del equipo:\n{shared}\n\n\
             Sintetiza una respuesta final clara y completa para el usuario, integrando lo mejor \
             de cada especialista. No menciones el proceso interno."
        );
        match self
            .engine
            .generate(GenerateRequest {
                messages: vec![Message::user(prompt)],
                think: false,
                temperature: Some(0.5),
                max_tokens: Some(700),
            })
            .await
        {
            Ok(m) if !m.content.trim().is_empty() => m.content.trim().to_string(),
            _ => shared.trim().to_string(),
        }
    }
}

/// Parsea el array JSON del plan, tolerante a texto alrededor.
fn parse_plan(text: &str) -> Option<Vec<Step>> {
    let start = text.find('[')?;
    let end = text.rfind(']')? + 1;
    let json = &text[start..end];
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = v.as_array()?;
    let valid = ["investigador", "programador", "analista", "redactor"];
    let steps: Vec<Step> = arr
        .iter()
        .filter_map(|s| {
            let role = s.get("role")?.as_str()?.to_lowercase();
            let role = if valid.contains(&role.as_str()) {
                role
            } else {
                "analista".into()
            };
            let subtask = s.get("subtask")?.as_str()?.to_string();
            Some(Step { role, subtask })
        })
        .collect();
    if steps.is_empty() {
        None
    } else {
        Some(steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plan_json() {
        let t = r#"Aquí está: [{"role":"investigador","subtask":"buscar X"},{"role":"redactor","subtask":"redactar"}] fin"#;
        let steps = parse_plan(t).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].role, "investigador");
        assert_eq!(steps[1].role, "redactor");
    }

    #[test]
    fn invalid_role_falls_back_to_analista() {
        let t = r#"[{"role":"hacker","subtask":"x"}]"#;
        assert_eq!(parse_plan(t).unwrap()[0].role, "analista");
    }
}
