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
use crate::skill_tool::SkillTool;
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

#[derive(Clone)]
struct AppState {
    engine: Arc<OllamaEngine>,
}

#[derive(Deserialize)]
struct ChatBody {
    prompt: String,
    #[serde(default)]
    think: bool,
}

/// Arranca el puente HTTP en la dirección indicada.
pub async fn run(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let engine = Arc::new(OllamaEngine::default_local());
    let state = AppState { engine };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/chat", post(chat))
        .route("/api/agent", post(agent))
        .route("/api/memory", get(memory_stats))
        .route("/api/memory/remember", post(memory_remember))
        .route("/api/memory/sleep", post(memory_sleep))
        .route("/api/memory/export", get(memory_export))
        .route("/api/memory/import", post(memory_import))
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
    let ok = st.engine.health().await.is_ok();
    Json(serde_json::json!({ "ok": ok, "engine": st.engine.id() }))
}

/// Chat con streaming SSE. Cada evento lleva JSON `{kind, text}` o `{kind:"done",...}`.
async fn chat(
    State(st): State<AppState>,
    Json(body): Json<ChatBody>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
    let engine = st.engine.clone();
    let prompt = body.prompt.clone();
    // Acumula la respuesta para guardarla en memoria al terminar.
    let answer_acc = std::sync::Arc::new(std::sync::Mutex::new(String::new()));

    tokio::spawn(async move {
        let req = GenerateRequest {
            messages: vec![Message::user(body.prompt)],
            think: body.think,
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
        // Auto-memoria: guarda el intercambio en la memoria de largo plazo.
        let answer = answer_acc.lock().unwrap().clone();
        if !answer.trim().is_empty() {
            if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
                let mut a = answer;
                a.truncate(600);
                let entry = format!("[conversación] yo: {prompt} · AION: {a}");
                let _ = mem.store(&entry).await;
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(Ok);
    Sse::new(stream)
}

#[derive(Deserialize)]
struct AgentBody {
    task: String,
}

/// Agente ReAct con herramientas. Emite por SSE los pasos (thought/action/
/// observation) y al final `answer` + `done`.
async fn agent(
    State(_st): State<AppState>,
    Json(body): Json<AgentBody>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);

    tokio::spawn(async move {
        let engine = OllamaEngine::default_local();
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(CalculatorTool));

        // Skill WASM (sandbox) como herramienta.
        if let Ok(skill_host) = WasmSkillHost::new() {
            if skill_host
                .register(
                    SkillManifest {
                        name: "sum_to".into(),
                        description: "suma 1..=n".into(),
                    },
                    SUM_TO_WAT,
                )
                .is_ok()
            {
                tools.register(Arc::new(SkillTool::new(
                    Arc::new(skill_host),
                    "sum_to",
                    "Suma todos los enteros de 1 hasta n (skill WASM en sandbox). Entrada: n.",
                )));
            }
        }
        // Memoria de largo plazo como herramienta.
        if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
            tools.register(Arc::new(MemoryTool::new(Arc::new(mem), 3)));
        }
        // Capacidad web (leer URLs).
        tools.register(Arc::new(WebTool::new(Arc::new(WebClient::new()))));

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

        let agent = ReActAgent::new(&engine, &tools, bus.clone());
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

// Pequeño helper para mapear el stream a Result sin traer todo StreamExt.
use tokio_stream::StreamExt;
