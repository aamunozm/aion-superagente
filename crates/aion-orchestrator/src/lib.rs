//! # aion-orchestrator
//!
//! Orquestador propio de AION (patrón AutoAgents): bucle **ReAct** (Reason+Act)
//! con herramientas, publicando en el bus de eventos pub/sub del kernel.
//!
//! - F2 (actual): [`ReActAgent`] + [`ToolRegistry`] + herramientas nativas
//!   (calculadora determinista). Inspira el bucle cognitivo de 4 fases (GWA).
//! - F3: las skills WASM (Extism) se exponen como [`Tool`].
//! - F4: integración con curiosidad (MAGELLAN) y memoria darwiniana.

mod calc;
mod crew;
mod react;
mod tool;

pub use crew::{CrewRun, Orchestrator, Role, Step, ROLES};
pub use react::{honesty_guard, AgentRun, AskFn, ConfirmFn, ReActAgent, HONEST_REFUSAL};
pub use tool::{CalculatorTool, Tool, ToolCategory, ToolRegistry};
