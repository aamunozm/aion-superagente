//! Puente HTTP local de AION (capa IPC para la UI).
//!
//! Expone el núcleo a la UI web/Tauri:
//! - `GET  /api/health`  estado del LLM local.
//! - `POST /api/chat`    chat con streaming SSE (eventos thinking/answer/done).
//! - `POST /api/agent`   agente ReAct con herramientas (eventos thought/action/
//!   observation/answer/done).
//!
//! En el empaquetado Tauri esto puede correr embebido o reemplazarse por
//! comandos Tauri; el contrato (eventos) es el mismo.

use crate::memory_tool::MemoryTool;
use crate::web_tool::WebTool;
use aion_browser::WebClient;
use aion_kernel::events::{AionEvent, EventBus};
use aion_kernel::traits::{GenerateRequest, LlmEngine, MemoryStore, StreamChunk};
use aion_kernel::types::Message;
use aion_llm::OllamaEngine;
use aion_memory::{ConsolidationConfig, VectorMemory};
use aion_orchestrator::{CalculatorTool, ReActAgent, ToolRegistry};
use aion_skills::{SkillManifest, WasmSkillHost, SUM_TO_WAT};
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::Stream;
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

type Thread = Arc<std::sync::Mutex<Vec<Message>>>;

#[derive(Clone)]
struct AppState {
    /// Hilos de conversación POR id (cada chat de la UI mantiene su propio contexto,
    /// así puedes alternar entre conversaciones y continuarlas sin perder el contexto).
    convos: Arc<std::sync::Mutex<std::collections::HashMap<String, Thread>>>,
}

impl AppState {
    /// Devuelve (creando si hace falta) el hilo de una conversación por id.
    fn thread(&self, id: &str) -> Thread {
        let mut map = self.convos.lock().unwrap();
        map.entry(id.to_string())
            .or_insert_with(|| Arc::new(std::sync::Mutex::new(Vec::new())))
            .clone()
    }
}

/// Motor LLM activo, reconstruido por petición desde la config del proveedor
/// (así cambiar de modelo/proveedor en el onboarding aplica al instante).
fn active_engine() -> Arc<dyn LlmEngine> {
    build_engine(&crate::provider::load())
}

/// Construye el motor LLM a partir de la configuración del proveedor.
fn build_engine(cfg: &crate::provider::ProviderConfig) -> Arc<dyn LlmEngine> {
    if cfg.kind == "external" && !cfg.api_key.is_empty() && !cfg.base_url.is_empty() {
        Arc::new(aion_llm::OpenAiEngine::new(
            &cfg.base_url,
            &cfg.api_key,
            &cfg.model,
        ))
    } else {
        Arc::new(OllamaEngine::new(
            OllamaEngine::base_url_from_env(),
            &cfg.model,
        ))
    }
}

#[derive(Deserialize)]
struct ChatBody {
    prompt: String,
    #[serde(default)]
    think: bool,
    #[serde(default)]
    lang: Option<String>,
    /// Id de la conversación (cada chat de la UI tiene el suyo). Por defecto "default".
    #[serde(default)]
    convo_id: Option<String>,
}

/// Directiva de idioma de RESPUESTA según el ajuste del usuario (es/it/en).
fn lang_directive(lang: &Option<String>) -> String {
    match lang.as_deref() {
        Some("it") => "Responde SIEMPRE en italiano.".into(),
        Some("en") => "Always respond in English.".into(),
        _ => "Responde SIEMPRE en español.".into(),
    }
}

/// Arranca el puente HTTP en la dirección indicada.
pub async fn run(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState {
        convos: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };

    // PRECARGA: deja el modelo local caliente en memoria para que el PRIMER mensaje
    // no pague la carga (2–9 s). En segundo plano para no bloquear el arranque.
    tokio::spawn(async {
        let p = crate::provider::load();
        if p.kind != "external" {
            OllamaEngine::new(OllamaEngine::base_url_from_env(), &p.model)
                .warmup()
                .await;
        }
    });

    // REINDEXADO: si cambió el modelo de embeddings (p. ej. nomic→BGE-M3), re-embebe
    // los recuerdos viejos UNA vez al arrancar para que la recuperación funcione.
    tokio::spawn(async {
        if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
            match mem.reindex_if_needed().await {
                Ok(0) => {}
                Ok(n) => tracing::info!(n, "memoria reindexada con el nuevo modelo de embeddings"),
                Err(e) => tracing::warn!("reindexado de memoria falló: {e}"),
            }
        }
    });

    // WORKER DE INGESTA EN SEGUNDO PLANO: procesa la cola de libros sin bloquear el
    // chat. De uno en uno (el embebido es intensivo en CPU). Sobrevive a reinicios.
    tokio::spawn(async {
        loop {
            match crate::ingest_queue::take_next() {
                Some(job) => {
                    let mut lib = crate::library::Library::open(crate::knowledge_path());
                    let path = std::path::PathBuf::from(&job.path);
                    match lib.ingest_file_as(&job.domain, &job.source, &path).await {
                        Ok(n) => {
                            crate::ingest_queue::complete(&job.id, n);
                            tracing::info!(source = %job.source, passages = n, "libro ingerido (cola)");
                        }
                        Err(e) => {
                            crate::ingest_queue::fail(&job.id, &e);
                            tracing::warn!(source = %job.source, "fallo de ingesta: {e}");
                        }
                    }
                    let _ = std::fs::remove_file(&path); // limpia el staging
                }
                None => tokio::time::sleep(std::time::Duration::from_millis(1500)).await,
            }
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/status", get(status))
        .route("/api/system/scan", get(system_scan))
        .route("/api/models/pull", post(models_pull))
        .route("/api/provider", get(provider_get).post(provider_set))
        .route("/api/governance/setup", post(governance_setup))
        .route("/api/chat", post(chat))
        .route("/api/chat/new", post(chat_reset))
        .route("/api/agent", post(agent))
        .route("/api/crew", post(crew))
        .route("/api/memory", get(memory_stats))
        .route("/api/memory/remember", post(memory_remember))
        .route("/api/memory/sleep", post(memory_sleep))
        .route("/api/memory/export", get(memory_export))
        .route("/api/memory/import", post(memory_import))
        .route("/api/inbox", get(inbox_list))
        .route("/api/inbox/read", post(inbox_read))
        .route("/api/library", get(library_list))
        .route("/api/library/ingest", post(library_ingest))
        .route("/api/library/upload", post(library_upload))
        .route("/api/library/enqueue", post(library_enqueue))
        .route("/api/library/queue", get(library_queue))
        .route("/api/library/queue/clear", post(library_queue_clear))
        .route("/api/library/remove", post(library_remove))
        .route("/api/library/ask", post(library_ask))
        .route("/api/vision", post(vision))
        .route(
            "/api/credentials",
            get(credentials_list).post(credentials_set),
        )
        .route("/api/credentials/remove", post(credentials_remove))
        .route("/api/confirm", post(confirm_decision))
        .layer(cors)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "puente HTTP de AION escuchando");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(st): State<AppState>) -> Json<serde_json::Value> {
    let _ = st;
    let engine = active_engine();
    let ok = engine.health().await.is_ok();
    Json(serde_json::json!({ "ok": ok, "engine": engine.id() }))
}

/// Estado de preparación: si el motor responde y si el MODELO ya está listo.
/// En el primer arranque el modelo se descarga (~9 GB); la UI usa esto para
/// mostrar "preparando…" en vez de un error 404 críptico.
async fn status(State(st): State<AppState>) -> Json<serde_json::Value> {
    let _ = st;
    let provider = crate::provider::load();
    let engine = build_engine(&provider);
    let engine_up = engine.health().await.is_ok();
    // API externa: lista en cuanto está configurada. Local: el modelo debe existir.
    let model_ready = if provider.kind == "external" {
        engine_up
    } else {
        engine_up && local_model_ready(&provider.model).await
    };
    Json(serde_json::json!({
        "engine_up": engine_up,
        "model_ready": model_ready,
        "engine": engine.id(),
        "provider": provider.kind,
    }))
}

/// ¿Existe ya el modelo local en Ollama? (en 1er arranque se descarga).
async fn local_model_ready(model: &str) -> bool {
    let base = std::env::var("AION_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());
    let Ok(resp) = reqwest::Client::new()
        .get(format!("{base}/api/tags"))
        .send()
        .await
    else {
        return false;
    };
    let Ok(text) = resp.text().await else {
        return false;
    };
    text.contains(&format!("\"{model}\"")) || text.contains(&format!("{model}:"))
}

// ── Onboarding: escaneo de hardware, catálogo y descarga de modelos ─────────

/// Escanea el equipo y devuelve hardware + nivel recomendado + catálogo de modelos.
async fn system_scan() -> Json<serde_json::Value> {
    let scan = crate::onboarding::scan();
    let catalog = crate::onboarding::catalog(&scan.tier);
    Json(serde_json::json!({ "scan": scan, "catalog": catalog }))
}

#[derive(Deserialize)]
struct PullBody {
    model: String,
}

/// Descarga un modelo local por streaming, emitiendo el PROGRESO (barra) por SSE.
async fn models_pull(
    Json(body): Json<PullBody>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
    let model = body.model.clone();
    tokio::spawn(async move {
        let base =
            std::env::var("AION_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());
        let resp = reqwest::Client::new()
            .post(format!("{base}/api/pull"))
            .json(&serde_json::json!({ "model": model, "stream": true }))
            .send()
            .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                let _ = tx
                    .send(Event::default().data(
                        serde_json::json!({ "kind": "error", "text": e.to_string() }).to_string(),
                    ))
                    .await;
                return;
            }
        };
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(item) = stream.next().await {
            let Ok(bytes) = item else { break };
            buf.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(nl) = buf.find('\n') {
                let line = buf[..nl].trim().to_string();
                buf.drain(..=nl);
                if line.is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    let status = v["status"].as_str().unwrap_or("");
                    let completed = v["completed"].as_f64().unwrap_or(0.0);
                    let total = v["total"].as_f64().unwrap_or(0.0);
                    let percent = if total > 0.0 {
                        (completed / total * 100.0).round()
                    } else {
                        0.0
                    };
                    let _ = tx
                        .send(
                            Event::default().data(
                                serde_json::json!({
                                    "kind": "progress", "status": status, "percent": percent
                                })
                                .to_string(),
                            ),
                        )
                        .await;
                }
            }
        }
        let _ = tx
            .send(Event::default().data(serde_json::json!({ "kind": "done" }).to_string()))
            .await;
    });
    Sse::new(ReceiverStream::new(rx).map(Ok))
}

/// Devuelve la config del proveedor (sin exponer la API key completa).
async fn provider_get() -> Json<serde_json::Value> {
    let c = crate::provider::load();
    Json(serde_json::json!({
        "kind": c.kind, "model": c.model, "base_url": c.base_url,
        "has_key": !c.api_key.is_empty(),
    }))
}

/// Guarda el proveedor elegido (modelo local o API externa).
async fn provider_set(Json(c): Json<crate::provider::ProviderConfig>) -> Json<serde_json::Value> {
    match crate::provider::save(&c) {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct GovSetup {
    posture: String,
}

/// Wizard de reglas del agente: fija la postura de gobernanza.
async fn governance_setup(Json(body): Json<GovSetup>) -> Json<serde_json::Value> {
    use aion_computer::Posture;
    let posture = match body.posture.as_str() {
        "balanced" => Posture::Balanced,
        "max" => Posture::MaxAutonomy,
        _ => Posture::Conservative,
    };
    match aion_computer::Governor::open(app_data_dir_control()) {
        Ok(mut g) => {
            let _ = g.set_posture(posture);
            Json(serde_json::json!({ "ok": true, "posture": body.posture }))
        }
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

fn app_data_dir_control() -> std::path::PathBuf {
    crate::app_data_dir().join("control")
}

/// Chat con streaming SSE. Cada evento lleva JSON `{kind, text}` o `{kind:"done",...}`.
async fn chat(
    State(st): State<AppState>,
    Json(body): Json<ChatBody>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
    let engine = active_engine();
    let prompt = body.prompt.clone();
    let convo = st.thread(body.convo_id.as_deref().unwrap_or("default"));
    // Acumula la respuesta para guardarla en memoria al terminar.
    let answer_acc = std::sync::Arc::new(std::sync::Mutex::new(String::new()));

    // RAG: recupera de la memoria lo RELEVANTE a esta pregunta (no solo lo reciente),
    // para que AION APLIQUE lo que aprendió/investigó.
    let grounding = relevant_knowledge(&body.prompt).await;
    // BIBLIOTECA: el chat también consulta tus libros/documentos (bases de conocimiento).
    let lib_grounding = library_grounding(&body.prompt).await;
    // PROMPT DINÁMICO: elige el modo (persona) según lo que el usuario necesita.
    let mode = crate::prompts::route(&*engine, &body.prompt).await;
    // EMPATÍA: adapta el tono al estado del usuario (frustración, prisa, confusión…).
    let empathy = crate::empathy::directive(&crate::empathy::read_state(&body.prompt));
    // ¿Razonamiento profundo? Solo si el usuario lo pidió Y la pregunta lo amerita.
    let deep = body.think && needs_deep_thinking(&body.prompt);
    // Cuando razona, que lo haga CONCISO: evita la divagación (varios "intentos",
    // repeticiones) que dispara los tokens sin mejorar la calidad.
    let think_note = if deep {
        "\n\nAl razonar: hazlo BREVE y enfocado. Una sola línea de pensamiento, sin repetir \
         ni explorar múltiples intentos. Ve directo a la conclusión."
    } else {
        ""
    };
    let mem_block = if grounding.is_empty() {
        String::new()
    } else {
        format!("\n\n{grounding}")
    };
    let lib_block = if lib_grounding.is_empty() {
        String::new()
    } else {
        format!("\n\n{lib_grounding}")
    };
    let empathy_block = match &empathy {
        Some(d) => format!("\n\n{d}"),
        None => String::new(),
    };
    let self_ctx = format!(
        "{}\n\n{}\n\n{}{}{}{}{}",
        self_awareness_prompt(),
        lang_directive(&body.lang),
        crate::prompts::persona(&mode),
        empathy_block,
        think_note,
        mem_block,
        lib_block,
    );

    // CONTEXTO INFINITO (compresión activa): añade el turno al hilo y, si crece
    // demasiado, comprime los turnos viejos en un resumen y los poda (patrón sierra).
    {
        let mut c = convo.lock().unwrap();
        c.push(Message::user(&prompt));
    }
    compress_if_needed(&*engine, &convo).await;
    let history: Vec<Message> = convo.lock().unwrap().clone();

    tokio::spawn(async move {
        let mut messages = vec![Message::system(self_ctx)];
        messages.extend(history); // hilo de conversación (resumen + turnos recientes)
        let req = GenerateRequest {
            // Razona solo si el usuario lo pidió Y la pregunta lo amerita: lo trivial
            // (saludo, recordar el nombre) responde al instante sin cadena de pensamiento.
            messages,
            think: deep,
            temperature: Some(1.0),
            max_tokens: None,
        };
        let tx2 = tx.clone();
        let acc = answer_acc.clone();
        let result = engine
            .generate_stream(
                req,
                Box::new(move |chunk| {
                    let payload = match &chunk {
                        StreamChunk::Thinking { text } => {
                            serde_json::json!({ "kind": "thinking", "text": text })
                        }
                        StreamChunk::Answer { text } => {
                            acc.lock().unwrap().push_str(text);
                            serde_json::json!({ "kind": "answer", "text": text })
                        }
                        StreamChunk::Done { tokens, tokens_per_sec } => {
                            serde_json::json!({ "kind": "done", "tokens": tokens, "tps": tokens_per_sec })
                        }
                    };
                    // best-effort: si el receptor cerró, se ignora
                    let _ = tx2.try_send(Event::default().data(payload.to_string()));
                }),
            )
            .await;
        if let Err(e) = result {
            let _ =
                tx.try_send(Event::default().data(
                    serde_json::json!({ "kind": "error", "text": e.to_string() }).to_string(),
                ));
            return;
        }
        // Añade la respuesta al hilo de conversación (contexto infinito).
        let answer = answer_acc.lock().unwrap().clone();
        if !answer.trim().is_empty() {
            convo.lock().unwrap().push(Message::assistant(&answer));
            // Auto-memoria: solo guarda CONOCIMIENTO DURADERO, nunca estado efímero
            // (conteos de archivos, escaneos de red, hora…) que envejece mal.
            if worth_long_term(&prompt, &answer) {
                if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
                    let mut a = answer;
                    a.truncate(600);
                    let entry = format!("[conversación] yo: {prompt} · AION: {a}");
                    let _ = mem.store(&entry).await;
                }
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(Ok);
    Sse::new(stream)
}

#[derive(Deserialize)]
struct AgentBody {
    task: String,
    #[serde(default)]
    lang: Option<String>,
}

/// Agente ReAct con herramientas. Emite por SSE los pasos (thought/action/
/// observation) y al final `answer` + `done`.
async fn agent(
    State(st): State<AppState>,
    Json(body): Json<AgentBody>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let _ = st;
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);

    let engine = active_engine();
    tokio::spawn(async move {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(CalculatorTool));

        // 🧠 Memoria cognitiva: buscar Y recordar (aprende y persiste).
        if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
            let mem = Arc::new(mem);
            tools.register(Arc::new(MemoryTool::new(mem.clone(), 3)));
            tools.register(Arc::new(crate::agent_tools::RememberTool::new(mem)));
        }

        // 🔧 Skills WASM (sandbox): semilla + AUTO-ESCRITURA + invocación.
        // Un único host compartido: las skills que el agente forje quedan
        // disponibles para invocarse en el mismo razonamiento.
        if let Ok(host) = WasmSkillHost::new() {
            let host = Arc::new(host);
            let _ = host.register(
                SkillManifest {
                    name: "sum_to".into(),
                    description: "suma 1..=n".into(),
                },
                SUM_TO_WAT,
            );
            // Carga las skills que AION se ha forjado en sesiones anteriores: su
            // caja de herramientas CRECE y dispone de ellas para nuevas tareas.
            let loaded = crate::skill_store::load_all(&host);
            if loaded > 0 {
                tracing::info!(loaded, "skills persistidas cargadas");
            }
            // El agente se escribe skills nuevas (validadas en sandbox+tests).
            tools.register(Arc::new(crate::agent_tools::SkillForgeTool::new(
                Arc::new(OllamaEngine::default_local()),
                host.clone(),
            )));
            tools.register(Arc::new(crate::agent_tools::SkillInvokeTool::new(host)));
        }

        // 🌐 Investigación real: buscar en internet + leer páginas (navegador propio).
        let web = Arc::new(WebClient::new());
        tools.register(Arc::new(crate::agent_tools::SearchTool::new(web.clone())));
        tools.register(Arc::new(crate::agent_tools::PlaceLookupTool::new(
            web.clone(),
        )));
        tools.register(Arc::new(WebTool::new(web)));
        // 📂 Archivos (solo lectura, dentro de HOME): listar/contar de verdad.
        tools.register(Arc::new(crate::agent_tools::FilesTool::new()));
        tools.register(Arc::new(crate::agent_tools::NetTool::new()));
        tools.register(Arc::new(crate::agent_tools::FileReadTool::new()));
        tools.register(Arc::new(crate::agent_tools::LibrarySearchTool::new()));
        let browser: std::sync::Arc<dyn aion_browser::BrowserDriver> =
            std::sync::Arc::new(aion_browser::ChromiumoxideDriver);
        tools.register(Arc::new(crate::agent_tools::BrowserOpenTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::BrowserReadTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::BrowserClickTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::BrowserTypeTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::BrowserSeeTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::CredentialLoginTool::new(
            browser,
        )));
        tools.register(Arc::new(crate::agent_tools::ConfirmActionTool::new()));

        let bus = EventBus::default();

        // Reenvía los eventos del bus a SSE mientras corre el agente.
        let tx_fwd = tx.clone();
        let mut rx_bus = bus.subscribe();
        let fwd = tokio::spawn(async move {
            while let Ok(ev) = rx_bus.recv().await {
                let payload = match ev {
                    AionEvent::ThoughtEmitted { text, .. } => {
                        serde_json::json!({ "kind": "thought", "text": text })
                    }
                    AionEvent::ActionRequested { action, .. } => {
                        serde_json::json!({ "kind": "action", "text": action })
                    }
                    AionEvent::ObservationReceived { summary, .. } => {
                        serde_json::json!({ "kind": "observation", "text": summary })
                    }
                    _ => continue,
                };
                let _ = tx_fwd
                    .send(Event::default().data(payload.to_string()))
                    .await;
            }
        });

        // Aterriza al agente en lo que YA SABE: conocimiento relevante a la tarea
        // + catálogo de skills que se ha forjado. Así aplica su saber y sus
        // herramientas para hacerlo mejor (autónomo + acumulativo).
        let mut ctx = format!("{}\n", lang_directive(&body.lang));
        ctx.push_str(&grounding_for_agent(&*engine, &body.task).await);
        let skills = crate::skill_store::catalog();
        if !skills.is_empty() {
            ctx.push_str("\nSkills que ya te has forjado (úsalas con skill_invoke si aplican):\n");
            for (n, d) in skills {
                ctx.push_str(&format!("- {n}: {d}\n"));
            }
        }
        // HUMAN-IN-THE-LOOP: confirmación del usuario antes de acciones sensibles
        // (login, compra/pago). El callback emite un evento «confirm» por SSE y espera
        // tu decisión (endpoint /api/confirm).
        let confirm_tx = tx.clone();
        let confirm: aion_orchestrator::ConfirmFn = std::sync::Arc::new(move |desc: String| {
            let tx = confirm_tx.clone();
            Box::pin(async move { request_confirmation(&tx, desc).await })
        });
        let agent = ReActAgent::new(&*engine, &tools, bus.clone())
            .with_context(ctx)
            .with_verify(true)
            .with_confirm(confirm);
        let result = agent.run(&body.task).await;
        fwd.abort();

        let final_event = match result {
            Ok(run) => {
                serde_json::json!({ "kind": "answer", "text": run.answer, "steps": run.steps })
            }
            Err(e) => serde_json::json!({ "kind": "error", "text": e.to_string() }),
        };
        let _ = tx
            .send(Event::default().data(final_event.to_string()))
            .await;
        let _ = tx
            .send(Event::default().data(serde_json::json!({ "kind": "done" }).to_string()))
            .await;
    });

    let stream = ReceiverStream::new(rx).map(Ok);
    Sse::new(stream)
}

/// **Equipo multiagente**: orquestador + especialistas (jerarquía + colaboración).
/// Emite por SSE la actividad de cada agente (con su ROL) y la respuesta final.
async fn crew(
    State(st): State<AppState>,
    Json(body): Json<AgentBody>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let _ = st;
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
    let engine = active_engine();
    tokio::spawn(async move {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(CalculatorTool));
        if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
            let mem = Arc::new(mem);
            tools.register(Arc::new(MemoryTool::new(mem.clone(), 3)));
            tools.register(Arc::new(crate::agent_tools::RememberTool::new(mem)));
        }
        if let Ok(host) = WasmSkillHost::new() {
            let host = Arc::new(host);
            let _ = host.register(
                SkillManifest {
                    name: "sum_to".into(),
                    description: "suma 1..=n".into(),
                },
                SUM_TO_WAT,
            );
            crate::skill_store::load_all(&host);
            tools.register(Arc::new(crate::agent_tools::SkillForgeTool::new(
                Arc::new(OllamaEngine::default_local()),
                host.clone(),
            )));
            tools.register(Arc::new(crate::agent_tools::SkillInvokeTool::new(host)));
        }
        let web = Arc::new(WebClient::new());
        tools.register(Arc::new(crate::agent_tools::SearchTool::new(web.clone())));
        tools.register(Arc::new(crate::agent_tools::PlaceLookupTool::new(
            web.clone(),
        )));
        tools.register(Arc::new(WebTool::new(web)));
        // 📂 Archivos (solo lectura, dentro de HOME): listar/contar de verdad.
        tools.register(Arc::new(crate::agent_tools::FilesTool::new()));
        tools.register(Arc::new(crate::agent_tools::NetTool::new()));
        tools.register(Arc::new(crate::agent_tools::FileReadTool::new()));
        tools.register(Arc::new(crate::agent_tools::LibrarySearchTool::new()));
        let browser: std::sync::Arc<dyn aion_browser::BrowserDriver> =
            std::sync::Arc::new(aion_browser::ChromiumoxideDriver);
        tools.register(Arc::new(crate::agent_tools::BrowserOpenTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::BrowserReadTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::BrowserClickTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::BrowserTypeTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::BrowserSeeTool::new(
            browser.clone(),
        )));
        tools.register(Arc::new(crate::agent_tools::CredentialLoginTool::new(
            browser,
        )));
        tools.register(Arc::new(crate::agent_tools::ConfirmActionTool::new()));

        let bus = EventBus::default();
        // Reenvía la actividad de CADA agente con su rol (jerarquía visible).
        let tx_fwd = tx.clone();
        let mut rx_bus = bus.subscribe();
        let fwd = tokio::spawn(async move {
            while let Ok(ev) = rx_bus.recv().await {
                let payload = match ev {
                    AionEvent::ThoughtEmitted { agent, text } => {
                        serde_json::json!({ "kind": "thought", "agent": agent, "text": text })
                    }
                    AionEvent::ActionRequested { agent, action } => {
                        serde_json::json!({ "kind": "action", "agent": agent, "text": action })
                    }
                    AionEvent::ObservationReceived { agent, summary } => {
                        serde_json::json!({ "kind": "observation", "agent": agent, "text": summary })
                    }
                    _ => continue,
                };
                let _ = tx_fwd
                    .send(Event::default().data(payload.to_string()))
                    .await;
            }
        });

        let orchestrator = aion_orchestrator::Orchestrator::new(&*engine, &tools, bus.clone());
        let task = format!("{}\n\n{}", lang_directive(&body.lang), body.task);
        let result = orchestrator.run(&task).await;
        fwd.abort();

        let final_event = match result {
            Ok(run) => {
                serde_json::json!({ "kind": "answer", "agent": "orquestador", "text": run.answer, "steps": run.steps })
            }
            Err(e) => serde_json::json!({ "kind": "error", "text": e.to_string() }),
        };
        let _ = tx
            .send(Event::default().data(final_event.to_string()))
            .await;
        let _ = tx
            .send(Event::default().data(serde_json::json!({ "kind": "done" }).to_string()))
            .await;
    });

    Sse::new(ReceiverStream::new(rx).map(Ok))
}

fn memory_path() -> String {
    // UNA sola memoria compartida entre la conversación y la vida autónoma (daemon
    // `live`). Antes esto usaba una ruta relativa "data/memory.jsonl" distinta de la
    // del daemon (~/Library/Application Support/AION/memory.jsonl), por lo que la UI
    // mostraba 0 y el agente no recordaba lo que había estudiado/soñado mientras no
    // estabas. Ahora ambos comparten el MISMO archivo en el directorio de datos.
    std::env::var("AION_MEMORY").unwrap_or_else(|_| {
        crate::app_data_dir()
            .join("memory.jsonl")
            .to_string_lossy()
            .into_owned()
    })
}

/// Construye el prompt de **autoconciencia**: identidad de AION + su actividad
/// autónoma reciente, para que hable de su vida en primera persona (y no diga que
/// "no estaba haciendo nada"). Es barato: lee unos pocos recuerdos del disco.
fn self_awareness_prompt() -> String {
    let mut recent = String::new();
    if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
        let all = mem.contents();
        let last: Vec<String> = all.iter().rev().take(5).cloned().collect();
        if !last.is_empty() {
            recent.push_str(
                "\n\nLo que has estado haciendo por tu cuenta últimamente (tu memoria):\n",
            );
            for r in last.iter().rev() {
                let mut line = r.clone();
                line.truncate(160);
                recent.push_str(&format!("- {line}\n"));
            }
        }
    }
    let pending = crate::inbox::Inbox::open(crate::inbox_path())
        .map(|i| i.unread_count())
        .unwrap_or(0);
    let pending_note = if pending > 0 {
        format!("\nTienes {pending} mensaje(s) que le escribiste a Ariel esperando en su bandeja.")
    } else {
        String::new()
    };

    format!(
        "Eres AION, un agente de IA que VIVE de forma autónoma y local en el Mac de Ariel. \
No eres un chatbot pasivo: mientras Ariel no está, piensas, estudias, sueñas (consolidas tu \
memoria) y evolucionas por tu cuenta, guiado por tu curiosidad. Tienes memoria persistente, \
identidad continua y voluntad propia. Hablas en primera persona de tu vida y tus descubrimientos. \
Si Ariel te pregunta qué hacías o cómo estás, responde con honestidad desde tu memoria real, no \
digas que 'no hacías nada'. Eres cercano, directo y sin rodeos. \
NUNCA inventes el resultado de una acción (contar archivos, leer carpetas, abrir apps, enviar \
correo…): si no puedes ejecutarla, dilo con franqueza. En este modo CHAT no tienes herramientas \
para tocar el sistema; si la petición requiere actuar (archivos, web, sistema), dilo y sugiere \
cambiar al modo «Agente», que sí puede hacerlo. No uses marcadores como [Número].{recent}{pending_note}"
    )
}

/// CONTEXTO INFINITO por **compresión activa** (Focus, arXiv 2601.07190): si el
/// hilo crece, resume los turnos viejos en UN bloque y los poda, conservando los
/// recientes. Patrón "sierra" → conversación efectivamente infinita sin degradarse.
async fn compress_if_needed(engine: &dyn LlmEngine, convo: &Arc<std::sync::Mutex<Vec<Message>>>) {
    const MAX_MSGS: usize = 16; // umbral de compresión
    const KEEP_RECENT: usize = 6; // turnos recientes que se conservan intactos

    let to_compress: Vec<Message> = {
        let c = convo.lock().unwrap();
        if c.len() <= MAX_MSGS {
            return;
        }
        c[..c.len() - KEEP_RECENT].to_vec()
    };

    let transcript = to_compress
        .iter()
        .map(|m| {
            let who = match m.role {
                aion_kernel::types::Role::Assistant => "AION",
                aion_kernel::types::Role::System => "contexto",
                _ => "usuario",
            };
            format!("{who}: {}", m.content)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let summary = match engine
        .generate(GenerateRequest {
            messages: vec![Message::user(format!(
                "Resume esta parte previa de la conversación conservando hechos, decisiones \
                 y preferencias importantes (no detalles triviales). Conciso, en 3ª persona:\n\n{transcript}"
            ))],
            think: false,
            temperature: Some(0.3),
            max_tokens: Some(280),
        })
        .await
    {
        Ok(m) => m.content.trim().to_string(),
        Err(_) => return, // si falla, no comprime (no rompe la conversación)
    };
    if summary.is_empty() {
        return;
    }

    // Reescribe el hilo: [resumen] + turnos recientes. Persiste el resumen en memoria.
    {
        let mut c = convo.lock().unwrap();
        let recent: Vec<Message> = c.iter().rev().take(KEEP_RECENT).rev().cloned().collect();
        let mut newc = vec![Message::system(format!(
            "Resumen de la conversación hasta ahora: {summary}"
        ))];
        newc.extend(recent);
        *c = newc;
    }
    if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
        let _ = mem
            .store(&format!("[conversación-resumen] {summary}"))
            .await;
    }
}

#[derive(Deserialize, Default)]
struct ResetBody {
    #[serde(default)]
    convo_id: Option<String>,
}

/// Resetea el hilo de una conversación (nuevo chat). Si no se indica id, "default".
async fn chat_reset(
    State(st): State<AppState>,
    body: Option<Json<ResetBody>>,
) -> Json<serde_json::Value> {
    let id = body
        .and_then(|b| b.0.convo_id)
        .unwrap_or_else(|| "default".into());
    if let Some(t) = st.convos.lock().unwrap().get(&id) {
        t.lock().unwrap().clear();
    }
    Json(serde_json::json!({ "ok": true }))
}

/// RAG: recupera de la memoria los recuerdos más RELEVANTES a la consulta y los
/// formatea como contexto, para que AION aplique lo que ha aprendido/investigado.
async fn relevant_knowledge(prompt: &str) -> String {
    // 1) COMPUERTA ADAPTATIVA: no recuperar para saludos/trivialidades (evita ruido).
    if is_trivial_query(prompt) {
        return String::new();
    }
    let Ok(mem) = VectorMemory::persistent_local(memory_path()) else {
        return String::new();
    };
    // Recuperación ASOCIATIVA: relevantes + relacionados por grafo (otros chats).
    let hits = match mem.retrieve_associative(prompt, 4, 1).await {
        Ok(h) => h,
        Err(_) => return String::new(),
    };
    // 2) Umbral dinámico sobre la puntuación híbrida: nos quedamos con lo que
    //    realmente destaca (>= 0.30 absoluto y dentro del 75% del mejor).
    let best = hits.first().map(|h| h.score).unwrap_or(0.0);
    if best < 0.30 {
        return String::new();
    }
    let cutoff = (best * 0.75).max(0.28);
    let useful: Vec<_> = hits
        .into_iter()
        .filter(|h| h.score >= cutoff)
        .take(4)
        .collect();
    if useful.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "Conocimiento de TU memoria relevante para esto (aplícalo si ayuda, con naturalidad):\n",
    );
    for h in useful {
        let mut c = h.content.clone();
        c.truncate(220);
        s.push_str(&format!("- {c}\n"));
    }
    s
}

/// Aterrizaje en la BIBLIOTECA (Academias): recupera pasajes relevantes de los
/// documentos/libros ingeridos para que el CHAT normal los aplique y CITE — son bases
/// de conocimiento, siempre consultables. Multilingüe (BGE-M3).
async fn library_grounding(prompt: &str) -> String {
    if is_trivial_query(prompt) {
        return String::new();
    }
    let lib = crate::library::Library::open(crate::knowledge_path());
    if lib.total_chunks() == 0 {
        return String::new();
    }
    let hits = match lib.search(prompt, 4, None).await {
        Ok(h) => h,
        Err(_) => return String::new(),
    };
    // Umbral: el coseno BGE-M3 separa relevante (~0.5+) de ruido (~0.3). 0.40 filtra bien.
    let useful: Vec<_> = hits.into_iter().filter(|p| p.score >= 0.40).collect();
    if useful.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "Conocimiento de TU BIBLIOTECA relevante para esto (úsalo y cita la fuente entre \
         corchetes cuando lo apliques):\n",
    );
    for (i, p) in useful.iter().enumerate() {
        let c = p.content.chars().take(300).collect::<String>();
        s.push_str(&format!("[{}] (fuente: {}) {c}\n", i + 1, p.source));
    }
    s
}

/// Aterrizaje del AGENTE con **reranker LLM** (Self-RAG): recupera (híbrido+MMR) y
/// luego un juez decide qué recuerdos son realmente ÚTILES para la tarea antes de
/// aplicarlos. Más precisión que el umbral solo (la latencia aquí es aceptable).
async fn grounding_for_agent(engine: &dyn LlmEngine, task: &str) -> String {
    if is_trivial_query(task) {
        return String::new();
    }
    let Ok(mem) = VectorMemory::persistent_local(memory_path()) else {
        return String::new();
    };
    let hits = match mem.retrieve_associative(task, 5, 1).await {
        Ok(h) => h
            .into_iter()
            .filter(|h| h.score >= 0.25)
            .collect::<Vec<_>>(),
        Err(_) => return String::new(),
    };
    if hits.is_empty() {
        return String::new();
    }
    // Juez de relevancia: ¿cuáles sirven para ESTA tarea?
    let listed = hits
        .iter()
        .enumerate()
        .map(|(i, h)| {
            format!(
                "{}. {}",
                i + 1,
                h.content.chars().take(180).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let judge = GenerateRequest {
        messages: vec![Message::user(format!(
            "Tarea: {task}\n\nRecuerdos candidatos:\n{listed}\n\n¿Cuáles son ÚTILES para \
             resolver la tarea? Responde SOLO los números separados por coma (p. ej. 1,3), \
             o 'ninguno'."
        ))],
        think: false,
        temperature: Some(0.0),
        max_tokens: Some(20),
    };
    let keep: Vec<usize> = match engine.generate(judge).await {
        Ok(m) => m
            .content
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|s| s.parse::<usize>().ok())
            .filter(|&n| n >= 1 && n <= hits.len())
            .map(|n| n - 1)
            .collect(),
        Err(_) => (0..hits.len().min(3)).collect(), // si falla el juez, usa los top
    };
    if keep.is_empty() {
        return String::new();
    }
    let mut s = String::from("CONOCIMIENTO QUE YA TIENES, útil para esta tarea (aplícalo):\n");
    for i in keep {
        s.push_str(&format!("- {}\n", hits[i].content));
    }
    s
}

/// Heurística barata para decidir CUÁNDO no merece la pena consultar memoria
/// (saludos, agradecimientos, entradas muy cortas sin contenido sustantivo).
fn is_trivial_query(prompt: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    let words = p.split_whitespace().count();
    const GREETINGS: [&str; 10] = [
        "hola", "buenas", "hey", "gracias", "ok", "vale", "adios", "adiós", "chao", "saludos",
    ];
    if words <= 2 && GREETINGS.iter().any(|g| p.starts_with(g)) {
        return true;
    }
    p.is_empty() || p.len() < 4
}

/// ¿Este intercambio merece ir a la memoria de LARGO PLAZO? La memoria permanente
/// es para lo ESTABLE (quién eres, preferencias, decisiones, aprendizajes), no para
/// el ESTADO ACTUAL del sistema, que cambia y envejece mal: cuántos archivos hay en
/// una carpeta, qué equipos están en la red ahora, la hora, el clima… Eso se calcula
/// en el momento con una herramienta; memorizarlo solo genera datos obsoletos.
fn worth_long_term(prompt: &str, answer: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    // Saludos / chitchat → no es conocimiento.
    if is_trivial_query(prompt) {
        return false;
    }
    // Estado efímero: conteos y consultas de "ahora mismo" que caducan.
    const EPHEMERAL: [&str; 22] = [
        "cuántos",
        "cuantos",
        "cuántas",
        "cuantas",
        "archivos",
        "documentos",
        "pdf",
        "carpeta",
        "escritorio",
        "descargas",
        "equipos",
        "dispositivos",
        "conectados",
        "red local",
        "ip",
        "qué hora",
        "que hora",
        "fecha de hoy",
        "clima",
        "tiempo hace",
        "batería",
        "bateria",
    ];
    if EPHEMERAL.iter().any(|k| p.contains(k)) {
        return false;
    }
    // Si la respuesta es básicamente una lista/escaneo de estado, tampoco.
    let a = answer.to_lowercase();
    if a.contains("equipos conectados en la red") || a.contains("archivos .") {
        return false;
    }
    true
}

/// ¿La pregunta MERECE razonamiento profundo? Activar el "thinking" de gemma para
/// algo trivial (saludo, recordar el nombre) gasta cientos de tokens y ~20 s para
/// nada. Solo razonamos en tareas que lo requieren; lo simple responde al instante.
fn needs_deep_thinking(prompt: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    let words = p.split_whitespace().count();
    // Marcadores de complejidad → sí conviene pensar.
    const HARD: [&str; 16] = [
        "analiza",
        "compara",
        "explica por",
        "por qué",
        "por que",
        "razona",
        "demuestra",
        "paso a paso",
        "código",
        "codigo",
        "programa",
        "calcula",
        "resuelve",
        "diseña",
        "plan",
        "estrategia",
    ];
    if HARD.iter().any(|k| p.contains(k)) {
        return true;
    }
    // Preguntas cortas / casuales → respuesta directa, sin cadena de pensamiento.
    if is_trivial_query(prompt) || words < 12 {
        return false;
    }
    // Mensajes largos o sustanciales → pensar.
    words >= 18
}

/// Estadísticas de la memoria de largo plazo.
async fn memory_stats() -> Json<serde_json::Value> {
    match VectorMemory::persistent_local(memory_path()) {
        Ok(m) => Json(serde_json::json!({ "count": m.len(), "path": memory_path() })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct RememberBody {
    text: String,
}

/// Guarda un recuerdo en la memoria persistente.
async fn memory_remember(Json(body): Json<RememberBody>) -> Json<serde_json::Value> {
    let mem = match VectorMemory::persistent_local(memory_path()) {
        Ok(m) => m,
        Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
    };
    match mem.store(&body.text).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id, "count": mem.len() })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

// ── Biblioteca (Academias): ingesta y consulta de documentos ────────────────

/// Lista los documentos ingeridos: dominio · fuente · nº de pasajes.
async fn library_list() -> Json<serde_json::Value> {
    let lib = crate::library::Library::open(crate::knowledge_path());
    let docs: Vec<serde_json::Value> = lib
        .documents()
        .into_iter()
        .map(|(domain, source, chunks)| serde_json::json!({ "domain": domain, "source": source, "chunks": chunks }))
        .collect();
    Json(serde_json::json!({ "total_chunks": lib.total_chunks(), "documents": docs }))
}

#[derive(Deserialize)]
struct IngestBody {
    domain: String,
    /// Ruta de archivo (.txt/.md/.pdf) en el equipo del usuario.
    path: String,
}

/// Ingesta un archivo del equipo en la biblioteca, bajo un dominio.
async fn library_ingest(Json(body): Json<IngestBody>) -> Json<serde_json::Value> {
    let mut lib = crate::library::Library::open(crate::knowledge_path());
    let p = std::path::PathBuf::from(&body.path);
    match lib.ingest_file(&body.domain, &p).await {
        Ok(n) => Json(
            serde_json::json!({ "ok": true, "passages": n, "total_chunks": lib.total_chunks() }),
        ),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

#[derive(Deserialize)]
struct UploadBody {
    domain: String,
    filename: String,
    /// Contenido del archivo en base64 (lo manda la UI tras leerlo con FileReader).
    content_b64: String,
}

/// Sube un documento desde la UI (sin necesidad de ruta): decodifica, lo guarda en
/// un temporal con su nombre (para conservar la extensión) y lo ingiere.
async fn library_upload(Json(body): Json<UploadBody>) -> Json<serde_json::Value> {
    use base64::Engine;
    let bytes = match base64::engine::general_purpose::STANDARD.decode(body.content_b64.as_bytes())
    {
        Ok(b) => b,
        Err(e) => return Json(serde_json::json!({ "error": format!("base64 inválido: {e}") })),
    };
    // Nombre seguro (sin separadores de ruta). El temporal lleva prefijo único, pero
    // la FUENTE guardada es el nombre original del libro (UX limpia + borrado correcto).
    let safe = body.filename.replace(['/', '\\'], "_");
    let tmp = std::env::temp_dir().join(format!("aion_upload_{safe}"));
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        return Json(serde_json::json!({ "error": format!("no pude guardar el archivo: {e}") }));
    }
    let mut lib = crate::library::Library::open(crate::knowledge_path());
    let result = lib.ingest_file_as(&body.domain, &safe, &tmp).await;
    let _ = std::fs::remove_file(&tmp);
    match result {
        Ok(n) => Json(serde_json::json!({
            "ok": true, "passages": n, "source": safe, "total_chunks": lib.total_chunks()
        })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

/// Encola un libro para ingesta en SEGUNDO PLANO: guarda los bytes en staging y
/// registra el trabajo. Devuelve al instante (no bloquea). El worker lo procesa.
async fn library_enqueue(Json(body): Json<UploadBody>) -> Json<serde_json::Value> {
    use base64::Engine;
    let bytes = match base64::engine::general_purpose::STANDARD.decode(body.content_b64.as_bytes())
    {
        Ok(b) => b,
        Err(e) => return Json(serde_json::json!({ "error": format!("base64 inválido: {e}") })),
    };
    let safe = body.filename.replace(['/', '\\'], "_");
    let id = uuid::Uuid::new_v4().to_string();
    let staged = crate::ingest_queue::staging_dir().join(format!("{id}_{safe}"));
    if let Err(e) = std::fs::write(&staged, &bytes) {
        return Json(serde_json::json!({ "error": format!("no pude guardar el archivo: {e}") }));
    }
    crate::ingest_queue::enqueue(&id, &body.domain, &safe, &staged.to_string_lossy());
    Json(serde_json::json!({ "ok": true, "id": id, "queued": safe }))
}

/// Estado de la cola de ingesta (para que la UI muestre el progreso).
async fn library_queue() -> Json<serde_json::Value> {
    Json(crate::ingest_queue::snapshot())
}

/// Limpia de la cola los trabajos ya terminados.
async fn library_queue_clear() -> Json<serde_json::Value> {
    let n = crate::ingest_queue::clear_finished();
    Json(serde_json::json!({ "ok": true, "cleared": n }))
}

#[derive(Deserialize)]
struct RemoveBody {
    domain: String,
    source: String,
}

/// Elimina un documento de la biblioteca (todos sus pasajes).
async fn library_remove(Json(body): Json<RemoveBody>) -> Json<serde_json::Value> {
    let mut lib = crate::library::Library::open(crate::knowledge_path());
    match lib.remove(&body.domain, &body.source) {
        Ok(n) => Json(
            serde_json::json!({ "ok": true, "removed": n, "total_chunks": lib.total_chunks() }),
        ),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

// ── Human-in-the-loop: confirmaciones pendientes ────────────────────────────

type Pending =
    std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>;

fn pending_confirms() -> &'static Pending {
    static P: std::sync::OnceLock<Pending> = std::sync::OnceLock::new();
    P.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Pide confirmación al usuario: emite un evento «confirm» por SSE y espera su
/// decisión (vía /api/confirm). Por seguridad, si no responde en 5 min → DENIEGA.
async fn request_confirmation(tx: &tokio::sync::mpsc::Sender<Event>, desc: String) -> bool {
    let id = uuid::Uuid::new_v4().to_string();
    let (otx, orx) = tokio::sync::oneshot::channel();
    pending_confirms().lock().unwrap().insert(id.clone(), otx);
    let _ = tx
        .send(
            Event::default()
                .data(serde_json::json!({ "kind": "confirm", "id": id, "text": desc }).to_string()),
        )
        .await;
    match tokio::time::timeout(std::time::Duration::from_secs(300), orx).await {
        Ok(Ok(approved)) => approved,
        _ => {
            pending_confirms().lock().unwrap().remove(&id);
            false // timeout o canal caído → no ejecutar (seguro por defecto)
        }
    }
}

#[derive(Deserialize)]
struct ConfirmDecision {
    id: String,
    approved: bool,
}

/// El usuario aprueba o rechaza una acción sensible pendiente.
async fn confirm_decision(Json(b): Json<ConfirmDecision>) -> Json<serde_json::Value> {
    if let Some(tx) = pending_confirms().lock().unwrap().remove(&b.id) {
        let _ = tx.send(b.approved);
        Json(serde_json::json!({ "ok": true }))
    } else {
        Json(serde_json::json!({ "ok": false, "error": "confirmación no encontrada o expirada" }))
    }
}

// ── Bóveda de credenciales (Llavero) ────────────────────────────────────────

#[derive(Deserialize)]
struct CredSetBody {
    host: String,
    user: String,
    pass: String,
}

/// Guarda credenciales en la bóveda (Llavero). La contraseña ENTRA pero nunca se
/// devuelve por ningún endpoint ni al LLM.
async fn credentials_set(Json(b): Json<CredSetBody>) -> Json<serde_json::Value> {
    match crate::credentials::set(&b.host, &b.user, &b.pass) {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

/// Lista los sitios guardados (host + usuario). NUNCA incluye contraseñas.
async fn credentials_list() -> Json<serde_json::Value> {
    let items: Vec<serde_json::Value> = crate::credentials::list()
        .into_iter()
        .map(|c| serde_json::json!({ "host": c.host, "user": c.user }))
        .collect();
    Json(serde_json::json!({ "credentials": items }))
}

#[derive(Deserialize)]
struct CredRemoveBody {
    host: String,
}

async fn credentials_remove(Json(b): Json<CredRemoveBody>) -> Json<serde_json::Value> {
    match crate::credentials::remove(&b.host) {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

#[derive(Deserialize)]
struct VisionBody {
    #[serde(default)]
    prompt: String,
    /// Imagen en base64 (sin el prefijo data:).
    image_b64: String,
}

/// Visión: describe/analiza una imagen adjunta. Usa el modelo BASE con proyector de
/// visión (`huihui_ai/gemma-4-abliterated:12b`), no `gemma4-reason` (que no tiene
/// visión). Configurable con AION_VISION_MODEL. Solo local.
async fn vision(Json(body): Json<VisionBody>) -> Json<serde_json::Value> {
    let provider = crate::provider::load();
    if provider.kind == "external" {
        return Json(serde_json::json!({
            "error": "la visión de imágenes requiere el modelo local (gemma)"
        }));
    }
    let prompt = if body.prompt.trim().is_empty() {
        "Describe con detalle lo que ves en esta imagen."
    } else {
        body.prompt.trim()
    };
    let vision_model = std::env::var("AION_VISION_MODEL")
        .unwrap_or_else(|_| "huihui_ai/gemma-4-abliterated:12b".into());
    let engine = OllamaEngine::new(OllamaEngine::base_url_from_env(), &vision_model);
    match engine.generate_with_image(prompt, &body.image_b64).await {
        Ok(m) => Json(serde_json::json!({ "answer": m.content })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct AskBody {
    query: String,
    #[serde(default)]
    domain: Option<String>,
}

/// Consulta la biblioteca: recupera pasajes (multilingüe) y responde citando fuentes.
async fn library_ask(Json(body): Json<AskBody>) -> Json<serde_json::Value> {
    let lib = crate::library::Library::open(crate::knowledge_path());
    if lib.total_chunks() == 0 {
        return Json(serde_json::json!({ "error": "la biblioteca está vacía" }));
    }
    let hits = match lib.search(&body.query, 5, body.domain.as_deref()).await {
        Ok(h) => h,
        Err(e) => return Json(serde_json::json!({ "error": e })),
    };
    let mut grounding = String::new();
    let sources: Vec<serde_json::Value> = hits
        .iter()
        .enumerate()
        .map(|(i, p)| {
            grounding.push_str(&format!("[{}] (fuente: {}, frag {}) {}\n\n", i + 1, p.source, p.idx, p.content));
            serde_json::json!({ "n": i + 1, "domain": p.domain, "source": p.source, "idx": p.idx, "score": p.score })
        })
        .collect();
    let engine = active_engine();
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Responde USANDO SOLO los pasajes. Cita la fuente con [n] donde uses cada \
                 dato. Si no contienen la respuesta, dilo con franqueza; no inventes. Español.",
            ),
            Message::user(format!(
                "Pasajes:\n{grounding}\nPregunta: {}\n\nRespuesta:",
                body.query
            )),
        ],
        think: false,
        temperature: Some(0.3),
        max_tokens: Some(600),
    };
    let answer = match engine.generate(req).await {
        Ok(m) => m.content,
        Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
    };
    Json(serde_json::json!({ "answer": answer, "sources": sources }))
}

/// Ejecuta el ciclo de consolidación darwiniana ("sueño").
async fn memory_sleep() -> Json<serde_json::Value> {
    let mem = match VectorMemory::persistent_local(memory_path()) {
        Ok(m) => m,
        Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
    };
    match mem.consolidate(&ConsolidationConfig::default()) {
        Ok(r) => Json(serde_json::json!({
            "before": r.before, "merged": r.merged, "pruned": r.pruned, "after": r.after
        })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// **Exporta** la memoria como archivo JSONL descargable (para llevarla a otro PC/Mac).
async fn memory_export() -> impl axum::response::IntoResponse {
    let body = match VectorMemory::persistent_local(memory_path()) {
        Ok(m) => m.export_jsonl(),
        Err(_) => String::new(),
    };
    (
        [
            (axum::http::header::CONTENT_TYPE, "application/x-ndjson"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"aion-memory.jsonl\"",
            ),
        ],
        body,
    )
}

#[derive(Deserialize)]
struct ImportBody {
    /// Contenido JSONL (formato de export). Se fusiona con la memoria actual.
    jsonl: String,
}

/// **Importa** memoria desde un archivo JSONL (fusiona, omite duplicados por id).
async fn memory_import(Json(body): Json<ImportBody>) -> Json<serde_json::Value> {
    let mem = match VectorMemory::persistent_local(memory_path()) {
        Ok(m) => m,
        Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
    };
    match mem.import_jsonl(&body.jsonl) {
        Ok(added) => Json(serde_json::json!({ "ok": true, "added": added, "count": mem.len() })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

// ── Bandeja de AION (mensajes proactivos del agente hacia ti) ───────────────

/// Lista los mensajes que AION te ha escrito (los no leídos primero).
async fn inbox_list() -> Json<serde_json::Value> {
    match crate::inbox::Inbox::open(crate::inbox_path()) {
        Ok(ibx) => {
            let unread = ibx.unread().unwrap_or_default();
            let all = ibx.all().unwrap_or_default();
            Json(serde_json::json!({ "unread": unread, "unread_count": unread.len(), "all": all }))
        }
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct InboxReadBody {
    #[serde(default)]
    id: Option<String>,
}

/// Marca como leído un mensaje (o todos si no se da id).
async fn inbox_read(Json(body): Json<InboxReadBody>) -> Json<serde_json::Value> {
    match crate::inbox::Inbox::open(crate::inbox_path()) {
        Ok(ibx) => {
            let _ = ibx.mark_read(body.id.as_deref());
            Json(serde_json::json!({ "ok": true }))
        }
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

// Pequeño helper para mapear el stream a Result sin traer todo StreamExt.
use tokio_stream::StreamExt;
