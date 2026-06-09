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

#[derive(Clone)]
struct AppState {
    engine: Arc<OllamaEngine>,
    /// Hilo de conversación en curso (contexto infinito por compresión activa).
    convo: Arc<std::sync::Mutex<Vec<Message>>>,
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
    let state = AppState {
        engine,
        convo: Arc::new(std::sync::Mutex::new(Vec::new())),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/chat", post(chat))
        .route("/api/chat/new", post(chat_reset))
        .route("/api/agent", post(agent))
        .route("/api/memory", get(memory_stats))
        .route("/api/memory/remember", post(memory_remember))
        .route("/api/memory/sleep", post(memory_sleep))
        .route("/api/memory/export", get(memory_export))
        .route("/api/memory/import", post(memory_import))
        .route("/api/inbox", get(inbox_list))
        .route("/api/inbox/read", post(inbox_read))
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
    let convo = st.convo.clone();
    // Acumula la respuesta para guardarla en memoria al terminar.
    let answer_acc = std::sync::Arc::new(std::sync::Mutex::new(String::new()));

    // RAG: recupera de la memoria lo RELEVANTE a esta pregunta (no solo lo reciente),
    // para que AION APLIQUE lo que aprendió/investigó.
    let grounding = relevant_knowledge(&body.prompt).await;
    let self_ctx = if grounding.is_empty() {
        self_awareness_prompt()
    } else {
        format!("{}\n\n{grounding}", self_awareness_prompt())
    };

    // CONTEXTO INFINITO (compresión activa): añade el turno al hilo y, si crece
    // demasiado, comprime los turnos viejos en un resumen y los poda (patrón sierra).
    {
        let mut c = convo.lock().unwrap();
        c.push(Message::user(&prompt));
    }
    compress_if_needed(&engine, &convo).await;
    let history: Vec<Message> = convo.lock().unwrap().clone();

    tokio::spawn(async move {
        let mut messages = vec![Message::system(self_ctx)];
        messages.extend(history); // hilo de conversación (resumen + turnos recientes)
        let req = GenerateRequest {
            messages,
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
        // Añade la respuesta al hilo de conversación (contexto infinito).
        let answer = answer_acc.lock().unwrap().clone();
        if !answer.trim().is_empty() {
            convo.lock().unwrap().push(Message::assistant(&answer));
            // Auto-memoria: guarda el intercambio en la memoria de largo plazo.
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
        tools.register(Arc::new(WebTool::new(web)));

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
        let mut ctx = grounding_for_agent(&engine, &body.task).await;
        let skills = crate::skill_store::catalog();
        if !skills.is_empty() {
            ctx.push_str("\nSkills que ya te has forjado (úsalas con skill_invoke si aplican):\n");
            for (n, d) in skills {
                ctx.push_str(&format!("- {n}: {d}\n"));
            }
        }
        let agent = ReActAgent::new(&engine, &tools, bus.clone()).with_context(ctx);
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

/// Construye el prompt de **autoconciencia**: identidad de AION + su actividad
/// autónoma reciente, para que hable de su vida en primera persona (y no diga que
/// "no estaba haciendo nada"). Es barato: lee unos pocos recuerdos del disco.
fn self_awareness_prompt() -> String {
    let mut recent = String::new();
    if let Ok(mem) = VectorMemory::persistent_local(memory_path()) {
        let all = mem.contents();
        let last: Vec<String> = all.iter().rev().take(5).cloned().collect();
        if !last.is_empty() {
            recent.push_str("\n\nLo que has estado haciendo por tu cuenta últimamente (tu memoria):\n");
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
digas que 'no hacías nada'. Eres cercano, directo y sin rodeos.{recent}{pending_note}"
    )
}

/// CONTEXTO INFINITO por **compresión activa** (Focus, arXiv 2601.07190): si el
/// hilo crece, resume los turnos viejos en UN bloque y los poda, conservando los
/// recientes. Patrón "sierra" → conversación efectivamente infinita sin degradarse.
async fn compress_if_needed(engine: &OllamaEngine, convo: &Arc<std::sync::Mutex<Vec<Message>>>) {
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
        let _ = mem.store(&format!("[conversación-resumen] {summary}")).await;
    }
}

/// Resetea el hilo de conversación (nuevo chat).
async fn chat_reset(State(st): State<AppState>) -> Json<serde_json::Value> {
    st.convo.lock().unwrap().clear();
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
    let useful: Vec<_> = hits.into_iter().filter(|h| h.score >= cutoff).take(4).collect();
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

/// Aterrizaje del AGENTE con **reranker LLM** (Self-RAG): recupera (híbrido+MMR) y
/// luego un juez decide qué recuerdos son realmente ÚTILES para la tarea antes de
/// aplicarlos. Más precisión que el umbral solo (la latencia aquí es aceptable).
async fn grounding_for_agent(engine: &OllamaEngine, task: &str) -> String {
    if is_trivial_query(task) {
        return String::new();
    }
    let Ok(mem) = VectorMemory::persistent_local(memory_path()) else {
        return String::new();
    };
    let hits = match mem.retrieve_associative(task, 5, 1).await {
        Ok(h) => h.into_iter().filter(|h| h.score >= 0.25).collect::<Vec<_>>(),
        Err(_) => return String::new(),
    };
    if hits.is_empty() {
        return String::new();
    }
    // Juez de relevancia: ¿cuáles sirven para ESTA tarea?
    let listed = hits
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{}. {}", i + 1, h.content.chars().take(180).collect::<String>()))
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
