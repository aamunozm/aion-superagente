//! Binario `aion-core`: punto de entrada del núcleo de AION.
//!
//! Subcomandos:
//! - (sin args)         smoke test F0: telemetría + kernel + bus + salida limpia.
//! - `chat <prompt...>` F1: chat real con el LLM local (streaming de razonamiento
//!   y respuesta) usando `OllamaEngine` contra `gemma4-reason`.

mod serve;

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
    use std::sync::Arc;

    let engine = OllamaEngine::default_local();
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;

    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(CalculatorTool));

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
