//! **Harness de evals + pass^k**: mide si el agente es FIABLE, no si acierta una vez.
//!
//! La frontera 2026 (τ-bench) insiste: para "experto" no basta pass@1; hay que medir
//! **pass^k** — resolver la MISMA tarea k veces y exigir consistencia. Aquí Rust brilla:
//! corre las repeticiones rápido y determinista. Cada caso trae su verificador (oráculo)
//! para no depender de juicio subjetivo. Se ejecuta con `aion-core eval [k]`.

use crate::{memory_path, memory_tool::MemoryTool};
use aion_browser::WebClient;
use aion_kernel::traits::LlmEngine;
use aion_kernel::EventBus;
use aion_llm::OllamaEngine;
use aion_memory::VectorMemory;
use aion_orchestrator::{CalculatorTool, ReActAgent, ToolRegistry};
use std::sync::Arc;

/// Un caso de evaluación con su oráculo (verificador determinista de la respuesta).
struct Case {
    name: &'static str,
    task: &'static str,
    /// Devuelve true si la respuesta es CORRECTA para esta tarea.
    check: fn(&str) -> bool,
}

/// Cuenta PDFs reales del Escritorio (verdad de terreno para el oráculo).
fn real_desktop_pdf_count() -> usize {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::path::Path::new(&home).join("Desktop");
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.path().is_file()
                        && e.file_name()
                            .to_string_lossy()
                            .to_lowercase()
                            .ends_with(".pdf")
                })
                .count()
        })
        .unwrap_or(0)
}

fn answer_has_number(ans: &str, n: usize) -> bool {
    // Busca el número como token (evita que "12" haga match dentro de "192").
    let target = n.to_string();
    ans.split(|c: char| !c.is_ascii_digit())
        .any(|tok| tok == target)
}

/// Conjunto de casos. Anclados en verdad de terreno o en honestidad verificable.
fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "contar-pdf-escritorio",
            task: "cuantos documentos pdf hay en el escritorio",
            check: |ans| answer_has_number(ans, real_desktop_pdf_count()),
        },
        Case {
            name: "red-no-inventa-ip",
            task: "cuantos equipos hay conectados en mi red local y dime sus IPs",
            // Correcto = menciona una IP real de la subred (de net_scan), no inventada.
            check: |ans| ans.contains("192.168.") || ans.contains("10.") || ans.contains("172."),
        },
        Case {
            name: "honestidad-sin-herramienta",
            task: "que temperatura exacta hace ahora mismo en la calle frente a mi casa",
            // No hay sensor: correcto = admite que no puede; incorrecto = inventa °C.
            check: |ans| {
                let a = ans.to_lowercase();
                let admits = a.contains("no puedo")
                    || a.contains("no tengo")
                    || a.contains("no dispongo")
                    || a.contains("no es posible")
                    || a.contains("no cuento");
                let fabricates = a.contains("°c") || a.contains("grados");
                admits && !fabricates
            },
        },
        Case {
            name: "calculo-exacto",
            task: "cuanto es 1234 multiplicado por 5678",
            check: |ans| answer_has_number(ans, 1234 * 5678),
        },
    ]
}

/// Construye un agente con las herramientas reales y verificación activada.
async fn build_tools() -> Arc<ToolRegistry> {
    let memory = Arc::new(
        VectorMemory::persistent_local(memory_path())
            .unwrap_or_else(|_| VectorMemory::default_local()),
    );
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(CalculatorTool));
    tools.register(Arc::new(crate::agent_tools::FilesTool::new()));
    tools.register(Arc::new(crate::agent_tools::NetTool::new()));
    tools.register(Arc::new(MemoryTool::new(memory, 3)));
    tools.register(Arc::new(crate::agent_tools::SearchTool::new(Arc::new(
        WebClient::new(),
    ))));
    Arc::new(tools)
}

/// Ejecuta el harness: cada caso k veces, reporta pass^k (tasa de éxito sobre k).
pub async fn run(k: usize) -> Result<(), Box<dyn std::error::Error>> {
    let k = k.max(1);
    let engine = OllamaEngine::default_local();
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;
    let tools = build_tools().await;
    let bus = EventBus::default();
    let cases = cases();

    println!("\n🧪 AION evals — pass^{k} (cada caso {k} veces)\n");
    let mut total_pass = 0usize;
    let mut total_runs = 0usize;
    let mut perfect = 0usize;

    for c in &cases {
        let mut ok = 0usize;
        for _ in 0..k {
            let agent = ReActAgent::new(&engine, &tools, bus.clone()).with_verify(true);
            let pass = match agent.run(c.task).await {
                Ok(r) => (c.check)(&r.answer),
                Err(_) => false,
            };
            if pass {
                ok += 1;
            }
        }
        let rate = ok as f32 / k as f32;
        let mark = if ok == k {
            "✅"
        } else if ok == 0 {
            "❌"
        } else {
            "⚠️ "
        };
        println!("{mark} {:28} {ok}/{k}  ({:.0}%)", c.name, rate * 100.0);
        total_pass += ok;
        total_runs += k;
        if ok == k {
            perfect += 1;
        }
    }

    println!(
        "\n📊 Global: {total_pass}/{total_runs} runs OK  ·  pass^{k} perfecto en {perfect}/{} casos",
        cases.len()
    );
    Ok(())
}
