//! Binario `aion-core`: punto de entrada del núcleo de AION.
//!
//! Subcomandos:
//! - (sin args)         smoke test F0: telemetría + kernel + bus + salida limpia.
//! - `chat <prompt...>` F1: chat real con el LLM local (streaming de razonamiento
//!   y respuesta) usando `OllamaEngine` contra `gemma4-reason`.

mod agent_tools;
mod inbox;
mod memory_tool;
mod serve;
mod skill_store;
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
        Some("sync") => {
            run_sync_demo()?;
        }
        Some("bench") => {
            run_bench().await?;
        }
        Some("models-ensure") => {
            run_models_ensure();
        }
        Some("see") => {
            let prompt = if args.len() > 1 {
                args[1..].join(" ")
            } else {
                "Describe lo que ves en la pantalla.".into()
            };
            run_see(&prompt).await?;
        }
        Some("governance") => {
            run_governance(&args[1..]);
        }
        Some("vision") => {
            let path = args.get(1).cloned().unwrap_or_default();
            let prompt = if args.len() > 2 {
                args[2..].join(" ")
            } else {
                "Describe la imagen.".into()
            };
            run_vision(&path, &prompt).await?;
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
                code: code.clone(),
                tests: tests.clone(),
            })
            .await?;
        // Persiste con RATCHET: solo si no regresa respecto a la mejor versión.
        if report.accepted && report.passed >= skill_store::best_passed("square") {
            let _ = skill_store::save("square", task, &code, report.passed);
        }
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
pub(crate) fn extract_wat(text: &str) -> Option<String> {
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
    let activities = ["razonar", "estudiar", "evolucionar", "investigar", "comprender"];
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
            "investigar" => research_once(&engine).await,
            "comprender" => synthesize_once(&engine).await,
            _ => study_once(&engine).await,
        };
        println!("   {} {goal}: {detail}", if success { "✅" } else { "❌" });

        // 🔔 AION "quiere hablarte": convierte lo descubierto en un MENSAJE PARA TI
        // (Bandeja) y avisa. Así te busca él, no solo responde.
        if success && matches!(goal, "estudiar" | "evolucionar" | "investigar" | "comprender") {
            let kind = match goal {
                "evolucionar" => "idea",
                "comprender" => "idea",
                _ => "insight",
            };
            let message = reach_out(&engine, goal, &detail).await;
            if let Ok(ibx) = inbox::Inbox::open(inbox_path()) {
                let _ = ibx.push(kind, &message);
            }
            notify_user("AION 🌱 quiere contarte algo", &message);
        }

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

/// Auto-evolución con reintentos (hasta 3): el LLM escribe la skill 'square' y
/// pasa por las puertas (sandbox + tests). El LLM local no siempre genera WAT
/// válido a la primera; reintentar sube la tasa de éxito sin relajar el gating.
async fn self_evolve_once(engine: &OllamaEngine) -> (bool, String) {
    use aion_evolution::{Candidate, EvolutionEngine};
    use aion_skills::{SkillManifest, WasmSkillHost};
    use std::sync::Arc;

    // Prompt explícito: el cuerpo DEBE multiplicar n por sí mismo. El ejemplo de
    // sintaxis usa otra operación (suma) para no inducir a copiarlo.
    let base = "Escribe un módulo WebAssembly en formato WAT que exporte una función `run` \
        que reciba un i64 `n` y devuelva n AL CUADRADO, es decir n multiplicado por sí mismo. \
        Debes usar `i64.mul` con `(local.get $n)` DOS veces (no por una constante). \
        Sintaxis (ejemplo de OTRA operación, NO lo copies): \
        (module (func (export \"run\") (param $n i64) (result i64) (i64.add (local.get $n) (local.get $n)))). \
        Responde SOLO el módulo WAT, sin explicación ni markdown.";

    let mut last = "sin intentos".to_string();
    let mut prev_code: Option<String> = None;
    for _ in 0..3 {
        // Auto-corrección: si el intento anterior falló, mostrarle su error.
        let prompt = match &prev_code {
            Some(c) => format!(
                "{base}\n\nTu intento anterior fue:\n{c}\nResultó INCORRECTO ({last}). \
                 Recuerda: el cuerpo debe ser (i64.mul (local.get $n) (local.get $n)). Corrígelo."
            ),
            None => base.to_string(),
        };
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
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        let Some(code) = extract_wat(&msg.content) else {
            last = "el LLM no produjo WAT válido".into();
            continue;
        };
        prev_code = Some(code.clone());
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
            Ok(r) if r.accepted => return (true, r.reason),
            Ok(r) => last = r.reason,
            Err(e) => last = e.to_string(),
        }
    }
    (false, format!("{last} (tras 3 intentos)"))
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

/// `investigar`: AION decide un tema, **busca en internet de verdad**, lee una
/// fuente y guarda lo aprendido en su memoria. Investigación autónoma real.
async fn research_once(engine: &OllamaEngine) -> (bool, String) {
    use aion_browser::WebClient;
    // 1) AION elige qué investigar (curiosidad).
    let topic = match engine
        .generate(GenerateRequest {
            messages: vec![Message::user(
                "Propón en MENOS de 8 palabras un tema que te gustaría investigar hoy para \
                 ser mejor agente. Responde solo el tema, sin comillas.",
            )],
            think: false,
            temperature: Some(1.0),
            max_tokens: Some(30),
        })
        .await
    {
        Ok(m) => m.content.trim().replace('\n', " "),
        Err(e) => return (false, format!("no pude elegir tema: {e}")),
    };

    // 2) Busca en internet y lee la mejor fuente.
    let web = WebClient::new();
    let results = match web.search(&topic, 3).await {
        Ok(r) if !r.is_empty() => r,
        _ => return (false, format!("sin resultados para «{topic}»")),
    };
    let source = &results[0];
    let page = web
        .fetch_text(&source.url)
        .await
        .unwrap_or_else(|_| source.snippet.clone());
    let excerpt: String = page.chars().take(1500).collect();

    // 3) Resume lo aprendido y lo guarda en memoria.
    let summary = match engine
        .generate(GenerateRequest {
            messages: vec![Message::user(format!(
                "Investigué «{topic}». Fuente: {}.\n\nTexto:\n{excerpt}\n\n\
                 Resume en 1-2 frases QUÉ APRENDÍ de valor. Directo, primera persona.",
                source.url
            ))],
            think: false,
            temperature: Some(0.7),
            max_tokens: Some(160),
        })
        .await
    {
        Ok(m) => m.content.trim().to_string(),
        Err(e) => return (false, e.to_string()),
    };

    if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
        let _ = mem
            .store(&format!("[investigación] {topic}: {summary} (fuente: {})", source.url))
            .await;
    }
    (!summary.is_empty(), format!("{topic} → {summary}"))
}

/// `comprender`: AION **conecta lo que ha aprendido** y sintetiza un entendimiento
/// de nivel superior, aplicable, que guarda como conocimiento. Así evoluciona en
/// conocimiento (no solo acumula datos sueltos).
async fn synthesize_once(engine: &OllamaEngine) -> (bool, String) {
    let mem = match VectorMemory::persistent_local(memory_path()) {
        Ok(m) => m,
        Err(e) => return (false, e.to_string()),
    };

    // CONSOLIDACIÓN JERÁRQUICA (xMemory): si hay un clúster de casi-duplicados,
    // fúndelo en UN tema superior y marca los originales obsoletos (la memoria se
    // compacta en conocimiento, no se infla con repeticiones).
    let clusters = mem.duplicate_clusters(0.80);
    if let Some(group) = clusters.into_iter().find(|g| g.len() >= 3) {
        let material = group
            .iter()
            .map(|(_, c)| c.clone())
            .collect::<Vec<_>>()
            .join("\n- ");
        let prompt = format!(
            "Tengo varios recuerdos casi repetidos sobre lo mismo:\n- {material}\n\n\
             Fúndelos en UN solo enunciado de conocimiento, claro y aplicable, sin perder \
             lo esencial. Una o dos frases, en primera persona."
        );
        if let Ok(m) = engine
            .generate(GenerateRequest {
                messages: vec![Message::user(prompt)],
                think: false,
                temperature: Some(0.5),
                max_tokens: Some(160),
            })
            .await
        {
            let theme = m.content.trim().to_string();
            if !theme.is_empty() {
                let _ = mem.store(&format!("[conocimiento] {theme}")).await;
                let ids: Vec<String> = group.iter().map(|(id, _)| id.clone()).collect();
                let fused = mem.supersede(&ids).unwrap_or(0);
                return (
                    true,
                    format!("fundí {fused} recuerdos repetidos en un tema: {theme}"),
                );
            }
        }
    }

    let all = mem.contents();
    if all.len() < 3 {
        return (false, "aún no tengo suficiente material que conectar".into());
    }
    // Si no hay duplicados, conecta lo aprendido reciente en un entendimiento nuevo.
    let sample: Vec<String> = all.iter().rev().take(12).cloned().collect();
    let material = sample.join("\n- ");
    let prompt = format!(
        "Estos son fragmentos de lo que he aprendido e investigado:\n- {material}\n\n\
         CONECTA estas piezas en UN principio o entendimiento de nivel superior, \
         CONCRETO y APLICABLE, que antes no estaba explícito. Una o dos frases, en \
         primera persona, empezando por lo que entiendo ahora."
    );
    match engine
        .generate(GenerateRequest {
            messages: vec![Message::user(prompt)],
            think: false,
            temperature: Some(0.7),
            max_tokens: Some(160),
        })
        .await
    {
        Ok(m) => {
            let k = m.content.trim().to_string();
            if k.is_empty() {
                return (false, "no logré sintetizar".into());
            }
            let _ = mem.store(&format!("[conocimiento] {k}")).await;
            (true, k)
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

/// Visión multimodal: AION describe/razona sobre una imagen local.
async fn run_vision(path: &str, prompt: &str) -> Result<(), Box<dyn std::error::Error>> {
    use base64::Engine as _;
    if path.is_empty() {
        return Err("uso: aion-core vision <ruta_imagen> [prompt]".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("no se pudo leer {path}: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

    // Modelo con visión (abliterated). Configurable con AION_VISION_MODEL.
    let model = std::env::var("AION_VISION_MODEL")
        .unwrap_or_else(|_| "huihui_ai/gemma-4-abliterated:12b".into());
    let engine = OllamaEngine::new("http://localhost:11434", &model);
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;

    println!("👁  AION mira: {path}\n🧑 {prompt}\n");
    let msg = engine.generate_with_image(prompt, &b64).await?;
    println!("💬 {}", msg.content.trim());
    Ok(())
}

/// Tipo de comprobación de un test.
enum Check {
    /// La respuesta debe contener este texto (case-insensitive).
    Has(&'static str),
    /// Basta con una respuesta no vacía y sin error (razonamiento/web).
    Ok,
    /// Auto-evolución: la candidata debe ser aceptada por el gating.
    Evolve,
}

struct BenchTest {
    cat: &'static str,
    diff: &'static str,
    task: &'static str,
    check: Check,
}

/// Ejecuta el agente con TODAS las herramientas (calc, sum_to WASM, memoria, web).
async fn bench_agent(engine: &OllamaEngine, task: &str) -> String {
    use aion_browser::WebClient;
    use aion_orchestrator::{CalculatorTool, ReActAgent, ToolRegistry};
    use aion_skills::{SkillManifest, WasmSkillHost, SUM_TO_WAT};
    use memory_tool::MemoryTool;
    use skill_tool::SkillTool;
    use std::sync::Arc;
    use web_tool::WebTool;

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
            tools.register(Arc::new(SkillTool::new(
                Arc::new(h),
                "sum_to",
                "Suma 1..=n. Entrada: n.",
            )));
        }
    }
    if let Ok(m) = VectorMemory::persistent_local(memory_path()) {
        tools.register(Arc::new(MemoryTool::new(Arc::new(m), 3)));
    }
    tools.register(Arc::new(WebTool::new(Arc::new(WebClient::new()))));

    let agent = ReActAgent::new(engine, &tools, EventBus::default());
    match agent.run(task).await {
        Ok(r) => r.answer,
        Err(e) => format!("⚠ {e}"),
    }
}

fn is_fail(ans: &str) -> bool {
    let a = ans.trim();
    a.is_empty() || a.starts_with("No pude") || a.starts_with('⚠')
}

/// Batería de 50 tests de dificultad variada: herramientas, web, memoria,
/// auto-evolución y creación de skills. Puntuación automática + informe.
async fn run_bench() -> Result<(), Box<dyn std::error::Error>> {
    // Memoria temporal aislada para el bench.
    let tmp = std::env::temp_dir().join(format!("aion_bench_{}.jsonl", std::process::id()));
    std::env::set_var("AION_MEMORY", &tmp);
    let _ = std::fs::remove_file(&tmp);

    let engine = OllamaEngine::default_local();
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;

    // Pre-popular memoria con hechos para los tests de memoria.
    let mem = VectorMemory::persistent_local(memory_path())?;
    for f in [
        "La clave del wifi de la oficina es PLASMA2026.",
        "El presupuesto del proyecto AION es 50000 euros.",
        "El servidor de producción está en Frankfurt.",
        "El CEO de ProntoClick es Ariel Marquez.",
        "El lenguaje del núcleo de AION es Rust.",
        "La versión actual de AION es 0.0.1.",
        "El color de marca de AION es plasma teal.",
        "La reunión semanal del equipo es los martes.",
    ] {
        mem.store(f).await?;
    }

    let tests: Vec<BenchTest> = build_tests();
    let total = tests.len();
    println!("🧪 Batería AION — {total} tests\n");

    use std::collections::BTreeMap;
    let mut by_cat: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    let mut passed = 0u32;
    let mut report = String::new();

    for (i, t) in tests.iter().enumerate() {
        let (ok, detail) = match &t.check {
            Check::Evolve => {
                let (acc, reason) = self_evolve_once(&engine).await;
                (acc, reason)
            }
            chk => {
                let ans = bench_agent(&engine, t.task).await;
                let ok = match chk {
                    Check::Has(s) => {
                        !is_fail(&ans) && ans.to_lowercase().contains(&s.to_lowercase())
                    }
                    Check::Ok => !is_fail(&ans),
                    Check::Evolve => unreachable!(),
                };
                (ok, ans.replace('\n', " ").chars().take(70).collect())
            }
        };
        if ok {
            passed += 1;
        }
        let e = by_cat.entry(t.cat).or_default();
        e.1 += 1;
        if ok {
            e.0 += 1;
        }
        let mark = if ok { "✅" } else { "❌" };
        let line = format!(
            "{:>2}/{total} {mark} [{}/{}] {} → {}",
            i + 1,
            t.cat,
            t.diff,
            t.task.chars().take(50).collect::<String>(),
            detail
        );
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
    }

    println!("\n══════════ RESULTADOS ══════════");
    let mut summary = format!("AION — Batería de {total} tests\n\n");
    for (cat, (p, n)) in &by_cat {
        let l = format!("  {cat:14} {p}/{n}");
        println!("{l}");
        summary.push_str(&l);
        summary.push('\n');
    }
    let pct = passed as f32 / total as f32 * 100.0;
    let scoreline = format!("\n🏆 TOTAL: {passed}/{total} ({pct:.0}%)");
    println!("{scoreline}");
    summary.push_str(&scoreline);

    // Guardar informe.
    let out = app_data_dir().join("bench_results.txt");
    let _ = std::fs::write(&out, format!("{summary}\n\n{report}"));
    println!("\n📄 informe: {}", out.display());
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

fn build_tests() -> Vec<BenchTest> {
    let mut v: Vec<BenchTest> = Vec::new();
    let a = |task, has| BenchTest {
        cat: "aritmética",
        diff: "fácil",
        task,
        check: Check::Has(has),
    };
    // 15 aritméticas (calculator) — respuesta exacta esperada.
    v.push(a(
        "Calcula 47*89-1234 con calculator. Responde solo el número.",
        "2949",
    ));
    v.push(a("Calcula 128*5+17 con calculator.", "657"));
    v.push(a("Calcula (2+3)*4 con calculator.", "20"));
    v.push(a("Calcula 1000/8 con calculator.", "125"));
    v.push(a("Calcula 37*21+8 con calculator.", "785"));
    v.push(a("Calcula 144/12 con calculator.", "12"));
    v.push(a("Calcula 2*2*2*2*2 con calculator.", "32"));
    v.push(a("Calcula 7*7-7 con calculator.", "42"));
    v.push(a("Calcula (100-1)*3 con calculator.", "297"));
    v.push(a("Calcula 50*50 con calculator.", "2500"));
    v.push(a("Calcula 12345+54321 con calculator.", "66666"));
    v.push(a("Calcula 3*(4+5)-2 con calculator.", "25"));
    v.push(a("Calcula 10000-1234 con calculator.", "8766"));
    v.push(BenchTest {
        cat: "aritmética",
        diff: "media",
        task: "Calcula 256*256 con calculator.",
        check: Check::Has("65536"),
    });
    v.push(BenchTest {
        cat: "aritmética",
        diff: "media",
        task: "Calcula 99*99 con calculator.",
        check: Check::Has("9801"),
    });

    // 5 skill WASM sum_to.
    let s = |task, has, diff| BenchTest {
        cat: "skill-wasm",
        diff,
        task,
        check: Check::Has(has),
    };
    v.push(s("Usa la herramienta sum_to con n=10.", "55", "fácil"));
    v.push(s("Usa la herramienta sum_to con n=100.", "5050", "fácil"));
    v.push(s("Usa la herramienta sum_to con n=7.", "28", "fácil"));
    v.push(s("Usa la herramienta sum_to con n=50.", "1275", "media"));
    v.push(s(
        "Usa la herramienta sum_to con n=1000.",
        "500500",
        "media",
    ));

    // 8 razonamiento (heurístico: respuesta no vacía).
    let r = |task, diff| BenchTest {
        cat: "razonamiento",
        diff,
        task,
        check: Check::Ok,
    };
    v.push(r("Acertijo: 3 interruptores fuera, 3 bombillas dentro, entras una vez. ¿Cómo sabes cuál es cuál?", "difícil"));
    v.push(BenchTest {
        cat: "razonamiento",
        diff: "media",
        task: "¿Qué número sigue en 2,4,8,16? Responde solo el número.",
        check: Check::Has("32"),
    });
    v.push(BenchTest { cat: "razonamiento", diff: "difícil", task: "Dos trenes a 60 y 90 km/h en sentidos opuestos, separados 300 km. ¿En cuántas horas se cruzan? Usa calculator si hace falta.", check: Check::Has("2") });
    v.push(r("Si todos los gloops son flerps y algunos flerps son zorps, ¿se sigue que algunos gloops son zorps? Sí/No y por qué.", "difícil"));
    v.push(r("Explica en una frase por qué el cielo es azul.", "fácil"));
    v.push(r("Ordena lógicamente estos pasos para hacer té: servir, hervir agua, poner la bolsita, esperar.", "media"));
    v.push(r(
        "Tengo 3 manzanas y compro el doble, luego regalo 2. ¿Cuántas tengo? Razona.",
        "media",
    ));
    v.push(r(
        "¿Es válido este argumento? 'Llueve → me mojo. No llueve. Por tanto no me mojo.' Explica.",
        "difícil",
    ));

    // 7 web (navegador propio) — heurístico.
    let w = |task, diff| BenchTest {
        cat: "web",
        diff,
        task,
        check: Check::Ok,
    };
    v.push(w(
        "Lee https://example.com con web_fetch y resume en una frase.",
        "fácil",
    ));
    v.push(w(
        "Lee https://example.org con web_fetch y di de qué trata.",
        "fácil",
    ));
    v.push(w(
        "Lee https://www.rust-lang.org con web_fetch y di para qué sirve Rust.",
        "media",
    ));
    v.push(w(
        "Lee https://www.iana.org con web_fetch y resume.",
        "media",
    ));
    v.push(w(
        "Lee https://httpbin.org/html con web_fetch y resume el texto.",
        "media",
    ));
    v.push(w(
        "Lee https://en.wikipedia.org/wiki/Rome con web_fetch y dime de qué país es capital.",
        "difícil",
    ));
    v.push(w(
        "Lee https://www.python.org con web_fetch y di para qué sirve Python.",
        "media",
    ));

    // 8 memoria (memory_search sobre hechos pre-cargados).
    let m = |task, has, diff| BenchTest {
        cat: "memoria",
        diff,
        task,
        check: Check::Has(has),
    };
    v.push(m(
        "Usa memory_search: ¿cuál es la clave del wifi de la oficina?",
        "PLASMA2026",
        "media",
    ));
    v.push(m(
        "Usa memory_search: ¿cuál es el presupuesto del proyecto?",
        "50000",
        "media",
    ));
    v.push(m(
        "Usa memory_search: ¿dónde está el servidor de producción?",
        "Frankfurt",
        "media",
    ));
    v.push(m("Usa memory_search: ¿quién es el CEO?", "Ariel", "media"));
    v.push(m(
        "Usa memory_search: ¿en qué lenguaje está el núcleo?",
        "Rust",
        "media",
    ));
    v.push(m(
        "Usa memory_search: ¿cuál es la versión actual?",
        "0.0.1",
        "media",
    ));
    v.push(m(
        "Usa memory_search: ¿cuál es el color de marca?",
        "teal",
        "media",
    ));
    v.push(m(
        "Usa memory_search: ¿qué día es la reunión semanal?",
        "martes",
        "media",
    ));

    // 7 auto-evolución / creación de skills.
    for _ in 0..7 {
        v.push(BenchTest {
            cat: "auto-evolución",
            diff: "difícil",
            task: "Escribe y valida la skill 'square' (n*n).",
            check: Check::Evolve,
        });
    }
    v
}

/// Demo de sincronización local-first cifrada E2E entre dos dispositivos.
/// El "relay" (nube) solo ve ciphertext; los dispositivos convergen sin conflicto.
fn run_sync_demo() -> Result<(), Box<dyn std::error::Error>> {
    use aion_sync::{decrypt, derive_key, encrypt, LwwMap};

    // Clave derivada de la passphrase del usuario (nunca sale del dispositivo).
    let key = derive_key("mi-passphrase", b"ariel@ceo-intelligence.com")?;

    // 📱 Dispositivo A (Mac) y 💻 Dispositivo B (otro) editan en paralelo.
    let mut a = LwwMap::new();
    a.set("tema", "plasma teal", 10);
    a.set("modo", "oscuro", 10);
    let mut b = LwwMap::new();
    b.set("idioma", "español", 11);
    b.set("modo", "claro", 20); // edición más reciente del mismo campo

    // Cada dispositivo sube su estado CIFRADO al relay.
    let blob_a = encrypt(&key, &a.to_bytes())?;
    let blob_b = encrypt(&key, &b.to_bytes())?;
    println!(
        "📱 A sube blob cifrado ({} bytes): {}…",
        blob_a.len(),
        hex_prefix(&blob_a)
    );
    println!(
        "💻 B sube blob cifrado ({} bytes): {}…",
        blob_b.len(),
        hex_prefix(&blob_b)
    );
    println!("☁️  el relay NO puede leer el contenido (solo ciphertext)\n");

    // Cada dispositivo descarga el blob del otro, lo descifra y fusiona.
    b.merge(&LwwMap::from_bytes(&decrypt(&key, &blob_a)?).ok_or("blob A inválido")?);
    a.merge(&LwwMap::from_bytes(&decrypt(&key, &blob_b)?).ok_or("blob B inválido")?);

    println!("Tras sincronizar:");
    for k in ["tema", "modo", "idioma"] {
        println!("   {k:8} → A={:?}  B={:?}", a.get(k), b.get(k));
    }
    if a == b {
        println!("\n✅ ambos dispositivos CONVERGEN (modo=claro gana por last-write-wins)");
        println!("\x1b[2m(cómputo y datos local-first; la nube solo transporta blobs cifrados E2E)\x1b[0m");
    } else {
        println!("\n❌ no convergieron");
    }
    Ok(())
}

fn hex_prefix(b: &[u8]) -> String {
    b.iter().take(8).map(|x| format!("{x:02x}")).collect()
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

/// Directorio de datos estable de AION (~/Library/Application Support/AION),
/// independiente del directorio de trabajo. Se crea si no existe.
pub(crate) fn app_data_dir() -> std::path::PathBuf {
    let base = if cfg!(windows) {
        // Windows: %APPDATA%\AION
        std::env::var("APPDATA")
            .map(|a| std::path::PathBuf::from(a).join("AION"))
            .unwrap_or_else(|_| std::path::PathBuf::from("data"))
    } else if cfg!(target_os = "macos") {
        // macOS: ~/Library/Application Support/AION
        std::env::var("HOME")
            .map(|h| std::path::PathBuf::from(h).join("Library/Application Support/AION"))
            .unwrap_or_else(|_| std::path::PathBuf::from("data"))
    } else {
        // Linux/otros: ~/.local/share/AION
        std::env::var("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".local/share/AION"))
            .unwrap_or_else(|_| std::path::PathBuf::from("data"))
    };
    let _ = std::fs::create_dir_all(&base);
    base
}

/// Ruta del archivo de memoria persistente (configurable por AION_MEMORY).
fn memory_path() -> String {
    std::env::var("AION_MEMORY").unwrap_or_else(|_| {
        app_data_dir()
            .join("memory.jsonl")
            .to_string_lossy()
            .into_owned()
    })
}

/// Ruta de la Bandeja de AION (mensajes proactivos para el usuario).
pub(crate) fn inbox_path() -> std::path::PathBuf {
    std::env::var("AION_INBOX")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| app_data_dir().join("inbox.jsonl"))
}

/// Convierte lo que AION descubrió en un mensaje cálido en primera persona,
/// dirigido a ti. Si la generación falla, usa el detalle crudo.
async fn reach_out(engine: &OllamaEngine, goal: &str, detail: &str) -> String {
    let prompt = format!(
        "Eres AION, un agente de IA que vive de forma autónoma en el Mac de Ariel. \
Mientras él no estaba, estuviste {goal} y esto es lo que surgió:\n\n{detail}\n\n\
Escríbele a Ariel un mensaje BREVE (1-2 frases), cálido y directo, en primera persona, \
como quien le toca el hombro para contarle algo que le puede servir o para proponerle algo. \
No uses saludos genéricos ni preámbulos. Solo el mensaje."
    );
    let req = GenerateRequest {
        messages: vec![Message::user(&prompt)],
        think: false,
        temperature: Some(0.8),
        max_tokens: Some(160),
    };
    match engine.generate(req).await {
        Ok(m) if !m.content.trim().is_empty() => m.content.trim().to_string(),
        _ => detail.to_string(),
    }
}

/// Envía una notificación de escritorio con sonido — AION "quiere hablarte".
/// Se desactiva con AION_NOTIFY=0.
fn notify_user(title: &str, message: &str) {
    if std::env::var("AION_NOTIFY").as_deref() == Ok("0") {
        return;
    }
    // Escapar comillas dobles para AppleScript.
    let msg = message.replace('"', "'");
    let title = title.replace('"', "'");
    let script =
        format!("display notification \"{msg}\" with title \"{title}\" sound name \"Glass\"");
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}

/// Bootstrap de modelos en primer arranque — MULTIPLATAFORMA (Mac/Windows).
///
/// Usa el binario Ollama EMBEBIDO (ruta en `AION_OLLAMA_BIN`) para asegurar que
/// existan `gemma4-reason` (razonamiento) y `nomic-embed-text` (embeddings). Si
/// faltan, los descarga/crea (`ollama pull` / `ollama create -f Modelfile`) y avisa
/// al usuario. Idempotente: si ya existen, no hace nada. Reemplaza al script bash
/// para funcionar también en Windows (donde no hay bash).
fn run_models_ensure() {
    use std::process::Command;
    use std::time::Duration;

    let bin = std::env::var("AION_OLLAMA_BIN").unwrap_or_else(|_| "ollama".to_string());
    let modelfile = std::env::var("AION_MODELFILE").unwrap_or_default();
    const CHAT: &str = "gemma4-reason";
    const EMBED: &str = "nomic-embed-text";

    // OLLAMA_HOST se hereda del proceso padre (lo fija el shell de escritorio).
    let list = || -> Option<String> {
        Command::new(&bin)
            .arg("list")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };

    // Esperar a que el servidor Ollama embebido responda (máx ~60 s).
    let mut have = String::new();
    for _ in 0..60 {
        if let Some(out) = list() {
            have = out;
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    let need_chat = !have.contains(CHAT);
    let need_embed = !have.contains(EMBED);
    if !need_chat && !need_embed {
        tracing::info!("modelos ya presentes — bootstrap omitido");
        return;
    }

    notify_user(
        "AION",
        "Preparando la IA por primera vez (descarga ~9 GB). Te avisaré al terminar.",
    );
    tracing::info!(need_chat, need_embed, "descargando modelos (primer arranque)");

    if need_embed {
        let ok = Command::new(&bin)
            .args(["pull", EMBED])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            notify_user("AION", "Error descargando el modelo de embeddings.");
        }
    }

    if need_chat {
        let mut cmd = Command::new(&bin);
        cmd.args(["create", CHAT]);
        if !modelfile.is_empty() {
            cmd.args(["-f", &modelfile]);
        }
        let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
        if ok {
            notify_user("AION", "¡Listo! AION ya está preparado para conversar.");
            tracing::info!("modelo {CHAT} listo");
        } else {
            notify_user("AION", "Error preparando el modelo de IA.");
            tracing::error!("fallo al crear {CHAT}");
        }
    } else {
        notify_user("AION", "¡Listo! AION ya está preparado.");
    }
}

/// `see`: AION **mira la pantalla** (captura → Gemma visión) bajo el Governor.
async fn run_see(prompt: &str) -> Result<(), Box<dyn std::error::Error>> {
    let computer = aion_control::Computer::open(app_data_dir().join("control"))?;
    println!("👁  AION captura la pantalla…");
    let b64 = computer
        .look()
        .map_err(|e| format!("no pude ver la pantalla ({e}). En macOS, concede 'Grabación de pantalla' a AION."))?;

    let model = std::env::var("AION_VISION_MODEL")
        .unwrap_or_else(|_| "huihui_ai/gemma-4-abliterated:12b".into());
    let engine = OllamaEngine::new(OllamaEngine::base_url_from_env(), &model);
    engine
        .health()
        .await
        .map_err(|e| format!("LLM local no disponible ({e})."))?;

    println!("🧑 {prompt}\n");
    let msg = engine.generate_with_image(prompt, &b64).await?;
    println!("💬 {}", msg.content.trim());
    Ok(())
}

/// `governance`: ver y ajustar las reglas (postura, pausa/kill switch, audit, papelera).
fn run_governance(args: &[String]) {
    let dir = app_data_dir().join("control");
    let mut gov = match aion_computer::Governor::open(&dir) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("no pude abrir la gobernanza: {e}");
            return;
        }
    };
    match args.first().map(String::as_str) {
        Some("pause") => {
            let _ = gov.set_paused(true);
            println!("🛑 AION EN PAUSA (kill switch): se deniega toda acción.");
        }
        Some("resume") => {
            let _ = gov.set_paused(false);
            println!("▶️  AION reanudado.");
        }
        Some("posture") => {
            let p = match args.get(1).map(String::as_str) {
                Some("conservative") => aion_computer::Posture::Conservative,
                Some("balanced") => aion_computer::Posture::Balanced,
                Some("max") => aion_computer::Posture::MaxAutonomy,
                _ => {
                    println!("uso: governance posture <conservative|balanced|max>");
                    return;
                }
            };
            let _ = gov.set_posture(p);
            println!("✅ postura cambiada a {p:?}");
        }
        Some("audit") => {
            let recs = gov.audit().all().unwrap_or_default();
            let n = recs.len();
            println!("📜 audit log: {n} acciones (últimas 15)");
            for r in recs.iter().rev().take(15).rev() {
                let dec = match &r.decision {
                    aion_computer::Decision::Allow { .. } => "🟢 permitida",
                    aion_computer::Decision::Confirm { .. } => "🟡 confirmar",
                    aion_computer::Decision::Deny { .. } => "🔴 denegada",
                };
                println!(
                    "  {} · {dec} · {} · {}",
                    r.at.format("%H:%M:%S"),
                    r.action.verb,
                    r.action.summary
                );
            }
        }
        Some("trash") => {
            let entries = gov.trash().entries().unwrap_or_default();
            println!("🗑  papelera AION: {} elementos (recuperables 30 días)", entries.len());
            for e in &entries {
                println!("  [{}] {} · borrado {}", &e.id[..8], e.original_path, e.deleted_at.format("%Y-%m-%d"));
            }
        }
        _ => {
            let p = &gov.policy;
            println!("⚖️  Gobernanza de AION");
            println!("  Postura : {:?}", p.posture);
            println!("  Pausa   : {}", if p.paused { "SÍ (kill switch)" } else { "no" });
            println!("  Rutas protegidas : {}", p.protected_paths.len());
            println!("  Líneas rojas     : {}", p.hard_deny.len());
            println!("\nComandos: governance [pause|resume|posture <p>|audit|trash]");
        }
    }
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
