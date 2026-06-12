//! **Claude Code** — conexión de la memoria de AION con Claude Code vía MCP.
//! AION expone un endpoint MCP (`/mcp`, ver [[claude_mcp]]) y este módulo guarda la
//! config local (token Bearer, estado) y registra/desregistra el servidor en la CLI
//! de Claude (`claude mcp add -s user`). El token y el registro viven FUERA del
//! binario (claude_code.json + ~/.claude.json), así la conexión sobrevive updates.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// URL estable del endpoint MCP (mismo bind que el resto del IPC local).
pub const MCP_URL: &str = "http://127.0.0.1:8765/mcp";
/// Nombre con el que AION se registra en Claude Code.
pub const MCP_NAME: &str = "aion";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Si el endpoint MCP acepta conexiones de Claude Code.
    #[serde(default)]
    pub enabled: bool,
    /// Token Bearer requerido en /mcp. Se regenera en cada conexión (revocación).
    #[serde(default)]
    pub token: String,
    /// Si se expone el recurso `aion://brief` (resumen compacto de contexto).
    #[serde(default)]
    pub auto_brief: bool,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    /// Última vez que Claude Code llamó al endpoint (visto en auth).
    #[serde(default)]
    pub last_seen_at: Option<DateTime<Utc>>,
}

fn path() -> PathBuf {
    crate::app_data_dir().join("claude_code.json")
}

pub fn load() -> Config {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(c: &Config) {
    if let Ok(b) = serde_json::to_string_pretty(c) {
        crate::write_atomic(&path(), &b);
    }
}

/// Marca actividad (última llamada MCP) sin pisar el resto de la config en disco.
pub fn touch_last_seen() {
    let mut c = load();
    c.last_seen_at = Some(Utc::now());
    save(&c);
}

/// Token opaco de 64 hex chars (2× UUID v4 sin guiones).
pub fn generate_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

fn claude_json_path() -> PathBuf {
    home_dir().join(".claude.json")
}

/// Busca la CLI de Claude. Las apps GUI de macOS NO heredan el PATH del shell,
/// así que se prueban las rutas típicas y, como último recurso, un login shell.
pub fn find_claude_cli() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
        home_dir().join(".claude/local/claude"),
        home_dir().join(".local/bin/claude"),
    ];
    for c in candidates {
        if c.is_file() {
            return Some(c);
        }
    }
    let out = std::process::Command::new("/bin/zsh")
        .args(["-lc", "command -v claude"])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

/// Registra el servidor MCP de AION en Claude Code (scope user, transporte HTTP).
/// Primero vía CLI; si el exec falla, fallback: merge directo en ~/.claude.json.
/// CLI ausente → Err("cli_not_found") para que la UI muestre el hint de instalación.
pub fn register(token: &str) -> Result<(), String> {
    let Some(cli) = find_claude_cli() else {
        return Err("cli_not_found".into());
    };
    // Limpia un registro previo para que reconectar regenere limpio (ignora error).
    let _ = std::process::Command::new(&cli)
        .args(["mcp", "remove", "-s", "user", MCP_NAME])
        .output();
    let auth = format!("Authorization: Bearer {token}");
    let out = std::process::Command::new(&cli)
        .args([
            "mcp", "add", "-s", "user", "-t", "http", MCP_NAME, MCP_URL, "-H", &auth,
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(()),
        _ => register_fallback(token),
    }
}

/// Fallback sin CLI: edita SOLO `mcpServers.aion` en ~/.claude.json (atómico).
fn register_fallback(token: &str) -> Result<(), String> {
    let p = claude_json_path();
    let mut root: serde_json::Value = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "~/.claude.json no es un objeto JSON".to_string())?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    servers
        .as_object_mut()
        .ok_or_else(|| "mcpServers no es un objeto".to_string())?
        .insert(
            MCP_NAME.into(),
            serde_json::json!({
                "type": "http",
                "url": MCP_URL,
                "headers": { "Authorization": format!("Bearer {token}") },
            }),
        );
    let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    crate::write_atomic(&p, &body);
    Ok(())
}

/// Quita el servidor de Claude Code (CLI primero, fallback edición directa).
pub fn unregister() -> Result<(), String> {
    if let Some(cli) = find_claude_cli() {
        let out = std::process::Command::new(&cli)
            .args(["mcp", "remove", "-s", "user", MCP_NAME])
            .output();
        if matches!(out, Ok(ref o) if o.status.success()) {
            return Ok(());
        }
    }
    let p = claude_json_path();
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Ok(()); // nada que quitar
    };
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(());
    };
    if let Some(servers) = root.get_mut("mcpServers").and_then(|s| s.as_object_mut()) {
        servers.remove(MCP_NAME);
        if let Ok(body) = serde_json::to_string_pretty(&root) {
            crate::write_atomic(&p, &body);
        }
    }
    Ok(())
}

/// ¿Figura `aion` en los mcpServers de Claude Code? (lectura, sin CLI)
pub fn is_registered() -> bool {
    std::fs::read_to_string(claude_json_path())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| v.get("mcpServers").and_then(|s| s.get(MCP_NAME)).is_some())
        .unwrap_or(false)
}

/// BRIEF compacto (~600 tokens máx) para orientar a Claude Code: quién es este
/// AION, recuerdos recientes, proyectos y dominios de la biblioteca. Nunca expone
/// el id de identidad, tokens ni credenciales. Cache de 5 minutos.
pub async fn build_brief() -> String {
    static CACHE: OnceLock<Mutex<Option<(Instant, String)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Some((t, s)) = cache.lock().unwrap().as_ref() {
        if t.elapsed().as_secs() < 300 {
            return s.clone();
        }
    }

    let me = crate::identity::get();
    let mut out = format!(
        "AION «{}» — asistente local del usuario. Resumen de su contexto:\n",
        me.name
    );

    // Recuerdos recientes vigentes (con fecha), truncados.
    if let Ok(mem) = aion_memory::VectorMemory::persistent_local(crate::memory_path()) {
        let recent = mem.recent_with_time(10);
        if !recent.is_empty() {
            out.push_str("\n## Recuerdos recientes\n");
            for (content, ts) in recent {
                let c: String = content.chars().take(180).collect();
                out.push_str(&format!("- [{}] {}\n", ts.format("%Y-%m-%d"), c));
            }
        }
    }

    // Proyectos del workspace (nombre + descripción).
    let projects = crate::projects::list();
    if !projects.is_empty() {
        out.push_str("\n## Proyectos\n");
        for p in projects.iter().take(12) {
            let d: String = p.desc.chars().take(120).collect();
            out.push_str(&format!("- {} · {} ({})\n", p.id, p.name, d));
        }
    }

    // Dominios/documentos de la biblioteca de conocimiento.
    let lib = crate::library::Library::open(crate::knowledge_path());
    let docs = lib.documents();
    if !docs.is_empty() {
        out.push_str("\n## Biblioteca\n");
        for (domain, source, chunks) in docs.iter().take(15) {
            out.push_str(&format!("- {domain}/{source} ({chunks} pasajes)\n"));
        }
    }

    // Techo duro ~600 tokens (≈2400 chars).
    if out.chars().count() > 2400 {
        out = out.chars().take(2400).collect();
    }
    *cache.lock().unwrap() = Some((Instant::now(), out.clone()));
    out
}
