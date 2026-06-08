//! Binario `aion-core`: punto de entrada del núcleo de AION.
//!
//! Subcomandos:
//! - (sin args)         smoke test F0: telemetría + kernel + bus + salida limpia.
//! - `chat <prompt...>` F1: chat real con el LLM local (streaming de razonamiento
//!   y respuesta) usando `OllamaEngine` contra `gemma4-reason`.

mod memory_tool;
mod serve;
mod skill_tool;

use aion_kernel::traits::{GenerateRequest, LlmEngine, MemoryStore, StreamChunk};
use aion_kernel::types::Message;
use aion_kernel::{kernel_info, AionEvent, EventBus};
use aion_llm::OllamaEngine;
use aion_memory::VectorMemory;
use chrono::Utc;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    aion_telemetry::init();

    let info = kernel_info();
    tracing::info!(
        kernel = info.name,
        version = info.version,
        contract = info.contract_version,
        "núcleo AION verificado"
    );

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("chat") => {
            let prompt = args[1..].join(" ");
            run_chat(&prompt).await?;
        }
        Some("rag") => {
            let query = args[1..].join(" ");
            run_rag(&query).await?;
        }
        Some("serve") => {
            let addr = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| "127.0.0.1:8765".to_string());
            serve::run(&addr).await?;
        }
        Some("agent") => {
            let task = args[1..].join(" ");
            run_agent(&task).await?;
        }
        Some("remember") => {
            let text = args[1..].join(" ");
            run_remember(&text).await?;
        }
        Some("recall") => {
            let query = args[1..].join(" ");
            run_recall(&query).await?;
        }
        Some("skill") => {
            let n: i64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
            run_skill(n).await?;
        }
        Some("sleep") => {
            run_sleep().await?;
        }
        Some("evolve") => {
            run_evolve().await?;
        }
        _ => smoke_test(&info),
    }
    Ok(())
}

/// Smoke test de F0: bus de eventos + arranque limpio.
fn smoke_test(info: &aion_kernel::KernelInfo) {
    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    bus.publish(AionEvent::CoreStarted {
        kernel_version: info.version.to_string(),
        at: Utc::now(),
    });
    if let Ok(AionEvent::CoreStarted { kernel_version, .. }) = rx.try_recv() {
        tracing::info!(%kernel_version, "✅ AION core arrancó correctamente");
    }
    tracing::info!("smoke test F0 completado — saliendo limpio");
}

/// Chat F1: streaming de razonamiento + respuesta contra el LLM local.
async fn run_chat(prompt: &str) -> Result<(), Box<dyn std::error::Error>> {
    let engine = OllamaEngine::default_local();
    engine.health().await.map_err(|e| {
        format!("LLM local no disponible ({e}). ¿Está Ollama corriendo con gemma4-reason?")
    })?;

    tracing::info!(engine = engine.id(), "iniciando chat");
    println!("\n🧑 {prompt}\n");

    let req = GenerateRequest {
        messages: vec![Message::user(prompt)],
        think: true,
        temperature: Some(1.0),
        max_tokens: None,
    };

    stream_to_stdout(&engine, req).await?;
    Ok(())
}

/// RAG F1: indexa documentos locales, recupera contexto y responde con el LLM.
/// Port del prototipo `legacy/gemma4-reasoning/rag_demo.py`.
async fn run_rag(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let engine = OllamaEngine::default_local();
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;

    // Base de conocimiento de ejemplo (en F2 vendrá de documentos del usuario).
    let docs = [
        "AION es un super-agente de IA local-first creado por Ariel Marquez (ProntoClick, Italia).",
        "La arquitectura recomendada por defecto es un monolito modular antes que microservicios.",
        "El núcleo de AION está escrito en Rust; la UI es Next.js vía Tauri y Capacitor.",
        "El motor LLM por defecto en F1 es gemma4-reason (Gemma 4 12B abliterated) servido por Ollama.",
        "La memoria vectorial usa embeddings de nomic-embed-text con recuperación por coseno.",
        "El acento visual de AION es plasma teal; CEO-Intelligence usa dorado.",
    ];

    println!(
        "📚 Indexando {} documentos (embeddings locales)...",
        docs.len()
    );
    let memory = VectorMemory::default_local();
    for d in &docs {
        memory.store(d).await?;
    }

    let hits = memory.retrieve(query, 3).await?;
    println!("🔎 Recuperados {} fragmentos relevantes.\n", hits.len());
    let context = hits
        .iter()
        .map(|h| format!("- {}", h.content))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Usa SOLO el siguiente contexto para responder. Si no está, dilo.\n\n\
         CONTEXTO:\n{context}\n\nPREGUNTA: {query}"
    );
    println!("🧑 {query}\n");

    let req = GenerateRequest {
        messages: vec![Message::user(prompt)],
        think: false,
        temperature: Some(0.7),
        max_tokens: None,
    };
    stream_to_stdout(&engine, req).await?;
    Ok(())
}

/// Agente F2: bucle ReAct con herramientas (calculadora). Muestra los pasos
/// (pensamiento/acción/observación) en vivo vía el bus de eventos.
async fn run_agent(task: &str) -> Result<(), Box<dyn std::error::Error>> {
    use aion_orchestrator::{CalculatorTool, ReActAgent, ToolRegistry};
    use aion_skills::{SkillManifest, WasmSkillHost, SUM_TO_WAT};
    use memory_tool::MemoryTool;
    use skill_tool::SkillTool;
    use std::sync::Arc;

    let engine = OllamaEngine::default_local();
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;

    // Skills WASM en sandbox, expuestas como herramientas del agente.
    let skill_host = Arc::new(WasmSkillHost::new()?);
    skill_host.register(
        SkillManifest {
            name: "sum_to".into(),
            description: "suma 1..=n".into(),
        },
        SUM_TO_WAT,
    )?;

    // Memoria de largo plazo (persistente) como herramienta del agente.
    let memory = Arc::new(VectorMemory::persistent_local(memory_path())?);

    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(CalculatorTool));
    tools.register(Arc::new(SkillTool::new(
        skill_host,
        "sum_to",
        "Suma todos los enteros de 1 hasta n (skill WASM en sandbox). Entrada: el número n.",
    )));
    tools.register(Arc::new(MemoryTool::new(memory, 3)));

    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    // Imprime los pasos del agente en vivo.
    let printer = tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            match ev {
                AionEvent::ThoughtEmitted { text, .. } => println!("\x1b[2m🧠 {text}\x1b[0m"),
                AionEvent::ActionRequested { action, .. } => println!("\x1b[36m🔧 {action}\x1b[0m"),
                AionEvent::ObservationReceived { summary, .. } => {
                    println!("\x1b[33m👁  {summary}\x1b[0m")
                }
                _ => {}
            }
        }
    });

    println!("🧑 {task}\n");
    let agent = ReActAgent::new(&engine, &tools, bus.clone());
    let run = agent.run(task).await?;
    // da tiempo a vaciar los eventos pendientes
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    printer.abort();
    println!("\n💬 {}\n\x1b[2m[{} pasos]\x1b[0m", run.answer, run.steps);
    Ok(())
}

/// Skill F3: ejecuta una skill WASM en sandbox deny-all (suma 1..=n).
async fn run_skill(n: i64) -> Result<(), Box<dyn std::error::Error>> {
    use aion_kernel::traits::SkillHost;
    use aion_skills::{SkillManifest, WasmSkillHost, SUM_TO_WAT};

    let host = WasmSkillHost::new()?;
    host.register(
        SkillManifest {
            name: "sum_to".into(),
            description: "suma 1..=n".into(),
        },
        SUM_TO_WAT,
    )?;

    println!("🧩 skills disponibles:");
    for s in host.list().await? {
        println!("   · {s}");
    }
    println!("\n▶  invocando sum_to(n={n}) en sandbox WASM (deny-all)…");
    let out = host.invoke("sum_to", serde_json::json!({ "n": n })).await?;
    println!("✅ resultado: {}", out.output["result"]);
    println!(
        "\x1b[2m(código WASM ejecutado sin acceso a disco/red — radio de daño acotado)\x1b[0m"
    );
    Ok(())
}

/// Evolución F5: demuestra el bucle de auto-mejora gated con 3 candidatas
/// (buena, defectuosa, maliciosa) — y la verificación del kernel inmutable.
async fn run_evolve() -> Result<(), Box<dyn std::error::Error>> {
    use aion_evolution::{verify_kernel, Candidate, EvolutionEngine};
    use aion_kernel::traits::SkillHost;
    use aion_kernel::KERNEL_CONTRACT_VERSION;
    use aion_skills::{SkillManifest, WasmSkillHost};
    use std::sync::Arc;

    println!(
        "🔒 kernel inmutable: {}",
        if verify_kernel(KERNEL_CONTRACT_VERSION) {
            "íntegro ✅"
        } else {
            "ALTERADO ⛔"
        }
    );

    let live = Arc::new(WasmSkillHost::new()?);
    let mut eng = EvolutionEngine::new(live.clone());

    let mk = |name: &str, code: &str| Candidate {
        manifest: SkillManifest {
            name: name.into(),
            description: "duplica n".into(),
        },
        code: code.into(),
        tests: vec![(5, 10), (0, 0), (21, 42)],
    };

    let good = "(module (func (export \"run\") (param $n i64) (result i64) (i64.mul (local.get $n) (i64.const 2))))";
    let bad = "(module (func (export \"run\") (param $n i64) (result i64) (i64.add (local.get $n) (i64.const 1))))";
    let evil = "(module (import \"host\" \"x\" (func $x)) (func (export \"run\") (param i64) (result i64) (call $x) (local.get 0)))";

    println!("\n▶  candidata BUENA (duplica correctamente):");
    let r = eng.propose(mk("double", good)).await?;
    println!(
        "   {} — {} ({} tests ok)",
        verdict(r.accepted),
        r.reason,
        r.passed
    );

    println!("\n▶  candidata DEFECTUOSA (suma 1 en vez de duplicar):");
    let r = eng.propose(mk("double_bad", bad)).await?;
    println!("   {} — {}", verdict(r.accepted), r.reason);

    println!("\n▶  candidata MALICIOSA (intenta importar función del host):");
    let r = eng.propose(mk("evil", evil)).await?;
    println!("   {} — {}", verdict(r.accepted), r.reason);

    println!("\n🧩 skills integradas en el sistema (solo las que pasaron las puertas):");
    for s in live.list().await? {
        println!("   · {s}");
    }
    let out = live.invoke("double", serde_json::json!(7)).await?;
    println!(
        "\n✅ la skill aceptada funciona: double(7) = {}",
        out.output["result"]
    );
    Ok(())
}

fn verdict(accepted: bool) -> &'static str {
    if accepted {
        "\x1b[32mACEPTADA\x1b[0m"
    } else {
        "\x1b[31mRECHAZADA\x1b[0m"
    }
}

/// "Sueño" F4: ciclo de consolidación darwiniana de la memoria persistente.
async fn run_sleep() -> Result<(), Box<dyn std::error::Error>> {
    use aion_memory::ConsolidationConfig;
    let path = memory_path();
    let mem = VectorMemory::persistent_local(&path)?;
    println!(
        "🌙 AION entra en fase de sueño · {} recuerdos ({path})",
        mem.len()
    );
    let report = mem.consolidate(&ConsolidationConfig::default())?;
    println!("   decaimiento de aptitud aplicado");
    println!("   🔗 fusionados (casi-duplicados): {}", report.merged);
    println!("   ✂️  podados (débiles sin uso):    {}", report.pruned);
    println!(
        "☀️  despierta · {} → {} recuerdos (snapshot en {path}.bak)",
        report.before, report.after
    );
    Ok(())
}

/// Ruta del archivo de memoria persistente (configurable por AION_MEMORY).
fn memory_path() -> String {
    std::env::var("AION_MEMORY").unwrap_or_else(|_| "data/memory.jsonl".to_string())
}

/// Guarda un recuerdo en la memoria persistente (sobrevive a reinicios).
async fn run_remember(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = memory_path();
    let mem = VectorMemory::persistent_local(&path)?;
    let id = mem.store(text).await?;
    println!(
        "🧠 recordado [{}] · memoria contiene {} recuerdos · {path}",
        &id[..8],
        mem.len()
    );
    Ok(())
}

/// Recupera de la memoria persistente los recuerdos más relevantes.
async fn run_recall(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = memory_path();
    let mem = VectorMemory::persistent_local(&path)?;
    println!(
        "📂 memoria cargada: {} recuerdos ({path})\n🔎 {query}\n",
        mem.len()
    );
    let hits = mem.retrieve(query, 3).await?;
    if hits.is_empty() {
        println!("(memoria vacía)");
    }
    for h in hits {
        println!("  · ({:.2}) {}", h.score, h.content);
    }
    Ok(())
}

/// Imprime el streaming (razonamiento atenuado + respuesta) en stdout.
async fn stream_to_stdout(
    engine: &OllamaEngine,
    req: GenerateRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut in_thinking = false;
    let mut in_answer = false;
    engine
        .generate_stream(
            req,
            Box::new(move |chunk| match chunk {
                StreamChunk::Thinking { text } => {
                    if !in_thinking {
                        print!("\x1b[2m🧠 ");
                        in_thinking = true;
                    }
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
                StreamChunk::Answer { text } => {
                    if in_thinking && !in_answer {
                        print!("\x1b[0m\n\n💬 ");
                    } else if !in_answer {
                        print!("💬 ");
                    }
                    in_answer = true;
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
                StreamChunk::Done {
                    tokens,
                    tokens_per_sec,
                } => {
                    println!("\n\n\x1b[2m[{tokens} tokens · {tokens_per_sec:.1} tok/s]\x1b[0m");
                }
            }),
        )
        .await?;
    Ok(())
}
