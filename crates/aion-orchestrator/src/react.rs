//! Agente ReAct: razona y actúa en bucle (Reason + Act), usando herramientas.
//! Publica su pensamiento/acciones/observaciones en el bus de eventos del kernel.

use crate::tool::ToolRegistry;
use aion_kernel::events::{AionEvent, EventBus};
use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;
use aion_kernel::Result;

/// Resultado de una ejecución del agente.
#[derive(Debug, Clone)]
pub struct AgentRun {
    pub answer: String,
    pub steps: usize,
}

/// Agente ReAct configurable.
pub struct ReActAgent<'a> {
    engine: &'a dyn LlmEngine,
    tools: &'a ToolRegistry,
    bus: EventBus,
    max_steps: usize,
    name: String,
}

impl<'a> ReActAgent<'a> {
    pub fn new(engine: &'a dyn LlmEngine, tools: &'a ToolRegistry, bus: EventBus) -> Self {
        Self {
            engine,
            tools,
            bus,
            max_steps: 8,
            name: "aion".to_string(),
        }
    }

    pub fn with_max_steps(mut self, n: usize) -> Self {
        self.max_steps = n;
        self
    }

    fn system_prompt(&self) -> String {
        format!(
            "Eres AION, un agente que resuelve tareas razonando y usando herramientas.\n\n\
             Herramientas disponibles:\n{tools}\n\n\
             Usa EXACTAMENTE este formato, UN solo paso por respuesta:\n\
             Thought: tu razonamiento breve\n\
             Action: nombre_exacto_de_la_herramienta\n\
             Action Input: la entrada para la herramienta\n\n\
             Cuando ya tengas la solución, responde en su lugar:\n\
             Thought: tu razonamiento\n\
             Final Answer: la respuesta para el usuario\n\n\
             Reglas: no inventes 'Observation' (yo la añado). Usa la calculadora para \
             cualquier aritmética. Responde en español.",
            tools = self.tools.describe()
        )
    }

    /// Ejecuta el bucle ReAct sobre una tarea.
    pub async fn run(&self, task: &str) -> Result<AgentRun> {
        let mut scratchpad = String::new();

        for step in 0..self.max_steps {
            let user = format!(
                "Tarea: {task}\n\n{scratchpad}\n\
                 Escribe el siguiente paso (Thought + Action/Action Input, o Thought + Final Answer):"
            );
            let req = GenerateRequest {
                messages: vec![Message::system(self.system_prompt()), Message::user(user)],
                think: false,
                temperature: Some(0.2),
                max_tokens: Some(512),
            };

            let msg = self.engine.generate(req).await?;
            let text = cut_before_observation(&msg.content);

            if let Some(thought) = extract(&text, "Thought:") {
                self.bus.publish(AionEvent::ThoughtEmitted {
                    agent: self.name.clone(),
                    text: thought.clone(),
                });
            }

            // ¿Respuesta final?
            if let Some(answer) = extract(&text, "Final Answer:") {
                return Ok(AgentRun {
                    answer,
                    steps: step + 1,
                });
            }

            // ¿Acción?
            let action = extract(&text, "Action:");
            let input = extract(&text, "Action Input:").unwrap_or_default();
            let Some(action) = action else {
                // El modelo no siguió el formato: devolver lo que haya como respuesta.
                return Ok(AgentRun {
                    answer: text.trim().to_string(),
                    steps: step + 1,
                });
            };
            let action = action.lines().next().unwrap_or("").trim().to_string();

            self.bus.publish(AionEvent::ActionRequested {
                agent: self.name.clone(),
                action: format!("{action}({input})"),
            });

            let observation = match self.tools.get(&action) {
                Some(tool) => match tool.run(input.trim()).await {
                    Ok(out) => out,
                    Err(e) => format!("error de herramienta: {e}"),
                },
                None => format!("herramienta desconocida: '{action}'"),
            };

            self.bus.publish(AionEvent::ObservationReceived {
                agent: self.name.clone(),
                summary: observation.clone(),
            });

            scratchpad.push_str(&format!(
                "Thought: {}\nAction: {action}\nAction Input: {input}\nObservation: {observation}\n",
                extract(&text, "Thought:").unwrap_or_default()
            ));
        }

        // Síntesis final: agotó los pasos, pero puede que ya tenga la info en el
        // scratchpad (p. ej. tras leer una página grande). En vez de rendirse,
        // pide una respuesta final con lo recopilado.
        let synth = GenerateRequest {
            messages: vec![
                Message::system(
                    "Da la mejor respuesta final posible a la tarea usando SOLO la \
                     información ya recopilada. Responde directo, sin pedir más acciones.",
                ),
                Message::user(format!(
                    "Tarea: {task}\n\nInformación recopilada:\n{scratchpad}\n\nRespuesta final:"
                )),
            ],
            think: false,
            temperature: Some(0.4),
            max_tokens: Some(400),
        };
        match self.engine.generate(synth).await {
            Ok(m) if !m.content.trim().is_empty() => Ok(AgentRun {
                answer: m.content.trim().to_string(),
                steps: self.max_steps,
            }),
            _ => Ok(AgentRun {
                answer: "No pude completar la tarea en los pasos disponibles.".into(),
                steps: self.max_steps,
            }),
        }
    }
}

/// Extrae el texto que sigue a una etiqueta hasta el final de su sección
/// (siguiente etiqueta conocida o fin).
fn extract(text: &str, label: &str) -> Option<String> {
    let start = text.find(label)? + label.len();
    let rest = &text[start..];
    let labels = [
        "Thought:",
        "Action Input:",
        "Action:",
        "Final Answer:",
        "Observation:",
    ];
    let mut end = rest.len();
    for l in labels {
        if let Some(idx) = rest.find(l) {
            if idx < end {
                end = idx;
            }
        }
    }
    let val = rest[..end].trim().to_string();
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

/// Corta cualquier "Observation:" que el modelo haya alucinado.
fn cut_before_observation(text: &str) -> String {
    match text.find("Observation:") {
        Some(idx) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_action_and_input() {
        let t = "Thought: necesito calcular\nAction: calculator\nAction Input: 2+2";
        assert_eq!(extract(t, "Action:").unwrap(), "calculator");
        assert_eq!(extract(t, "Action Input:").unwrap(), "2+2");
    }

    #[test]
    fn extract_final_answer() {
        let t = "Thought: ya está\nFinal Answer: 42";
        assert_eq!(extract(t, "Final Answer:").unwrap(), "42");
    }

    #[test]
    fn cut_hallucinated_observation() {
        let t = "Action: calculator\nAction Input: 2+2\nObservation: 4 (inventada)";
        assert!(!cut_before_observation(t).contains("inventada"));
    }
}
