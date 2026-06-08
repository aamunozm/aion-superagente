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

use aion_kernel::events::{AionEvent, EventBus};
use aion_kernel::traits::{GenerateRequest, LlmEngine, StreamChunk};
use aion_kernel::types::Message;
use aion_llm::OllamaEngine;
use aion_orchestrator::{CalculatorTool, ReActAgent, ToolRegistry};
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
        .layer(cors)
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

    tokio::spawn(async move {
        let req = GenerateRequest {
            messages: vec![Message::user(body.prompt)],
            think: body.think,
            temperature: Some(1.0),
            max_tokens: None,
        };
        let tx2 = tx.clone();
        let result = engine
            .generate_stream(
                req,
                Box::new(move |chunk| {
                    let payload = match &chunk {
                        StreamChunk::Thinking { text } => {
                            serde_json::json!({ "kind": "thinking", "text": text })
                        }
                        StreamChunk::Answer { text } => {
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

// Pequeño helper para mapear el stream a Result sin traer todo StreamExt.
use tokio_stream::StreamExt;
