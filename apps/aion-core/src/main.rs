//! Binario `aion-core`: punto de entrada del núcleo de AION.
//!
//! Subcomandos:
//! - (sin args)         smoke test F0: telemetría + kernel + bus + salida limpia.
//! - `chat <prompt...>` F1: chat real con el LLM local (streaming de razonamiento
//!   y respuesta) usando `OllamaEngine` contra `gemma4-reason`.

mod memory_tool;
mod serve;
mod skill_tool;
mod web_tool;

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
        Some("self-evolve") => {
            run_self_evolve().await?;
        }
        Some("history") => {
            run_history()?;
        }
        Some("audit") => {
            run_audit();
        }
        Some("cognition") => {
            run_cognition();
        }
        Some("live") => {
            let cycles: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2);
            run_live(cycles).await?;
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
    use aion_browser::WebClient;
    use aion_orchestrator::{CalculatorTool, ReActAgent, ToolRegistry};
    use aion_skills::{SkillManifest, WasmSkillHost, SUM_TO_WAT};
    use memory_tool::MemoryTool;
    use skill_tool::SkillTool;
    use std::sync::Arc;
    use web_tool::WebTool;

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
    tools.register(Arc::new(WebTool::new(Arc::new(WebClient::new()))));

    let audit = Arc::new(aion_telemetry::AuditLog::default_local());
    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    // Imprime los pasos del agente en vivo y audita las acciones.
    let audit_p = audit.clone();
    let printer = tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            match ev {
                AionEvent::ThoughtEmitted { text, .. } => println!("\x1b[2m🧠 {text}\x1b[0m"),
                AionEvent::ActionRequested { action, .. } => {
                    println!("\x1b[36m🔧 {action}\x1b[0m");
                    audit_p.record("agent", "tool_call", action);
                }
                AionEvent::ObservationReceived { summary, .. } => {
                    println!("\x1b[33m👁  {summary}\x1b[0m")
                }
                _ => {}
            }
        }
    });

    println!("🧑 {task}\n");
    audit.record("agent", "task_start", task);
    let agent = ReActAgent::new(&engine, &tools, bus.clone());
    let run = agent.run(task).await?;
    // da tiempo a vaciar los eventos pendientes
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    printer.abort();
    audit.record(
        "agent",
        "task_done",
        format!("{} pasos · {}", run.steps, run.answer),
    );
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

/// Auto-evolución F5 (lazo cerrado): el LLM ESCRIBE una skill candidata y esta
/// pasa por las puertas de seguridad gated. Si el código es inválido o falla los
/// tests, se rechaza sin dañar el sistema — la autonomía es segura por diseño.
async fn run_self_evolve() -> Result<(), Box<dyn std::error::Error>> {
    use aion_evolution::{Candidate, EvolutionEngine};
    use aion_kernel::traits::SkillHost;
    use aion_skills::{SkillManifest, WasmSkillHost};
    use std::sync::Arc;

    let engine = OllamaEngine::default_local();
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;

    // Tarea a auto-implementar + su oráculo (tests).
    let task = "eleva el número al cuadrado (devuelve n*n)";
    let tests = vec![(2_i64, 4_i64), (3, 9), (5, 25), (0, 0)];

    let live = Arc::new(WasmSkillHost::new()?);
    let mut evo = EvolutionEngine::new(live.clone());
    let audit = aion_telemetry::AuditLog::default_local();

    println!("🎯 Tarea para auto-implementar: {task}");
    println!("🧪 Oráculo (tests): {tests:?}\n");

    // Hasta 3 intentos: el LLM regenera si la candidata es rechazada.
    for attempt in 1..=3 {
        println!("── Intento {attempt}/3 ─────────────────────────────");
        let prompt = format!(
            "Escribe un módulo WebAssembly en formato WAT que exporte una función `run` \
             que reciba un i64 y devuelva un i64, implementando: {task}.\n\n\
             Ejemplo de formato VÁLIDO (esto duplica n):\n\
             (module (func (export \"run\") (param $n i64) (result i64) \
             (i64.mul (local.get $n) (i64.const 2))))\n\n\
             Responde SOLO con el módulo WAT en una línea, sin explicación ni markdown."
        );
        let msg = engine
            .generate(aion_kernel::traits::GenerateRequest {
                messages: vec![Message::user(prompt)],
                think: false,
                temperature: Some(0.3),
                max_tokens: Some(256),
            })
            .await?;

        let Some(code) = extract_wat(&msg.content) else {
            println!("🤖 el LLM no produjo un módulo WAT válido; reintentando…\n");
            continue;
        };
        println!(
            "🤖 candidata generada por el LLM:\n   {}\n",
            code.replace('\n', " ")
        );

        let report = evo
            .propose(Candidate {
                manifest: SkillManifest {
                    name: "square".into(),
                    description: task.into(),
                },
                code,
                tests: tests.clone(),
            })
            .await?;
        println!("   {} — {}", verdict(report.accepted), report.reason);
        audit.record(
            "evolution",
            if report.accepted {
                "candidate_accepted"
            } else {
                "candidate_rejected"
            },
            format!("square (intento {attempt}): {}", report.reason),
        );

        if report.accepted {
            let out = live.invoke("square", serde_json::json!(9)).await?;
            println!(
                "\n✅ El agente escribió y AION integró una skill nueva: square(9) = {}",
                out.output["result"]
            );
            println!("\x1b[2m(código generado por el LLM, validado por sandbox+tests antes de integrarse)\x1b[0m");
            return Ok(());
        }
        println!();
    }
    println!("⛔ Ninguna candidata superó las puertas. El sistema queda intacto (rollback). ");
    println!("\x1b[2m(esto es el comportamiento seguro: nada inválido se integra)\x1b[0m");
    Ok(())
}

/// Extrae un módulo WAT `(module ...)` balanceando paréntesis.
fn extract_wat(text: &str) -> Option<String> {
    let start = text.find("(module")?;
    let mut depth = 0i32;
    let mut end = None;
    for (i, c) in text[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    end.map(|e| text[start..start + e].to_string())
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

/// Bucle de VIDA autónomo completo: AION actúa sin que se lo pidan.
/// En cada ciclo la **curiosidad elige** una actividad (razonar/estudiar/evolucionar),
/// el **agente la ejecuta** y el resultado **realimenta la curiosidad**; además
/// 🌙 sueña (consolida) y 🪞 reflexiona. Acotado, con circuit breaker, todo auditado.
async fn run_live(cycles: u32) -> Result<(), Box<dyn std::error::Error>> {
    use aion_cognition::{CuriosityEngine, SelfModel};
    use aion_memory::ConsolidationConfig;

    let engine = OllamaEngine::default_local();
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;

    let audit = aion_telemetry::AuditLog::default_local();
    let mut self_model = SelfModel::default();
    let mut curiosity = CuriosityEngine::new(8);
    let activities = ["razonar", "estudiar", "evolucionar"];
    let mut consecutive_errors = 0u32;
    const BREAKER: u32 = 3;

    audit.record("daemon", "live_start", format!("{cycles} ciclos"));
    println!(
        "🌱 AION despierta — bucle de vida autónomo ({cycles} ciclos). Ctrl-C para detener.\n"
    );

    for cycle in 1..=cycles {
        println!("── ciclo {cycle}/{cycles} ────────────────────────");

        // 🎯 CURIOSIDAD elige la actividad (mayor learning progress / no explorada).
        let goal = curiosity.next_goal(&activities).unwrap_or("estudiar");
        println!(
            "🎯 curiosidad elige: {goal}  (LP={:+.2})",
            curiosity.learning_progress(goal)
        );

        // 🤖 EJECUTAR la actividad elegida.
        let (success, detail) = match goal {
            "razonar" => agent_once(&engine, "¿Cuánto es 37*21+8? Usa la calculadora.").await,
            "evolucionar" => self_evolve_once(&engine).await,
            _ => study_once(&engine).await,
        };
        println!("   {} {goal}: {detail}", if success { "✅" } else { "❌" });

        // 🔁 REALIMENTAR curiosidad + auto-modelo.
        curiosity.record(goal, success);
        self_model.observe(success);
        audit.record(
            "daemon",
            goal,
            format!("{}: {detail}", if success { "ok" } else { "fail" }),
        );
        if success {
            consecutive_errors = 0;
        } else {
            consecutive_errors += 1;
            if consecutive_errors >= BREAKER {
                println!("🛑 circuit breaker: demasiados fallos, deteniendo el bucle.");
                audit.record("daemon", "breaker_tripped", "demasiados fallos");
                break;
            }
        }

        // 🌙 SOÑAR (consolidar) y 🪞 REFLEXIONAR.
        if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
            if let Ok(r) = mem.consolidate(&ConsolidationConfig::default()) {
                println!("🌙 sueño: {} → {} recuerdos", r.before, r.after);
            }
        }
        println!("🪞 {}\n", self_model.introspect());

        if cycle < cycles {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    audit.record("daemon", "live_stop", "fin del bucle");
    println!("💤 AION vuelve al reposo. (lo aprendido quedó en su memoria y en el audit log)");
    Ok(())
}

/// Ejecuta el agente ReAct (silencioso) sobre una tarea; devuelve (éxito, respuesta).
async fn agent_once(engine: &OllamaEngine, task: &str) -> (bool, String) {
    use aion_orchestrator::{CalculatorTool, ReActAgent, ToolRegistry};
    use aion_skills::{SkillManifest, WasmSkillHost, SUM_TO_WAT};
    use std::sync::Arc;

    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(CalculatorTool));
    if let Ok(h) = WasmSkillHost::new() {
        if h.register(
            SkillManifest {
                name: "sum_to".into(),
                description: "suma 1..=n".into(),
            },
            SUM_TO_WAT,
        )
        .is_ok()
        {
            tools.register(Arc::new(skill_tool::SkillTool::new(
                Arc::new(h),
                "sum_to",
                "Suma 1..=n (skill WASM). Entrada: n.",
            )));
        }
    }
    let agent = ReActAgent::new(engine, &tools, EventBus::default());
    match agent.run(task).await {
        Ok(run) if !run.answer.starts_with("No pude") => (true, run.answer),
        Ok(run) => (false, run.answer),
        Err(e) => (false, e.to_string()),
    }
}

/// Un intento de auto-evolución (el LLM escribe la skill 'square', gated).
async fn self_evolve_once(engine: &OllamaEngine) -> (bool, String) {
    use aion_evolution::{Candidate, EvolutionEngine};
    use aion_skills::{SkillManifest, WasmSkillHost};
    use std::sync::Arc;

    let prompt = "Escribe un módulo WebAssembly WAT que exporte `run` (param i64, result i64) \
        que devuelva n*n. Ejemplo válido: (module (func (export \"run\") (param $n i64) (result i64) \
        (i64.mul (local.get $n) (i64.const 2)))). Responde SOLO el módulo WAT.";
    let msg = match engine
        .generate(GenerateRequest {
            messages: vec![Message::user(prompt)],
            think: false,
            temperature: Some(0.3),
            max_tokens: Some(256),
        })
        .await
    {
        Ok(m) => m,
        Err(e) => return (false, e.to_string()),
    };
    let Some(code) = extract_wat(&msg.content) else {
        return (false, "el LLM no produjo WAT válido".into());
    };
    let Ok(live) = WasmSkillHost::new() else {
        return (false, "no se pudo crear el host".into());
    };
    let mut evo = EvolutionEngine::new(Arc::new(live));
    match evo
        .propose(Candidate {
            manifest: SkillManifest {
                name: "square".into(),
                description: "n*n".into(),
            },
            code,
            tests: vec![(2, 4), (3, 9), (5, 25)],
        })
        .await
    {
        Ok(r) => (r.accepted, r.reason),
        Err(e) => (false, e.to_string()),
    }
}

/// Genera un insight de auto-mejora y lo guarda en memoria; devuelve (éxito, insight).
async fn study_once(engine: &OllamaEngine) -> (bool, String) {
    let req = GenerateRequest {
        messages: vec![Message::user(
            "Genera UNA idea breve y concreta para mejorarte como agente de IA local. Una sola frase.",
        )],
        think: false,
        temperature: Some(0.9),
        max_tokens: Some(80),
    };
    match engine.generate(req).await {
        Ok(msg) => {
            let insight = msg.content.trim().to_string();
            if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
                let _ = mem.store(&format!("[insight] {insight}")).await;
            }
            (!insight.is_empty(), insight)
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Demo de cognición: curiosidad (learning progress), auto-modelo y calibración.
fn run_cognition() {
    use aion_cognition::{Calibration, CuriosityEngine, SelfModel};

    println!("🧠 Subsistemas cognitivos de AION\n");

    // 1) Curiosidad por learning progress (3 objetivos distintos).
    let mut cur = CuriosityEngine::new(6);
    for s in [true, true, true, true, true, true] {
        cur.record("ya_dominado", s); // mastered
    }
    for s in [false, false, false, false, false, false] {
        cur.record("imposible", s); // sin progreso
    }
    for s in [false, false, false, true, true, true] {
        cur.record("en_aprendizaje", s); // progresando
    }
    println!("🎯 Curiosidad (learning progress):");
    for g in ["ya_dominado", "imposible", "en_aprendizaje"] {
        println!(
            "   {g:14} competencia={:.0}%  LP={:+.2}",
            cur.competence(g) * 100.0,
            cur.learning_progress(g)
        );
    }
    let next = cur.next_goal(&["ya_dominado", "imposible", "en_aprendizaje", "nunca_visto"]);
    println!("   → siguiente objetivo elegido: {next:?}  (curiosidad)\n");

    // 2) Auto-modelo.
    let mut sm = SelfModel::default();
    for s in [true, true, false, true, true, true, true, false, true, true] {
        sm.observe(s);
    }
    println!("🪞 {}\n", sm.introspect());

    // 3) Metacognición (calibración).
    let mut cal = Calibration::new();
    cal.record(0.9, true);
    cal.record(0.8, true);
    cal.record(0.6, false);
    cal.record(0.7, true);
    println!(
        "🤔 Metacognición: {} ({} muestras)",
        cal.verdict(),
        cal.samples()
    );
}

/// Muestra el audit log (acciones del agente y de la auto-evolución).
fn run_audit() {
    let log = aion_telemetry::AuditLog::default_local();
    let entries = log.read_all();
    println!("🔎 Audit log ({} entradas)\n", entries.len());
    for e in &entries {
        println!(
            "  {} · [{}] {} — {}",
            &e.ts[..19.min(e.ts.len())],
            e.actor,
            e.action,
            e.detail
        );
    }
    if entries.is_empty() {
        println!("(vacío — usa el agente o `evolve`/`self-evolve` y se irá registrando)");
    }
}

/// Historial de conversaciones guardadas en la memoria de largo plazo.
fn run_history() -> Result<(), Box<dyn std::error::Error>> {
    let path = memory_path();
    let mem = VectorMemory::persistent_local(&path)?;
    let convos: Vec<String> = mem
        .contents()
        .into_iter()
        .filter(|c| c.starts_with("[conversación]"))
        .collect();
    println!(
        "🗂  Historial de conversaciones ({}) · {path}\n",
        convos.len()
    );
    for (i, c) in convos.iter().enumerate() {
        let line = c.trim_start_matches("[conversación]").trim();
        println!("{:>3}. {}", i + 1, line);
    }
    if convos.is_empty() {
        println!("(aún no hay conversaciones guardadas — chatea y se guardarán solas)");
    }
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
