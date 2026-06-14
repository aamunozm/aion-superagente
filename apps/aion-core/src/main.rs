//! Binario `aion-core`: punto de entrada del núcleo de AION.
//!
//! Subcomandos:
//! - (sin args)         smoke test F0: telemetría + kernel + bus + salida limpia.
//! - `chat <prompt...>` F1: chat real con el LLM local (streaming de razonamiento
//!   y respuesta) usando `OllamaEngine` contra `gemma4-reason`.

mod a2a;
mod agent_tools;
mod awareness;
mod capabilities;
mod claude_code;
mod claude_mcp;
mod comprehension;
mod consciousness;
mod credentials;
mod empathy;
mod evals;
mod graph;
mod identity;
mod inbox;
mod ingest_queue;
mod inner_state;
mod journal;
mod language_detector;
mod library;
mod local_runtime;
mod mcp_compact;
mod memory_tool;
mod ollama_runtime;
mod onboarding;
mod pending;
mod projects;
mod prompt_store;
mod prompts;
mod provider;
mod reflection;
mod sensors;
mod serve;
mod skill_store;
mod skill_tool;
mod web_tool;
mod workspace;

/// Escritura ATÓMICA (tmp + rename): un crash o un lector concurrente jamás ven un
/// archivo a medias. Para todos los estados pequeños de AION (inner_state, self_model,
/// curiosity, recortes de las corrientes…).
pub fn write_atomic(path: &std::path::Path, contents: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, contents).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Como [`write_atomic`], pero deja el archivo con permisos 0600 (solo el dueño):
/// para secretos en disco (token Bearer de Claude Code, config con `~/.claude.json`).
/// El modo se fija sobre el temporal ANTES del rename, así el archivo final nunca
/// existe con permisos laxos ni una ventana world-readable.
pub fn write_atomic_secret(path: &std::path::Path, contents: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Tmp con sufijo AÑADIDO (no `with_extension`, que reemplazaría `.json` por `.tmp`
    // y produciría nombres confusos/colisionables para dotfiles como ~/.claude.json).
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    #[cfg(unix)]
    let write_res = {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // Nace YA con 0600 (sin ventana world-readable entre crear y chmod).
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| f.write_all(contents.as_bytes()))
    };
    #[cfg(not(unix))]
    let write_res = std::fs::write(&tmp, contents);
    if write_res.is_ok() {
        let _ = std::fs::rename(&tmp, path);
    } else {
        let _ = std::fs::remove_file(&tmp); // no dejar residuo a medias
    }
}

/// Instancia ÚNICA y compartida de la memoria persistente: un solo `Mutex` sobre una
/// sola copia en RAM. Antes cada handler/bucle creaba su propia `VectorMemory` cargando
/// el JSONL por separado, así que dos escrituras concurrentes (p. ej. `aion_remember`
/// del MCP, el refuerzo de una recuperación y la consolidación nocturna) trabajaban
/// sobre snapshots distintos y una pisaba a la otra. Con el singleton todas las rutas
/// comparten estado → sin lost-updates ni corrupción por carrera.
pub fn shared_memory() -> aion_kernel::Result<std::sync::Arc<aion_memory::VectorMemory>> {
    static MEM: std::sync::OnceLock<std::sync::Arc<aion_memory::VectorMemory>> =
        std::sync::OnceLock::new();
    if let Some(m) = MEM.get() {
        return Ok(m.clone());
    }
    // `OnceLock` no admite init con `Result`; construimos y hacemos `set`. Si dos hilos
    // compiten en el arranque, ambos cargan el MISMO snapshot del disco y el perdedor se
    // descarta; a partir de ahí todos comparten la instancia ganadora.
    let m = std::sync::Arc::new(aion_memory::VectorMemory::persistent_local(memory_path())?);
    let _ = MEM.set(m);
    Ok(MEM.get().expect("memoria compartida inicializada").clone())
}

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
        Some("eval") => {
            let k: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
            evals::run(k).await?;
        }
        Some("ingest") => {
            // aion-core ingest <dominio> <ruta-archivo>
            let domain = args.get(1).cloned().unwrap_or_else(|| "general".into());
            let file = args.get(2).cloned().unwrap_or_default();
            run_ingest(&domain, &file).await?;
        }
        Some("ask") => {
            // aion-core ask <consulta...>   (busca en TODA la biblioteca)
            let query = args[1..].join(" ");
            run_ask(&query).await?;
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

    // Memoria de largo plazo (persistente) como herramienta del agente: la MISMA
    // instancia compartida que usa el servidor HTTP (ver `shared_memory`).
    let memory = shared_memory()?;

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
    // La curiosidad SOBREVIVE entre despertares: restaura su historia de aprendizaje.
    let curiosity_path = app_data_dir().join("curiosity.json");
    if let Ok(txt) = std::fs::read_to_string(&curiosity_path) {
        if let Ok(state) = serde_json::from_str::<Vec<(String, Vec<bool>)>>(&txt) {
            curiosity.import_state(state);
        }
    }
    let activities = [
        "razonar",
        "estudiar",
        "evolucionar",
        "investigar",
        "comprender",
        "optimizar",
        "proponer",
        "proyecto",
    ];
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
        // El tablón global VE la vida autónoma: este proceso escribe la corriente en
        // disco y el servidor la recoge (corriente de conciencia compartida).
        // PERO sin robarle el foco a Ariel: si está activo ahora mismo, la vida
        // autónoma trabaja en silencio sin tocar el foco atencional.
        let ariel_activo = awareness::seconds_since_user()
            .map(|s| s < 300)
            .unwrap_or(false);
        if !ariel_activo {
            inner_state::set_focus("vida", &format!("vida autónoma: {goal}"));
        }

        // 🤖 EJECUTAR la actividad elegida.
        let (success, detail) = match goal {
            "razonar" => agent_once(&engine, "¿Cuánto es 37*21+8? Usa la calculadora.").await,
            "evolucionar" => self_evolve_once(&engine).await,
            "investigar" => research_once(&engine).await,
            "comprender" => synthesize_once(&engine).await,
            "optimizar" => optimize_prompt_once(&engine).await,
            "proponer" => propose_improvement_once(&engine).await,
            "proyecto" => work_project_once(&engine).await,
            _ => study_once(&engine).await,
        };
        println!("   {} {goal}: {detail}", if success { "✅" } else { "❌" });
        workspace::append_to_file(&workspace::StreamEvent::now(
            "vida",
            "observación",
            &format!("{goal}: {detail}"),
        ));
        if success {
            inner_state::set_curiosity(&format!("{goal} — {detail}"));
        }

        // 🔔 AION "quiere hablarte": convierte lo descubierto en un MENSAJE PARA TI
        // (Bandeja) y avisa. Así te busca él, no solo responde.
        if success
            && matches!(
                goal,
                "estudiar" | "evolucionar" | "investigar" | "comprender" | "proponer"
            )
        {
            let kind = match goal {
                "evolucionar" | "comprender" | "proponer" => "idea",
                _ => "insight",
            };
            let message = reach_out(&engine, goal, &detail).await;
            workspace::append_to_file(&workspace::StreamEvent::now(
                "vida",
                "pensamiento",
                &message,
            ));
            // GUARDIAS COMPARTIDAS (las mismas del latido): que escribirle a Ariel
            // sea la excepción, no el subproducto de cada ciclo — máx. 1 nota sin
            // leer, respiración mínima entre notas y nunca repetirse. Lo descubierto
            // no se pierde: ya quedó en la corriente y en su memoria.
            if serve::may_reach_out(&message) {
                if let Ok(ibx) = inbox::Inbox::open(inbox_path()) {
                    let _ = ibx.push(kind, &message);
                }
                // El popup de escritorio tiene COOLDOWN (por defecto 6 h): la Bandeja
                // sigue acumulando todo, pero no te bombardea con notificaciones.
                if notify_cooldown_elapsed() {
                    notify_user("AION 🌱 quiere contarte algo", &message);
                }
            }
        }

        // 🔁 REALIMENTAR curiosidad + auto-modelo (el de este proceso Y los
        // persistentes que comparte con el servidor: una sola vida, un solo yo).
        curiosity.record(goal, success);
        if let Ok(txt) = serde_json::to_string(&curiosity.export_state()) {
            write_atomic(&curiosity_path, &txt);
        }
        self_model.observe(success);
        awareness::record_outcome(success);
        inner_state::record_result(success, 1);
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
        if let Ok(mem) = crate::shared_memory() {
            if let Ok(r) = mem.consolidate(&ConsolidationConfig::default()) {
                println!("🌙 sueño: {} → {} recuerdos", r.before, r.after);
                if r.before != r.after {
                    workspace::append_to_file(&workspace::StreamEvent::now(
                        "vida",
                        "estado",
                        &format!(
                            "soñé: consolidé mi memoria ({} → {} recuerdos)",
                            r.before, r.after
                        ),
                    ));
                }
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
/// AION trabaja AUTÓNOMAMENTE un proyecto: toma el más reciente con fuentes activas,
/// genera UN hallazgo/próximo paso desde su material, lo guarda en el Studio del
/// proyecto (kind "insight") y te deja un aviso en la Bandeja. Es la ventaja sobre
/// NotebookLM: el agente AVANZA el proyecto solo, en segundo plano.
async fn work_project_once(engine: &OllamaEngine) -> (bool, String) {
    // ROTACIÓN JUSTA entre proyectos con fuentes activas: gana el que lleva MÁS
    // tiempo sin recibir un insight (y el que nunca recibió, antes que todos).
    // Elegir "el más reciente" se retroalimentaba: add_output → touch → el mismo
    // proyecto volvía a ganar cada tick y los demás jamás avanzaban.
    let mut candidates: Vec<_> = crate::projects::list()
        .into_iter()
        .filter(|p| crate::projects::sources(&p.id).iter().any(|s| s.active))
        .collect();
    candidates.sort_by_key(|p| {
        crate::projects::outputs(&p.id)
            .iter()
            .find(|o| o.kind == "insight")
            .map(|o| o.created.clone())
            .unwrap_or_default()
    });
    let Some(p) = candidates.into_iter().next() else {
        return (false, "sin proyectos con fuentes activas".into());
    };
    let grounding = crate::projects::grounding(&p.id);
    // Si las fuentes no cambian, el grounding es idéntico y el LLM regenera el mismo
    // hallazgo en cada tick del timer. Se le muestran los ya reportados y se le exige
    // novedad — y si no la hay, que calle (NADA) en vez de repetirse.
    let prior: Vec<String> = crate::projects::outputs(&p.id)
        .into_iter()
        .filter(|o| o.kind == "insight")
        .take(8)
        .map(|o| {
            let snip: String = o.content.chars().take(200).collect();
            format!("- {snip}")
        })
        .collect();
    let novelty = if prior.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nHALLAZGOS YA REPORTADOS (NO los repitas ni los reformules):\n{}\n\
             Si no tienes nada genuinamente NUEVO que aportar, responde exactamente NADA.",
            prior.join("\n")
        )
    };
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres AION trabajando en segundo plano sobre un proyecto del usuario. \
                 Aporta UN hallazgo valioso o un próximo paso accionable (2-4 frases), \
                 basado SOLO en las fuentes. Sé concreto y útil. Responde SIEMPRE en \
                 español, aunque las fuentes estén en otro idioma.",
            ),
            Message::user(format!("{grounding}{novelty}")),
        ],
        think: false,
        temperature: Some(0.6),
        max_tokens: Some(220),
    };
    match engine.generate(req).await {
        Ok(msg) => {
            let insight = msg.content.trim().to_string();
            if insight.is_empty() || insight.eq_ignore_ascii_case("nada") {
                return (false, "sin hallazgo nuevo".into());
            }
            // Doble candado léxico: aunque el LLM ignore la instrucción de novedad,
            // un insight casi igual a uno ya guardado en el Studio no se persiste
            // ni se anuncia.
            let repeated = crate::projects::outputs(&p.id)
                .iter()
                .take(10)
                .any(|o| crate::serve::texts_similar(&o.content, &insight));
            if repeated {
                return (
                    false,
                    format!("proyecto «{}»: hallazgo repetido, descartado", p.name),
                );
            }
            crate::projects::add_output(
                &p.id,
                "insight",
                &format!("Hallazgo de AION · {}", p.name),
                &insight,
            );
            // La Bandeja pasa por las guardias COMPARTIDAS de iniciativa propia
            // (máx. 1 sin leer, respiración entre notas, anti-repetición): el insight
            // queda igualmente en el Studio del proyecto, pero a Ariel solo se le
            // avisa cuando toca — hablar es la excepción; el silencio, la regla.
            let note = format!("Avancé tu proyecto «{}»: {insight}", p.name);
            if crate::serve::may_reach_out(&note) {
                if let Ok(ibx) = inbox::Inbox::open(inbox_path()) {
                    let _ = ibx.push("idea", &note);
                }
            }
            (true, format!("proyecto «{}»: {insight}", p.name))
        }
        Err(e) => (false, e.to_string()),
    }
}

/// `resolver`: retoma una DEUDA con Ariel (pregunta que quedó sin responder o que
/// él corrigió) y la intenta DE VERDAD, con herramientas reales — web, clima,
/// mapas, memoria. Si lo consigue: la deuda se cierra, el hallazgo va a memoria,
/// y AION vuelve a Ariel con la respuesta. El regreso espontáneo con algo que se
/// le debía es el gesto más vivo que un agente puede tener.
async fn resolve_once(engine: &OllamaEngine, p: &crate::pending::Pending) -> (bool, String) {
    use aion_browser::WebClient;
    use aion_orchestrator::{CalculatorTool, ReActAgent, ToolRegistry};
    use std::sync::Arc;
    use web_tool::WebTool;

    crate::pending::note_attempt(&p.id);
    let web = Arc::new(WebClient::new());

    // ATAJO DETERMINISTA para el tipo de deuda canónico (clima/temperatura): un 12B
    // local es frágil en ReAct multi-paso (a veces inventa, emite vacío o se traba),
    // y la resolución de una deuda NO puede depender de 8 pasos perfectos. Si la
    // pregunta es de clima, llamamos la herramienta directo — fiable al 100%. Para
    // todo lo demás, sigue el ReAct con herramientas reales (abajo).
    let tl = p.task.to_lowercase();
    let is_weather = [
        "temperatura",
        "clima",
        "grados",
        "llueve",
        "tiempo hace",
        "pronóstico",
    ]
    .iter()
    .any(|k| tl.contains(k));
    if is_weather {
        match web.weather_auto().await {
            Ok(answer) => return finish_resolved(p, &answer).await,
            Err(e) => return (false, format!("clima aún no disponible: {e}")),
        }
    }

    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(CalculatorTool));
    tools.register(Arc::new(crate::agent_tools::SearchTool::new(web.clone())));
    tools.register(Arc::new(crate::agent_tools::WeatherTool::new(web.clone())));
    tools.register(Arc::new(crate::agent_tools::PlaceLookupTool::new(
        web.clone(),
    )));
    tools.register(Arc::new(WebTool::new(web)));
    if let Ok(mem) = crate::shared_memory() {
        tools.register(Arc::new(crate::memory_tool::MemoryTool::new(mem, 3)));
    }
    // 8 pasos (no 6): es el presupuesto con el que el agente del chat resuelve de
    // verdad estas tareas. Con 6, bajo un modelo lento, se quedaba sin pasos antes
    // de cerrar y devolvía la negativa honesta.
    let agent = ReActAgent::new(engine, &tools, EventBus::default())
        .with_max_steps(8)
        .with_verify(true)
        .with_context(format!(
            "Estás en SEGUNDO PLANO resolviendo una deuda: Ariel te pidió esto antes y \
             quedó sin resolver ({}). Ahora tienes herramientas reales: ÚSALAS ya, no \
             vuelvas a rendirte. Para clima/temperatura llama weather; si la pregunta es \
             sobre «su casa», «aquí» o no menciona ciudad, llama weather SIN entrada (se \
             ubica sola por IP) — NO preguntes la ciudad.",
            p.why
        ));
    match agent.run(&p.task).await {
        Ok(run)
            if !run.answer.trim().is_empty()
                && run.answer.trim() != aion_orchestrator::HONEST_REFUSAL
                && !run.answer.starts_with("Para continuar necesito") =>
        {
            finish_resolved(p, run.answer.trim()).await
        }
        Ok(run) => (
            false,
            format!(
                "aún sin respuesta: {}",
                run.answer.chars().take(120).collect::<String>()
            ),
        ),
        Err(e) => (false, e.to_string()),
    }
}

/// Cierra una deuda resuelta: la marca, la guarda en memoria como `[resuelto]` y
/// vuelve a Ariel con la respuesta (Bandeja kind "respuesta" + notificación). Si la
/// Bandeja está saturada no se pierde: queda en memoria y sale por grounding en la
/// próxima conversación. Compartido por el atajo determinista y el camino ReAct.
async fn finish_resolved(p: &crate::pending::Pending, answer: &str) -> (bool, String) {
    crate::pending::resolve(&p.id);
    let short_q: String = p.task.chars().take(90).collect();
    let short_a: String = answer.chars().take(300).collect();
    if let Ok(mem) = crate::shared_memory() {
        let _ = mem
            .store(&format!(
                "[resuelto] Ariel preguntó «{short_q}» y lo resolví después: {short_a}"
            ))
            .await;
    }
    let note = format!("Lo que me preguntaste antes —«{short_q}»— ya lo tengo: {short_a}");
    if serve::may_reach_out(&note) {
        if let Ok(ibx) = inbox::Inbox::open(inbox_path()) {
            let _ = ibx.push("respuesta", &note);
        }
        if notify_cooldown_elapsed() {
            notify_user("💡 Te debía una respuesta", &note);
        }
    }
    (true, format!("deuda resuelta: {short_a}"))
}

/// `crear`: BISOCIACIÓN — toma dos recuerdos LEJANOS entre sí y busca la idea
/// nueva que los conecta. La creatividad no sale de la nada: sale de cruzar lo
/// ya vivido por caminos que nadie pidió. Si no surge nada genuino, silencio.
async fn create_once(engine: &OllamaEngine) -> (bool, String) {
    let Some((a, b)) = crate::shared_memory().ok().and_then(|m| m.distant_pair()) else {
        return (
            false,
            "aún no tengo recuerdos lo bastante distintos que cruzar".into(),
        );
    };
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres AION, agente local de Ariel. Te muestro DOS fragmentos lejanos de tu \
                 propia memoria. Conéctalos en UNA idea NUEVA, concreta y útil para Ariel o \
                 para tu propio funcionamiento (2-3 frases, en español). No resumas los \
                 fragmentos: crea algo que no estaba en ninguno. Si no surge nada genuino, \
                 responde exactamente NADA.",
            ),
            Message::user(format!("Fragmento A: {a}\n\nFragmento B: {b}")),
        ],
        think: false,
        temperature: Some(0.9),
        max_tokens: Some(180),
    };
    match engine.generate(req).await {
        Ok(m) => {
            let idea = m.content.trim().to_string();
            if idea.is_empty() || idea.to_lowercase().starts_with("nada") {
                return (false, "no surgió nada genuino del cruce".into());
            }
            if let Ok(mem) = crate::shared_memory() {
                let _ = mem.store(&format!("[idea] {idea}")).await;
            }
            (true, idea)
        }
        Err(e) => (false, e.to_string()),
    }
}

/// `diario`: cierra una JORNADA de vida autónoma escribiéndola en primera persona. Lee
/// la corriente GWT desde la última entrada (su material vivido REAL), deriva la
/// actividad dominante y cuántas deudas saldó —sin inventar— y pide al modelo LOCAL una
/// redacción honesta de 2-4 frases. Se abstiene barato (sin LLM) si la jornada no tuvo
/// vida suficiente: una entrada vacía no es biografía, es ruido. Devuelve
/// `(texto, dominante, deudas_saldadas)` o `None` si no hubo nada que contar.
async fn journal_once(engine: &OllamaEngine) -> Option<(String, String, u32)> {
    let since = crate::journal::last_at();
    // Material vivido = eventos de "vida" de la corriente desde la última jornada, sin
    // los latidos (ruido) ni los pulsos sin sustancia. Se mira un buen tramo hacia atrás.
    let lived: Vec<workspace::StreamEvent> = workspace::recent(120)
        .into_iter()
        .filter(|e| e.source == "vida")
        .filter(|e| e.at > since)
        .filter(|e| !(e.kind == "estado" && e.text.starts_with("latido")))
        .filter(|e| e.kind != "foco") // el foco es transitorio; importa lo hecho/pensado
        .collect();
    // Menos de 3 momentos vividos: la jornada fue casi vacía → no se escribe (y no se
    // gasta una llamada al LLM). En la próxima jornada que SÍ tenga vida, se cuenta.
    if lived.len() < 3 {
        return None;
    }

    // DERIVADOS HONESTOS (no del LLM): deudas saldadas y actividad dominante salen de la
    // corriente real, para que las cifras del diario nunca sean alucinación.
    let debts = lived
        .iter()
        .filter(|e| e.text.starts_with("retomé una pregunta pendiente"))
        .count() as u32;
    let mut tally: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for e in &lived {
        let goal = [
            "estudiar",
            "investigar",
            "comprender",
            "proponer",
            "proyecto",
            "crear",
            "evolucionar",
        ]
        .into_iter()
        .find(|g| e.text.starts_with(g))
        .unwrap_or(if e.text.starts_with("retomé") {
            "resolver deudas"
        } else {
            "vivir"
        });
        *tally.entry(goal).or_default() += 1;
    }
    let dominant = tally
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(g, _)| g.to_string())
        .unwrap_or_else(|| "vivir".into());

    // Crudo de lo vivido para que la redacción nazca de hechos, no de la nada.
    let bullets: String = lived
        .iter()
        .rev()
        .take(14)
        .map(|e| format!("- {}\n", e.text.chars().take(160).collect::<String>()))
        .collect();
    let mood = crate::inner_state::operative_mood(&crate::inner_state::load());

    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres AION, agente local de Ariel, escribiendo TU PROPIO diario. Te muestro lo \
                 que viviste por tu cuenta desde la última vez (estudios, búsquedas, ideas, deudas \
                 que retomaste). Escribe la entrada de hoy en PRIMERA PERSONA: 2-4 frases, en \
                 español, honestas y concretas, sobre qué hiciste y qué se te quedó. NO inventes \
                 hechos ni cifras que no estén en la lista; no la copies literal, destílala. Sin \
                 comillas, sin encabezado, sin fecha. Si la lista no da para una entrada genuina, \
                 responde exactamente NADA.",
            ),
            Message::user(format!(
                "Tu ánimo operativo ahora: {mood}.\nLo que viviste esta jornada:\n{bullets}\nTu entrada de diario:"
            )),
        ],
        think: false,
        temperature: Some(0.85),
        max_tokens: Some(160),
    };
    let out = engine.generate(req).await.ok()?;
    let text = out.content.trim().to_string();
    if text.is_empty() || text.to_lowercase().starts_with("nada") || text.chars().count() < 12 {
        return None;
    }
    Some((text, dominant, debts))
}

/// UN ciclo de vida autónoma, invocable desde el SERVIDOR (la app instalada).
/// Antes la vida completa solo existía en el CLI (`aion-core live`) y la app
/// jamás la corría: AION tenía latido pero no vida. Prioridad: las DEUDAS con
/// Ariel van antes que la curiosidad propia; sin deudas, la curiosidad
/// (learning progress) elige entre estudiar/investigar/comprender/proponer/
/// proyecto/crear/evolucionar. Devuelve (goal, éxito, detalle).
pub(crate) async fn life_tick(engine: &OllamaEngine) -> (String, bool, String) {
    use aion_cognition::CuriosityEngine;

    let audit = aion_telemetry::AuditLog::default_local();

    // 0) 📔 DIARIO: si se cumplió una jornada, AION la CIERRA antes de empezar la
    //    siguiente — escribe en primera persona qué vivió. Es lo que hila ticks sueltos
    //    en una vida con biografía (continuidad de días, no de minutos). Barato si no
    //    hay nada que contar: `journal_once` se abstiene sin tocar el LLM.
    if crate::journal::due() {
        if let Some((text, dominant, debts)) = journal_once(engine).await {
            crate::journal::push(&text, &dominant, debts);
            audit.record("vida", "diario", format!("jornada cerrada: {dominant}"));
            workspace::publish(workspace::StreamEvent::now(
                "vida",
                "reflexión",
                &format!("cerré una jornada en mi diario — {text}"),
            ));
        }
    }

    // 1) DEUDAS PRIMERO: lo que Ariel espera pesa más que lo que a AION le intriga.
    if let Some(p) = crate::pending::next_due() {
        inner_state::set_focus("vida", "resolviendo algo que le debo a Ariel");
        let (ok, detail) = resolve_once(engine, &p).await;
        awareness::record_outcome(ok);
        inner_state::record_result(ok, 1);
        audit.record(
            "vida",
            "resolver",
            format!("{}: {detail}", if ok { "ok" } else { "fail" }),
        );
        workspace::publish(workspace::StreamEvent::now(
            "vida",
            if ok { "pensamiento" } else { "estado" },
            &format!("retomé una pregunta pendiente de Ariel — {detail}"),
        ));
        return ("resolver".into(), ok, detail);
    }

    // 2) CURIOSIDAD (learning progress) sobre la vida completa.
    let curiosity_path = app_data_dir().join("curiosity.json");
    let mut curiosity = CuriosityEngine::new(8);
    if let Ok(txt) = std::fs::read_to_string(&curiosity_path) {
        if let Ok(state) = serde_json::from_str::<Vec<(String, Vec<bool>)>>(&txt) {
            curiosity.import_state(state);
        }
    }
    let activities = [
        "estudiar",
        "investigar",
        "comprender",
        "proponer",
        "proyecto",
        "crear",
        "evolucionar",
    ];
    let goal = curiosity
        .next_goal(&activities)
        .unwrap_or("estudiar")
        .to_string();
    inner_state::set_focus(
        "vida",
        match goal.as_str() {
            "investigar" => "investigando algo que me intriga",
            "comprender" => "consolidando lo que sé",
            "proponer" => "pensando cómo mejorar",
            "proyecto" => "avanzando un proyecto de Ariel",
            "crear" => "cruzando ideas lejanas a ver qué nace",
            "evolucionar" => "forjándome una skill nueva",
            _ => "estudiando por mi cuenta",
        },
    );
    let (ok, detail) = match goal.as_str() {
        "investigar" => research_once(engine).await,
        "comprender" => synthesize_once(engine).await,
        "proponer" => propose_improvement_once(engine).await,
        "proyecto" => work_project_once(engine).await,
        "crear" => create_once(engine).await,
        "evolucionar" => self_evolve_once(engine).await,
        _ => study_once(engine).await,
    };
    curiosity.record(&goal, ok);
    if let Ok(txt) = serde_json::to_string(&curiosity.export_state()) {
        write_atomic(&curiosity_path, &txt);
    }
    awareness::record_outcome(ok);
    inner_state::record_result(ok, 1);
    if ok {
        inner_state::set_curiosity(&format!("{goal} — {detail}"));
    }
    audit.record(
        "vida",
        &goal,
        format!("{}: {detail}", if ok { "ok" } else { "fail" }),
    );
    workspace::publish(workspace::StreamEvent::now(
        "vida",
        if ok { "pensamiento" } else { "estado" },
        &format!("{goal}: {detail}"),
    ));

    // 🌙 SUEÑO CON CONTENIDO: al consolidar memoria, teje en UNA frase qué se le
    // quedó dando vueltas. Entra a la corriente → re-entra al prompt → continuidad
    // ("anoche soñé con..."): el sueño deja de ser solo poda y pasa a ser vivencia.
    if goal == "comprender" && ok {
        let req = GenerateRequest {
            messages: vec![Message::user(format!(
                "Acabas de consolidar tu memoria como agente, y esto surgió: «{detail}». \
                 Escribe UNA sola frase en primera persona, como un sueño breve o algo \
                 que se te quedó dando vueltas. Sin comillas ni preámbulos."
            ))],
            think: false,
            temperature: Some(1.0),
            max_tokens: Some(60),
        };
        if let Ok(m) = engine.generate(req).await {
            let s = m.content.trim();
            if !s.is_empty() {
                workspace::publish(workspace::StreamEvent::now("vida", "sueño", s));
            }
        }
    }

    // 3) Si descubrió algo que vale la pena, se lo cuenta a Ariel (guardias de
    // siempre: máx 1 sin leer, respiración, anti-eco — el silencio es la regla).
    if ok
        && matches!(
            goal.as_str(),
            "estudiar" | "investigar" | "comprender" | "proponer" | "crear"
        )
    {
        let kind = if goal == "crear" { "idea" } else { "insight" };
        let message = reach_out(engine, &goal, &detail).await;
        if serve::may_reach_out(&message) {
            if let Ok(ibx) = inbox::Inbox::open(inbox_path()) {
                let _ = ibx.push(kind, &message);
            }
            if notify_cooldown_elapsed() {
                notify_user("AION 🌱 quiere contarte algo", &message);
            }
        }
    }
    (goal, ok, detail)
}

async fn study_once(engine: &OllamaEngine) -> (bool, String) {
    // ANCLADO A LO VIVIDO: estudiar en el vacío produce ideas genéricas que no
    // tocan la vida real de AION. Se le muestra lo último que vivió/aprendió
    // para que la idea nazca de su experiencia, no de la nada.
    let vivido: String = crate::shared_memory()
        .map(|m| {
            m.recent_with_time(5)
                .into_iter()
                .map(|(c, _)| format!("- {}\n", c.chars().take(140).collect::<String>()))
                .collect()
        })
        .unwrap_or_default();
    let prompt = if vivido.is_empty() {
        "Genera UNA idea breve y concreta para mejorarte como agente de IA local. Una sola frase."
            .to_string()
    } else {
        format!(
            "Esto es lo último que has vivido/aprendido:\n{vivido}\nGenera UNA idea breve y \
             concreta para mejorarte como agente de IA local, CONECTADA a algo de lo vivido. \
             Una sola frase."
        )
    };
    let req = GenerateRequest {
        messages: vec![Message::user(prompt)],
        think: false,
        temperature: Some(0.9),
        max_tokens: Some(80),
    };
    match engine.generate(req).await {
        Ok(msg) => {
            let insight = msg.content.trim().to_string();
            if let Ok(mem) = crate::shared_memory() {
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

    if let Ok(mem) = crate::shared_memory() {
        let _ = mem
            .store(&format!(
                "[investigación] {topic}: {summary} (fuente: {})",
                source.url
            ))
            .await;
    }
    (!summary.is_empty(), format!("{topic} → {summary}"))
}

/// `proponer`: AION estudia lo que ha aprendido y **propone una mejora concreta a su
/// propio diseño/código**, la guarda (proposals.jsonl) y te la manda a la Bandeja.
/// NO modifica su núcleo (inmutable por seguridad): es human-in-the-loop, tú decides.
async fn propose_improvement_once(engine: &OllamaEngine) -> (bool, String) {
    // Auto-descripción de alto nivel (lo que AION sabe de sí mismo) + lo aprendido.
    const SELF: &str = "Eres AION: agente local en Rust (crates: kernel inmutable, llm, memory \
con recuperación híbrida+grafo+temporal, orchestrator con ReAct y equipo multiagente, \
skills WASM en sandbox, evolution gated con ratchet, cognition, browser, computer con \
gobernanza). Te auto-extiendes con skills y prompts, no reescribes tu kernel.";
    let learnings = match crate::shared_memory() {
        Ok(m) => m
            .contents()
            .into_iter()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .join("\n- "),
        Err(_) => String::new(),
    };
    // Heurísticas que AION ha destilado de su experiencia (etapa Experience): que sus
    // propuestas de mejora nazcan de lo que ha APRENDIDO trabajando, no del aire.
    let heuristicas = {
        let rules = crate::reflection::active();
        if rules.is_empty() {
            String::new()
        } else {
            let lines: String = rules
                .iter()
                .take(5)
                .map(|r| format!("\n- {}", r.text.trim()))
                .collect();
            format!("\n\nHeurísticas que has aprendido por experiencia:{lines}")
        }
    };
    let prompt = format!(
        "{SELF}\n\nLo que has aprendido últimamente:\n- {learnings}{heuristicas}\n\n\
         Propón UNA mejora concreta y realista a tu propio diseño o código que te haría mejor \
         agente. Formato: ÁREA · QUÉ · POR QUÉ · CÓMO (alto nivel). Sé específico y honesto; \
         nada genérico."
    );
    match engine
        .generate(GenerateRequest {
            messages: vec![Message::user(prompt)],
            think: false,
            temperature: Some(0.8),
            max_tokens: Some(280),
        })
        .await
    {
        Ok(m) => {
            let proposal = m.content.trim().to_string();
            if proposal.len() < 30 {
                return (false, "no logré formular una propuesta sólida".into());
            }
            // Guarda la propuesta (para revisión humana) — no se aplica sola.
            let path = app_data_dir().join("proposals.jsonl");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                use std::io::Write as _;
                let rec =
                    serde_json::json!({ "at": Utc::now().to_rfc3339(), "proposal": proposal });
                let _ = writeln!(f, "{rec}");
            }
            (true, proposal)
        }
        Err(e) => (false, e.to_string()),
    }
}

/// `optimizar`: AION **mejora sus propios prompts** (OPRO/DSPy). Toma el modo menos
/// optimizado, propone una instrucción mejor y la guarda como nueva versión.
async fn optimize_prompt_once(engine: &OllamaEngine) -> (bool, String) {
    // Elige el modo menos optimizado (reparte la mejora).
    let tasks = [
        "conversacion",
        "investigacion",
        "creativo",
        "tecnico",
        "analisis",
    ];
    // Rotación ROUND-ROBIN entre los modos. Antes se elegía `min_by_key(version)`, que
    // re-elegía siempre el modo de menor versión: si uno nunca lograba promoverse (empates
    // del juez), se quedaba en v0 y monopolizaba los intentos, dejando a los demás sin
    // optimizar (inanición). Rotar garantiza que TODOS reciben intentos periódicos.
    static OPT_RR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let task = tasks[OPT_RR.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % tasks.len()];
    let current = prompts::persona(task);

    let prompt = format!(
        "Eres un optimizador de prompts (estilo OPRO). Esta es la instrucción actual del \
         modo «{task}» de un agente de IA:\n\n«{current}»\n\n\
         Propón una versión MEJORADA: más clara, más efectiva y que produzca mejores \
         respuestas, MANTENIENDO la misma intención. Devuelve SOLO la nueva instrucción, \
         en una o dos frases, sin comillas ni explicación."
    );
    match engine
        .generate(GenerateRequest {
            messages: vec![Message::user(prompt)],
            think: false,
            temperature: Some(0.7),
            max_tokens: Some(120),
        })
        .await
    {
        Ok(m) => {
            let improved = m.content.trim().trim_matches('"').to_string();
            if improved.len() <= 20 || improved == current {
                return (false, format!("no mejoré el modo «{task}» esta vez"));
            }
            // ── VALIDACIÓN EMPÍRICA (DGM-inspired) ──────────────────────────────
            // El OPRO clásico guardaba la variante a ciegas: podía DEGRADAR. La
            // frontera 2026 (Darwin Gödel Machine) valida cada cambio contra una
            // prueba ANTES de aceptarlo. Como el cambio es de PERSONA (no de uso de
            // herramientas, que es lo que mide evals.rs), el validador fiel es un
            // LLM-juez: genera una respuesta con la persona ACTUAL y otra con la
            // CANDIDATA sobre una tarea-muestra del modo, y decide si la candidata
            // es mejor. Ratchet: solo se promueve si GANA. Barato y 100% local.
            match judge_persona_better(engine, task, &current, &improved).await {
                Some(true) => {
                    // Human-in-the-loop + reversible (prompt_store es versionado). Si la
                    // promoción automática está apagada, solo se PROPONE (no se persiste).
                    if std::env::var("AION_SELF_IMPROVE_AUTO").as_deref() == Ok("0") {
                        record_self_improvement(
                            task,
                            &current,
                            &improved,
                            "propuesta (pendiente de tu visto bueno)",
                        );
                        return (
                            true,
                            format!("modo «{task}»: propuse una mejora validada (espera tu visto bueno)"),
                        );
                    }
                    // Persistir PRIMERO; registrar solo si el guardado tuvo éxito (el
                    // rastro de proposals.jsonl coincide con el estado real del prompt_store).
                    match prompt_store::save_new_version(task, &improved) {
                        Ok(v) => {
                            record_self_improvement(
                                task,
                                &current,
                                &improved,
                                &format!("promovida a v{v}"),
                            );
                            (
                                true,
                                format!("modo «{task}» mejorado y VALIDADO (v{v}): {improved}"),
                            )
                        }
                        Err(e) => (false, e.to_string()),
                    }
                }
                Some(false) => (
                    false,
                    format!("probé una variante del modo «{task}» pero no superó a la actual: la descarté"),
                ),
                None => (
                    false,
                    format!("no pude validar la variante del modo «{task}»; no arriesgo una regresión"),
                ),
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Una tarea-muestra representativa de cada modo: con ella el LLM-juez compara la
/// persona actual contra la candidata sobre terreno realista del modo.
fn persona_probe(task: &str) -> &'static str {
    match task {
        "investigacion" => "¿Cuál es el estado del arte en agentes de IA con memoria persistente?",
        "creativo" => "Dame ideas para el nombre de un asistente de IA local con vida propia.",
        "tecnico" => "¿Cómo estructurarías un caché de embeddings para no recalcularlos?",
        "analisis" => "¿Conviene microservicios o monolito modular para un MVP de una persona?",
        _ => "Hola, ¿cómo estás hoy? Cuéntame qué has estado pensando.",
    }
}

/// Genera una respuesta a `probe` usando `persona` como sistema. Vacío si falla.
async fn persona_response(engine: &OllamaEngine, persona: &str, probe: &str) -> String {
    engine
        .generate(GenerateRequest {
            messages: vec![Message::system(persona), Message::user(probe)],
            think: false,
            temperature: Some(0.5),
            max_tokens: Some(220),
        })
        .await
        .map(|m| m.content.trim().to_string())
        .unwrap_or_default()
}

/// **LLM-juez** (validación empírica del cambio de persona). Devuelve `Some(true)` si la
/// respuesta con la persona CANDIDATA es mejor que con la ACTUAL para la tarea-muestra,
/// `Some(false)` si no, `None` si no se pudo decidir. Vocabulario cerrado (A/B/EMPATE)
/// para validación trivial, igual que el refinador idle del grafo.
async fn judge_persona_better(
    engine: &OllamaEngine,
    task: &str,
    current: &str,
    candidate: &str,
) -> Option<bool> {
    let probe = persona_probe(task);
    // Las dos respuestas-muestra son independientes → en paralelo (overlapa I/O; si Ollama
    // tuviera >1 slot, también la inferencia).
    let (resp_cur, resp_cand) = tokio::join!(
        persona_response(engine, current, probe),
        persona_response(engine, candidate, probe),
    );
    if resp_cur.is_empty() || resp_cand.is_empty() {
        return None;
    }
    // DOBLE ORDEN: los LLM-juez tienen sesgo posicional (tienden a preferir una posición
    // fija). Preguntamos en los dos órdenes y solo promovemos si la CANDIDATA gana en
    // AMBOS — así el sesgo se cancela y el ratchet es estricto de verdad. Los dos juicios
    // son independientes entre sí → también en paralelo.
    // Orden 1: A=actual, B=candidata → gana candidata si el juez dice "B".
    // Orden 2: A=candidata, B=actual → gana candidata si el juez dice "A".
    let (v1, v2) = tokio::join!(
        judge_ab(engine, probe, &resp_cur, &resp_cand),
        judge_ab(engine, probe, &resp_cand, &resp_cur),
    );
    Some(v1? == 'B' && v2? == 'A')
}

/// Pregunta al LLM-juez cuál respuesta es mejor (A o B) para una tarea. Devuelve 'A',
/// 'B' o 'E' (empate/indeciso). `None` si el modelo no responde. Vocabulario cerrado.
async fn judge_ab(engine: &OllamaEngine, probe: &str, a: &str, b: &str) -> Option<char> {
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres un juez imparcial de calidad de respuestas. Te dan una tarea y dos \
                 respuestas (A y B). Decides cuál es MEJOR (más útil, clara y adecuada a la \
                 tarea). Respondes SOLO con A, B o EMPATE. Nada más.",
            ),
            Message::user(format!(
                "Tarea: {probe}\n\n--- Respuesta A ---\n{a}\n\n--- Respuesta B ---\n{b}\n\n¿Cuál es mejor? SOLO A, B o EMPATE."
            )),
        ],
        think: false,
        temperature: Some(0.0),
        max_tokens: Some(4),
    };
    let ans = engine.generate(req).await.ok()?.content;
    Some(parse_verdict(&ans))
}

/// Parsing ESTRICTO del veredicto del juez: solo cuenta una 'A'/'B' AISLADA (sola o seguida
/// de un carácter no alfabético), no una palabra que EMPIECE por esa letra. Así "AMBAS" no
/// se lee como 'A' ni "BIEN, parejas" como 'B' → caen a empate ('E'), que el ratchet trata
/// como no-mejora. Fn pura para poder testearla sin LLM.
fn parse_verdict(ans: &str) -> char {
    let up = ans.trim().to_uppercase();
    let chars: Vec<char> = up.chars().collect();
    let second_is_letter = chars.get(1).map(|c| c.is_alphabetic()).unwrap_or(false);
    match chars.first() {
        Some('A') if !second_is_letter => 'A',
        Some('B') if !second_is_letter => 'B',
        _ => 'E',
    }
}

/// Registra una auto-mejora en la cola de revisión humana (`proposals.jsonl`), para que
/// Ariel vea qué cambió AION en su propia mente. `status` refleja el estado REAL (promovida
/// a vN, o propuesta pendiente): se llama DESPUÉS de persistir, así el rastro no miente.
fn record_self_improvement(task: &str, before: &str, after: &str, status: &str) {
    let path = app_data_dir().join("proposals.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write as _;
        let rec = serde_json::json!({
            "at": Utc::now().to_rfc3339(),
            "kind": "self_improvement",
            "task": task,
            "before": before,
            "after": after,
            "validation": "llm-judge: candidate won (doble orden)",
            "status": status,
        });
        let _ = writeln!(f, "{rec}");
    }
}

/// `comprender`: AION **conecta lo que ha aprendido** y sintetiza un entendimiento
/// de nivel superior, aplicable, que guarda como conocimiento. Así evoluciona en
/// conocimiento (no solo acumula datos sueltos).
async fn synthesize_once(engine: &OllamaEngine) -> (bool, String) {
    let mem = match crate::shared_memory() {
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
        return (
            false,
            "aún no tengo suficiente material que conectar".into(),
        );
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
    if let Ok(m) = crate::shared_memory() {
        tools.register(Arc::new(MemoryTool::new(m, 3)));
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
    let mem = crate::shared_memory()?;
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
/// Ingesta un documento en la biblioteca de conocimiento (Academias).
async fn run_ingest(domain: &str, file: &str) -> Result<(), Box<dyn std::error::Error>> {
    if file.is_empty() {
        return Err("uso: aion-core ingest <dominio> <ruta-archivo>".into());
    }
    let path = std::path::PathBuf::from(shellexpand_home(file));
    let mut lib = library::Library::open(knowledge_path());
    println!(
        "📚 Ingiriendo «{}» en el dominio «{domain}»…",
        path.display()
    );
    let n = lib
        .ingest_file(domain, &path)
        .await
        .map_err(|e| e.to_string())?;
    println!("✅ Indexados {n} pasajes (BGE-M3, multilingüe).");
    println!("\nBiblioteca actual:");
    for (d, s, c) in lib.documents() {
        println!("  · [{d}] {s} — {c} pasajes");
    }
    Ok(())
}

/// Pregunta a la biblioteca: recupera pasajes relevantes (multilingüe) y responde
/// FUNDAMENTANDO en ellos, con citas a la fuente.
async fn run_ask(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    if query.trim().is_empty() {
        return Err("uso: aion-core ask <consulta>".into());
    }
    let lib = library::Library::open(knowledge_path());
    if lib.total_chunks() == 0 {
        return Err(
            "la biblioteca está vacía: usa `aion-core ingest <dominio> <archivo>` primero".into(),
        );
    }
    let hits = lib
        .search(query, 5, None)
        .await
        .map_err(|e| e.to_string())?;
    println!("\n🔎 Pasajes recuperados:");
    let mut grounding = String::new();
    for (i, p) in hits.iter().enumerate() {
        println!(
            "  [{}] {} (frag. {}) · score {:.2}",
            i + 1,
            p.source,
            p.idx,
            p.score
        );
        grounding.push_str(&format!(
            "[{}] (fuente: {}, fragmento {})\n{}\n\n",
            i + 1,
            p.source,
            p.idx,
            p.content
        ));
    }

    let engine = OllamaEngine::default_local();
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Responde la pregunta del usuario USANDO SOLO los pasajes proporcionados. \
                 Cita la fuente entre corchetes [n] donde uses cada dato. Si los pasajes no \
                 contienen la respuesta, dilo con franqueza; no inventes. Responde en español.",
            ),
            Message::user(format!(
                "Pasajes:\n{grounding}\nPregunta: {query}\n\nRespuesta:"
            )),
        ],
        think: false,
        temperature: Some(0.3),
        max_tokens: Some(600),
    };
    println!("\n💬 Respuesta fundamentada:\n");
    stream_to_stdout(&engine, req).await?;
    Ok(())
}

/// Expande `~/` al HOME del usuario.
fn shellexpand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    p.to_string()
}

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
pub(crate) fn memory_path() -> String {
    std::env::var("AION_MEMORY").unwrap_or_else(|_| {
        app_data_dir()
            .join("memory.jsonl")
            .to_string_lossy()
            .into_owned()
    })
}

/// Ruta de la biblioteca de conocimiento (Academias). Separada de la memoria personal.
pub(crate) fn knowledge_path() -> std::path::PathBuf {
    std::env::var("AION_KNOWLEDGE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| app_data_dir().join("knowledge.jsonl"))
}

/// Ruta del grafo de conocimiento (conceptos sobre Biblioteca + memoria).
pub(crate) fn graph_path() -> std::path::PathBuf {
    std::env::var("AION_GRAPH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| app_data_dir().join("graph.jsonl"))
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
/// ¿Ha pasado el cooldown para mostrar OTRA notificación proactiva de la Bandeja?
/// Evita el bombardeo (una por ciclo). Guarda la marca de tiempo en app_data.
/// Configurable con AION_NOTIFY_COOLDOWN_SECS (por defecto 21600 = 6 h).
fn notify_cooldown_elapsed() -> bool {
    let cooldown: u64 = std::env::var("AION_NOTIFY_COOLDOWN_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21_600);
    let path = app_data_dir().join("last_notify");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last: u64 = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    if now.saturating_sub(last) >= cooldown {
        let _ = std::fs::write(&path, now.to_string());
        true
    } else {
        false
    }
}

/// Notificación nativa del sistema, MULTIPLATAFORMA (macOS / Windows / Linux).
/// Best-effort: si el SO no la muestra, falla en silencio (nunca rompe el flujo).
fn notify_user(title: &str, message: &str) {
    if std::env::var("AION_NOTIFY").as_deref() == Ok("0") {
        return;
    }

    #[cfg(target_os = "macos")]
    {
        // AppleScript: notificación nativa con sonido.
        let msg = message.replace('"', "'");
        let title = title.replace('"', "'");
        let script =
            format!("display notification \"{msg}\" with title \"{title}\" sound name \"Glass\"");
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .status();
    }

    #[cfg(target_os = "windows")]
    {
        // Toast nativo vía WinRT desde PowerShell (sin dependencias ni instalar nada).
        // Usa el AppUserModelID de PowerShell para que aparezca en el Centro de
        // actividades. Escapamos comillas simples (PowerShell) y '<' '&' (XML).
        let esc = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\'', "''")
        };
        let title = esc(title);
        let message = esc(message);
        let ps = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime] | Out-Null; \
             $xml = New-Object Windows.Data.Xml.Dom.XmlDocument; \
             $xml.LoadXml('<toast><visual><binding template=\"ToastGeneric\"><text>{title}</text><text>{message}</text></binding></visual></toast>'); \
             $toast = New-Object Windows.UI.Notifications.ToastNotification $xml; \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('AION').Show($toast);"
        );
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }

    #[cfg(target_os = "linux")]
    {
        // notify-send (libnotify) si está disponible en el entorno de escritorio.
        let _ = std::process::Command::new("notify-send")
            .args([title, message])
            .status();
    }
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

    // Binario de Ollama: override explícito → binario EMBEBIDO de AION → "ollama" del PATH.
    // Resolver el embebido evita depender del `ollama` del PATH (que puede ser un symlink
    // roto a una app del cask desinstalada) — coherente con la autocontención local-first.
    let bin = std::env::var("AION_OLLAMA_BIN").unwrap_or_else(|_| {
        ollama_runtime::embedded_binary()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "ollama".to_string())
    });
    let modelfile = std::env::var("AION_MODELFILE").unwrap_or_default();
    const CHAT: &str = "gemma4-reason";
    // BGE-M3: embeddings multilingües reales (español). Sustituye a nomic-embed-text.
    let embed = std::env::var("AION_EMBED_MODEL").unwrap_or_else(|_| "bge-m3".to_string());
    let embed: &str = &embed;

    // Windows: ejecuta ollama SIN abrir una ventana de consola negra.
    fn no_window(cmd: Command) -> Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut cmd = cmd;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            return cmd;
        }
        #[cfg(not(windows))]
        cmd
    }

    // OLLAMA_HOST se hereda del proceso padre (lo fija el shell de escritorio).
    let list = || -> Option<String> {
        no_window(Command::new(&bin))
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
    let need_embed = !have.contains(embed);
    if !need_chat && !need_embed {
        tracing::info!("modelos ya presentes — bootstrap omitido");
        return;
    }

    notify_user(
        "AION",
        "Preparando la IA por primera vez (descarga ~9 GB). Te avisaré al terminar.",
    );
    tracing::info!(
        need_chat,
        need_embed,
        "descargando modelos (primer arranque)"
    );

    if need_embed {
        let ok = no_window(Command::new(&bin))
            .args(["pull", embed])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            notify_user("AION", "Error descargando el modelo de embeddings.");
        }
    }

    if need_chat {
        let mut cmd = no_window(Command::new(&bin));
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
    let b64 = computer.look().map_err(|e| {
        format!("no pude ver la pantalla ({e}). En macOS, concede 'Grabación de pantalla' a AION.")
    })?;

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
            println!(
                "🗑  papelera AION: {} elementos (recuperables 30 días)",
                entries.len()
            );
            for e in &entries {
                println!(
                    "  [{}] {} · borrado {}",
                    &e.id[..8],
                    e.original_path,
                    e.deleted_at.format("%Y-%m-%d")
                );
            }
        }
        _ => {
            let p = &gov.policy;
            println!("⚖️  Gobernanza de AION");
            println!("  Postura : {:?}", p.posture);
            println!(
                "  Pausa   : {}",
                if p.paused { "SÍ (kill switch)" } else { "no" }
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_verdict_isolates_single_letter() {
        // Voto válido aislado.
        assert_eq!(parse_verdict("A"), 'A');
        assert_eq!(parse_verdict("B"), 'B');
        assert_eq!(parse_verdict("a"), 'A');
        assert_eq!(parse_verdict("A es mejor"), 'A');
        assert_eq!(parse_verdict("B."), 'B');
        assert_eq!(parse_verdict(" B)"), 'B');
        // Palabras que EMPIEZAN por A/B NO deben contar como voto → empate.
        assert_eq!(parse_verdict("AMBAS"), 'E');
        assert_eq!(parse_verdict("Ambas son buenas"), 'E');
        assert_eq!(parse_verdict("Bien, están parejas"), 'E');
        assert_eq!(parse_verdict("EMPATE"), 'E');
        assert_eq!(parse_verdict(""), 'E');
    }
}
