//! Herramientas que dan al **agente conversacional** sus verdaderas capacidades:
//! memoria cognitiva (recordar), **auto-escritura de skills** (se escribe código a
//! sí mismo, validado en sandbox+tests) e invocación de las skills que ha forjado.
//! Esto unifica los "órganos" de AION dentro del agente con el que hablas.

use aion_evolution::{Candidate, EvolutionEngine};
use aion_kernel::traits::{GenerateRequest, LlmEngine, MemoryStore, SkillHost};
use aion_kernel::types::Message;
use aion_llm::OllamaEngine;
use aion_memory::VectorMemory;
use aion_orchestrator::Tool;
use aion_skills::{SkillManifest, WasmSkillHost};
use async_trait::async_trait;
use std::sync::Arc;

// ── 1) Recordar: el agente escribe en su memoria de largo plazo ─────────────

/// Permite al agente **persistir** algo que aprendió (memoria cognitiva activa).
pub struct RememberTool {
    memory: Arc<VectorMemory>,
}

impl RememberTool {
    pub fn new(memory: Arc<VectorMemory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }
    fn description(&self) -> &str {
        "Guarda un hecho o aprendizaje en tu memoria de largo plazo para recordarlo \
         en el futuro. Entrada: el texto a recordar."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        if input.trim().is_empty() {
            return Err("nada que recordar".into());
        }
        self.memory
            .store(input.trim())
            .await
            .map(|_| format!("recordado: «{}»", input.trim()))
            .map_err(|e| e.to_string())
    }
}

// ── 2) Forjar skill: el agente SE ESCRIBE una herramienta nueva ─────────────

/// El agente describe una capacidad numérica que necesita; AION genera un módulo
/// WASM (WAT) con el LLM, lo **valida en sandbox con tests** y, si pasa, lo
/// registra para poder usarlo. Auto-escritura segura (gated por sandbox+oráculo).
pub struct SkillForgeTool {
    engine: Arc<OllamaEngine>,
    host: Arc<WasmSkillHost>,
}

impl SkillForgeTool {
    pub fn new(engine: Arc<OllamaEngine>, host: Arc<WasmSkillHost>) -> Self {
        Self { engine, host }
    }
}

#[async_trait]
impl Tool for SkillForgeTool {
    fn name(&self) -> &str {
        "skill_forge"
    }
    fn description(&self) -> &str {
        "Crea (te escribes a ti mismo) una skill nueva que calcula sobre un entero y \
         devuelve un entero. Entrada JSON: \
         {\"name\":\"factorial\",\"description\":\"n!\",\"tests\":[[3,6],[4,24]]}. \
         La skill se valida en sandbox con esos tests antes de integrarse; si pasa, \
         queda disponible para skill_invoke."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct Spec {
            name: String,
            description: String,
            tests: Vec<(i64, i64)>,
        }
        let spec: Spec = serde_json::from_str(input.trim()).map_err(|e| {
            format!(
                "entrada inválida ({e}). Usa JSON: \
                 {{\"name\":\"...\",\"description\":\"...\",\"tests\":[[in,out],...]}}"
            )
        })?;
        if spec.tests.is_empty() {
            return Err("necesito al menos un test (entrada,salida) como oráculo".into());
        }

        let mut last = String::new();
        for _ in 0..3 {
            let prompt = format!(
                "Escribe un módulo WebAssembly en formato WAT que exporte una función `run` \
                 que reciba un i64 y devuelva un i64, implementando: {}.\n\n\
                 Ejemplo de formato VÁLIDO (esto duplica n):\n\
                 (module (func (export \"run\") (param $n i64) (result i64) \
                 (i64.mul (local.get $n) (i64.const 2))))\n\n\
                 Responde SOLO con el módulo WAT, sin explicación ni markdown.",
                spec.description
            );
            let msg = self
                .engine
                .generate(GenerateRequest {
                    messages: vec![Message::user(prompt)],
                    think: false,
                    temperature: Some(0.3),
                    max_tokens: Some(256),
                })
                .await
                .map_err(|e| e.to_string())?;
            let Some(code) = crate::extract_wat(&msg.content) else {
                last = "el LLM no produjo WAT válido".into();
                continue;
            };
            let mut evo = EvolutionEngine::new(self.host.clone());
            let report = evo
                .propose(Candidate {
                    manifest: SkillManifest {
                        name: spec.name.clone(),
                        description: spec.description.clone(),
                    },
                    code,
                    tests: spec.tests.clone(),
                })
                .await
                .map_err(|e| e.to_string())?;
            if report.accepted {
                return Ok(format!(
                    "✅ skill «{}» creada y validada ({} tests ok). Ya puedes usarla con \
                     skill_invoke. (código auto-generado, aprobado por sandbox+tests)",
                    spec.name, report.passed
                ));
            }
            last = report.reason;
        }
        Err(format!(
            "no logré crear una skill válida tras 3 intentos: {last} (el sistema queda intacto)"
        ))
    }
}

// ── 3) Invocar skill: usar una skill (semilla o forjada) ────────────────────

/// Invoca una skill registrada (incluidas las que el agente acaba de forjar).
pub struct SkillInvokeTool {
    host: Arc<WasmSkillHost>,
}

impl SkillInvokeTool {
    pub fn new(host: Arc<WasmSkillHost>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for SkillInvokeTool {
    fn name(&self) -> &str {
        "skill_invoke"
    }
    fn description(&self) -> &str {
        "Ejecuta una skill por nombre sobre un entero. Entrada: \"nombre numero\" \
         (p. ej. \"factorial 5\")."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let mut it = input.trim().splitn(2, char::is_whitespace);
        let name = it.next().unwrap_or("").trim();
        let num: i64 = it
            .next()
            .unwrap_or("")
            .trim()
            .parse()
            .map_err(|_| "uso: \"nombre numero\" (el número debe ser entero)".to_string())?;
        if name.is_empty() {
            return Err("falta el nombre de la skill".into());
        }
        let out = self
            .host
            .invoke(name, serde_json::json!(num))
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!("{} = {}", name, out.output["result"]))
    }
}
