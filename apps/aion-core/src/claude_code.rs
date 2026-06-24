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
        // Contiene el token Bearer → 0600 (antes 0644 world-readable).
        crate::write_atomic_secret(&path(), &b);
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

/// **Token LOCAL del API, ESTABLE entre reinicios.** Se lee de `app_data_dir/api_token`
/// o, la primera vez, se genera y se persiste (0600). Antes el token era EFÍMERO (un UUID
/// nuevo en cada arranque), de modo que cada reinicio —y cada actualización OTA— dejaba
/// `~/.claude.json` con el token viejo y ROMPÍA la conexión MCP Claude Code↔AION hasta
/// re-registrar. Persistirlo mantiene la conexión viva entre reinicios sin re-sincronizar.
pub fn persisted_token() -> String {
    let p = crate::app_data_dir().join("api_token");
    if let Ok(t) = std::fs::read_to_string(&p) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return t;
        }
    }
    let t = generate_token();
    crate::write_atomic_secret(&p, &t);
    t
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

/// Registra el servidor MCP de AION en Claude Code (scope user, transporte HTTP)
/// editando directamente `mcpServers.aion` en ~/.claude.json. NO usa `claude mcp add`
/// porque eso pasaría el token por argv (visible en `ps` durante el registro); el
/// `insert` reemplaza cualquier entrada previa, así reconectar regenera limpio.
/// CLI ausente → Err("cli_not_found") para que la UI muestre el hint de instalación.
pub fn register(token: &str) -> Result<(), String> {
    if find_claude_cli().is_none() {
        return Err("cli_not_found".into());
    }
    register_fallback(token)?;
    // Auto-configura permisos + hook de arranque para que el ahorro de tokens funcione SOLO,
    // en cualquier máquina, sin pegar JSON a mano (best-effort: si falla, la conexión MCP sigue).
    if let Err(e) = configure_claude_settings() {
        tracing::debug!(error = %e, "auto-configuración de ~/.claude/settings.json omitida");
    }
    Ok(())
}

/// Edita SOLO `mcpServers.aion` en ~/.claude.json (atómico, sin exponer el token).
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
    // ~/.claude.json lleva el token Bearer → 0600 (no degradar a 0644 al reescribir).
    crate::write_atomic_secret(&p, &body);
    Ok(())
}

/// Quita el servidor de Claude Code (CLI primero, fallback edición directa).
pub fn unregister() -> Result<(), String> {
    // Revierte la auto-configuración (allowlist + hook) para no dejar residuos al desconectar.
    let _ = deconfigure_claude_settings();
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
            crate::write_atomic_secret(&p, &body);
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

/// Token Bearer con el que `aion` está registrado en ~/.claude.json, si lo hay. Permite
/// detectar una DESINCRONIZACIÓN con `cfg.token` (lo que valida `/mcp`) y re-registrar:
/// esa divergencia es justo lo que producía el 401 "Token inválido" tras un reinicio/OTA.
pub fn registered_token() -> Option<String> {
    let v: serde_json::Value = std::fs::read_to_string(claude_json_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())?;
    v.get("mcpServers")?
        .get(MCP_NAME)?
        .get("headers")?
        .get("Authorization")?
        .as_str()?
        .strip_prefix("Bearer ")
        .map(|s| s.to_string())
}

/// Ruta del `settings.json` GLOBAL de Claude Code (permisos + hooks; NO lleva secretos).
fn claude_settings_path() -> PathBuf {
    home_dir().join(".claude").join("settings.json")
}

/// Las tools MCP de AION que se auto-permiten (allowlist) para que Claude Code NO pida
/// confirmación manual en cada llamada — el retrieval bajo demanda solo ahorra tokens si
/// fluye sin fricción. Debe estar sincronizada con las tools expuestas en [[claude_mcp]].
const AION_MCP_TOOLS: [&str; 8] = [
    "mcp__aion__aion_brief",
    "mcp__aion__aion_memory_search",
    "mcp__aion__aion_library_search",
    "mcp__aion__aion_graph_query",
    "mcp__aion__aion_project_context",
    "mcp__aion__aion_remember",
    "mcp__aion__aion_forget",
    "mcp__aion__aion_episodic_recall",
];

/// Marcador único del hook de AION dentro del `command` (también es el prefijo del sentinel).
/// Permite detectar idempotencia y revertir sin tocar hooks de otras herramientas.
const AION_HOOK_MARKER: &str = "/tmp/.aion_brief_";

/// Comando del hook `UserPromptSubmit`. En el PRIMER prompt de CADA sesión (cualquier proyecto,
/// rama o carpeta) inyecta un recordatorio CONDICIONAL: que Claude Code llame a `aion_brief` SOLO
/// si la sesión trata del trabajo de Ariel (código, proyectos, decisiones, preferencias) y que lo
/// OMITA en preguntas triviales o no relacionadas — así el brief (~450 tok) no se paga en sesiones
/// que no usan la memoria de AION (gate del coste por sesión). Portable: extrae `session_id` con
/// `sed` (macOS NO trae `jq`) y cae a `nosession`. El sentinel en `/tmp` evita repetir tras el 1er
/// prompt (coste cero desde el 2º).
const AION_BRIEF_HOOK_CMD: &str = "SID=$(sed -n 's/.*\"session_id\"[^\"]*\"\\([^\"]*\\)\".*/\\1/p'); [ -z \"$SID\" ] && SID=nosession; S=\"/tmp/.aion_brief_$SID\"; if [ ! -f \"$S\" ]; then touch \"$S\"; printf '%s' '{\"hookSpecificOutput\":{\"hookEventName\":\"UserPromptSubmit\",\"additionalContext\":\"[AION] Primera vez en esta sesion. SI trata sobre codigo, proyectos, decisiones o preferencias de Ariel, llama mcp__aion__aion_brief (y aion_memory_search antes de asumir contexto sobre el o sus proyectos). Si es trivial, generica o no relacionada con su trabajo, OMITELO para no gastar tokens.\"}}'; fi";

/// ¿El grupo de hooks contiene el hook de AION? (busca el marcador en cualquier `command`).
fn group_has_aion_hook(group: &serde_json::Value) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.contains(AION_HOOK_MARKER))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// **Auto-configura el `~/.claude/settings.json` GLOBAL de Claude Code para el ahorro de
/// tokens con AION.** Hace que la función sea portable y "se configure sola" en cualquier Mac:
///   1. **Allowlist** de las tools MCP de AION → cero prompts de permiso por llamada.
///   2. **Hook `UserPromptSubmit`** → en el primer prompt de cada sesión recuerda llamar a
///      `aion_brief` (trigger DETERMINISTA, independiente del criterio del LLM).
///
/// Idempotente y MERGE-safe: preserva cualquier permiso, hook o ajuste que el usuario ya tenga
/// (añade solo lo que falte). Sin secretos. Best-effort: si algo falla, la conexión MCP sigue
/// funcionando — solo se pierden los auto-permisos y el recordatorio de arranque.
pub fn configure_claude_settings() -> Result<(), String> {
    let p = claude_settings_path();
    let mut root: serde_json::Value = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    apply_aion_settings(&mut root)?;
    let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    crate::write_atomic(&p, &body);
    Ok(())
}

/// Lógica PURA de merge (sin I/O) — testeable: inserta allowlist + hook en `root`, preservando
/// lo existente. Idempotente y MERGE-safe. Devuelve Err si `permissions`/`hooks` ya existen con
/// un tipo incompatible (no son objeto/array), para no pisar una config manual del usuario.
fn apply_aion_settings(root: &mut serde_json::Value) -> Result<(), String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "~/.claude/settings.json no es un objeto JSON".to_string())?;

    // (1) Allowlist — añade las tools que falten, preservando las existentes.
    let allow_arr = obj
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "permissions no es un objeto".to_string())?
        .entry("allow")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "permissions.allow no es un array".to_string())?;
    for tool in AION_MCP_TOOLS {
        if !allow_arr.iter().any(|v| v.as_str() == Some(tool)) {
            allow_arr.push(serde_json::Value::from(tool));
        }
    }

    // (2) Hook UserPromptSubmit — AUTO-SANA: quita cualquier hook de AION previo (por el marcador)
    // y reinserta el actual, así una versión vieja del comando se actualiza sola en el próximo
    // arranque. Idempotente (drop+readd del mismo) y preserva los hooks NO-AION del usuario.
    let ups_arr = obj
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "hooks no es un objeto".to_string())?
        .entry("UserPromptSubmit")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "hooks.UserPromptSubmit no es un array".to_string())?;
    ups_arr.retain(|g| !group_has_aion_hook(g));
    ups_arr.push(serde_json::json!({
        "hooks": [{
            "type": "command",
            "timeout": 5,
            "command": AION_BRIEF_HOOK_CMD,
        }]
    }));
    Ok(())
}

/// Revierte `configure_claude_settings`: quita las tools de AION del allowlist y el grupo de
/// hook de AION, preservando todo lo demás. Idempotente y best-effort (si el archivo no existe
/// o no parsea, no hay nada que revertir).
pub fn deconfigure_claude_settings() -> Result<(), String> {
    let p = claude_settings_path();
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Ok(());
    };
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(());
    };
    remove_aion_settings(&mut root);
    if let Ok(body) = serde_json::to_string_pretty(&root) {
        crate::write_atomic(&p, &body);
    }
    Ok(())
}

/// Lógica PURA de reversión (sin I/O) — testeable.
fn remove_aion_settings(root: &mut serde_json::Value) {
    if let Some(allow) = root
        .get_mut("permissions")
        .and_then(|p| p.get_mut("allow"))
        .and_then(|a| a.as_array_mut())
    {
        allow.retain(|v| {
            !v.as_str()
                .map(|s| AION_MCP_TOOLS.contains(&s))
                .unwrap_or(false)
        });
    }
    if let Some(ups) = root
        .get_mut("hooks")
        .and_then(|h| h.get_mut("UserPromptSubmit"))
        .and_then(|u| u.as_array_mut())
    {
        ups.retain(|group| !group_has_aion_hook(group));
    }
}

/// BRIEF compacto (~450 tokens máx) para orientar a Claude Code: quién es este
/// AION, recuerdos recientes (de-duplicados), proyectos y dominios de la biblioteca.
/// Nunca expone el id de identidad, tokens ni credenciales. Cache de 5 minutos.
pub async fn build_brief() -> String {
    static CACHE: OnceLock<Mutex<Option<(Instant, String)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Some((t, s)) = cache.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if t.elapsed().as_secs() < 300 {
            return s.clone();
        }
    }

    let me = crate::identity::get();
    let mut out = format!(
        "AION «{}» — asistente local del usuario. Resumen de su contexto:\n",
        me.name
    );

    // Recuerdos recientes vigentes, DURABLES, DE-DUPLICADOS y truncados. Las fechas
    // desconocidas (epoch 1970) se omiten en vez de mentir.
    if let Ok(mem) = crate::shared_memory() {
        // Se piden de más (24) para backfill tras descartar ruido (eco conversacional/deudas
        // resueltas) y casi-duplicados, y aun así quedarnos con 8 recuerdos DURABLES y
        // distintos: menos tokens, cero pérdida de información real.
        let recent = mem.recent_with_time(24);
        let mut shown: Vec<String> = Vec::new();
        let mut lines = String::new();
        // Las auto-reflexiones del loop de vida interior ([insight]/[conocimiento], síntesis del
        // LLM en 1ª persona) son introspección de AION, no hechos de Ariel ni decisiones de
        // proyecto: a Claude Code en tareas de código no le aportan, y suelen ser verbosas y
        // copar el brief (coste GARANTIZADO por sesión). Se limita su número para dejar sitio a lo
        // accionable (hechos/proyectos); NO se borran (siguen en memoria y en aion_memory_search).
        const MAX_SELF_REFLECTION: usize = 2;
        let mut self_reflections = 0usize;
        for (content, ts) in recent.into_iter().rev() {
            // Fuera el eco conversacional y las deudas ya resueltas: son turnos de charla
            // efímera cuyos HECHOS ya viven como [hecho]/[proyecto] aparte. No pagan su sitio
            // en el brief (que es coste por sesión). El recuerdo sigue en memoria y buscable.
            if is_brief_noise(&content) {
                continue;
            }
            if is_self_reflection(&content) {
                if self_reflections >= MAX_SELF_REFLECTION {
                    continue;
                }
                self_reflections += 1;
            }
            let c: String = content.chars().take(180).collect();
            if shown.iter().any(|s| near_duplicate(s, &c)) {
                continue;
            }
            // El brief es coste GARANTIZADO por sesión y lo consume SOLO Claude Code
            // (tokens de pago) → se muestra la versión inglesa cacheada (~40% menos
            // tokens) cuando existe; en miss se muestra español y se calienta para la
            // próxima. Se compacta desde el recuerdo COMPLETO (`content`) truncando a 180:
            // así comparte traducción/caché con `aion_memory_search` y el grafo (clave por
            // contenido completo). La de-duplicación sigue sobre el español (`c`), para que el
            // filtro no varíe según esté o no traducido el recuerdo.
            let display = crate::mcp_compact::compact_dense(&content, 180);
            if aion_memory::is_unknown_time(ts) {
                lines.push_str(&format!("- {display}\n"));
            } else {
                lines.push_str(&format!("- [{}] {}\n", ts.format("%Y-%m-%d"), display));
            }
            shown.push(c);
            if shown.len() >= 8 {
                break;
            }
        }
        if !lines.is_empty() {
            out.push_str("\n## Recuerdos recientes\n");
            out.push_str(&lines);
        }
    }

    // Proyectos del workspace (nombre + descripción). Fuera los de PRUEBA (TEST_*, itest):
    // son fixtures de desarrollo, no contexto real del usuario — no deben gastar tokens aquí.
    let projects: Vec<_> = crate::projects::list()
        .into_iter()
        .filter(|p| !is_test_fixture(&p.name))
        .collect();
    if !projects.is_empty() {
        out.push_str("\n## Proyectos\n");
        for p in projects.iter().take(12) {
            let d: String = p.desc.chars().take(120).collect();
            out.push_str(&format!("- {} · {} ({})\n", p.id, p.name, d));
        }
    }

    // Dominios/documentos de la biblioteca de conocimiento. Fuera los de prueba (dominio
    // itest / fixtures): no son la biblioteca real del usuario.
    let lib = crate::library::Library::open(crate::knowledge_path());
    let docs: Vec<_> = lib
        .documents()
        .into_iter()
        .filter(|(domain, source, _)| !is_test_fixture(domain) && !is_test_fixture(source))
        .collect();
    if !docs.is_empty() {
        out.push_str("\n## Biblioteca\n");
        for (domain, source, chunks) in docs.iter().take(15) {
            out.push_str(&format!("- {domain}/{source} ({chunks} pasajes)\n"));
        }
    }

    // Grafo de conocimiento: orienta sobre estructura sin necesitar una llamada extra a
    // aion_graph_query. Una sola línea (≤120 chars / ~30 tokens) con conteo de conceptos,
    // comunidades y los 3 temas principales — suficiente para decidir si vale la pena
    // profundizar con una query directa al grafo.
    let g = crate::graph::KnowledgeGraph::open(crate::graph_path());
    let g_summary = g.brief_summary();
    if !g_summary.is_empty() {
        out.push_str(&format!("\n## Grafo de conocimiento\n{g_summary}\n"));
    }

    // Techo duro ~450 tokens (≈1800 chars): el brief es coste por sesión, se mantiene
    // compacto sin perder lo esencial (identidad + recientes de-duplicados + proyectos).
    if out.chars().count() > 1800 {
        out = out.chars().take(1800).collect();
    }
    *cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), out.clone()));
    out
}

/// ¿Es este recuerdo RUIDO EFÍMERO para el brief? El eco conversacional ("[conversación]
/// yo: … · AION: …") y las deudas ya resueltas ("[resuelto] …") son turnos de charla cuyos
/// hechos durables ya viven aparte como `[hecho]`/`[proyecto:]`. Mostrarlos en el brief
/// gasta tokens cada sesión sin añadir contexto. Solo afecta a la PRESENTACIÓN del brief:
/// el recuerdo permanece en memoria y sigue siendo recuperable por `aion_memory_search`.
fn is_brief_noise(content: &str) -> bool {
    let c = content.trim_start().to_lowercase();
    c.starts_with("[conversación]")
        || c.starts_with("[conversacion]")
        || c.starts_with("[resuelto]")
}

/// ¿Es una AUTO-REFLEXIÓN del loop de vida interior? Las síntesis introspectivas que AION genera
/// con el LLM en 1ª persona se guardan como `[insight]`/`[conocimiento]` (ver `main.rs`). Son
/// valiosas para la mente de AION, pero a Claude Code (tareas de código) no le aportan contexto
/// accionable y, por su verbosidad, copan el brief. Se LIMITA su número (no se excluyen): el
/// recuerdo permanece en memoria y sigue siendo recuperable por `aion_memory_search`.
fn is_self_reflection(content: &str) -> bool {
    let c = content.trim_start().to_lowercase();
    // Prefijos que el loop de vida interior autogenera (ver `main.rs`): síntesis, auto-crítica de
    // desempeño, aprendizaje de errores y consolidación. NO incluye `[idea]` (puede ser una idea
    // de negocio real de Ariel) ni `[hecho]`/`[proyecto:]`/`[decisión]` (contexto accionable).
    c.starts_with("[insight]")
        || c.starts_with("[conocimiento]")
        || c.starts_with("[reflexión]")
        || c.starts_with("[reflexion]")
        || c.starts_with("[aprendizaje]")
        || c.starts_with("[consolidación]")
        || c.starts_with("[consolidacion]")
}

/// ¿Es un nombre de PRUEBA/fixture de desarrollo (proyecto o dominio de biblioteca)?
/// `TEST_*`, `itest` — ruido de desarrollo que no es contexto real del usuario.
fn is_test_fixture(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    n.starts_with("test") || n.starts_with("itest")
}

/// ¿Son `a` y `b` casi el mismo texto? Jaccard de palabras significativas (>3 chars)
/// ≥ 0.6 — barato y suficiente para descartar variantes del mismo recuerdo en el brief.
fn near_duplicate(a: &str, b: &str) -> bool {
    fn words(s: &str) -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|w| w.to_string())
            .collect()
    }
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return false;
    }
    let inter = wa.intersection(&wb).count() as f32;
    let union = wa.union(&wb).count() as f32;
    // 0.72: lo bastante alto para no fundir recuerdos que solo comparten vocabulario
    // común (y difieren en tokens cortos significativos: siglas, A/B, versiones).
    union > 0.0 && inter / union >= 0.72
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    fn allow_list(root: &serde_json::Value) -> Vec<String> {
        root["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn apply_on_empty_adds_tools_and_hook() {
        let mut root = serde_json::json!({});
        apply_aion_settings(&mut root).unwrap();
        let allow = allow_list(&root);
        for tool in AION_MCP_TOOLS {
            assert!(allow.contains(&tool.to_string()), "falta {tool}");
        }
        let ups = root["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
        assert!(ups.iter().any(group_has_aion_hook));
    }

    #[test]
    fn apply_is_idempotent() {
        let mut root = serde_json::json!({});
        apply_aion_settings(&mut root).unwrap();
        apply_aion_settings(&mut root).unwrap();
        apply_aion_settings(&mut root).unwrap();
        // Sin duplicados: exactamente las 8 tools y 1 grupo de hook.
        assert_eq!(allow_list(&root).len(), AION_MCP_TOOLS.len());
        assert_eq!(
            root["hooks"]["UserPromptSubmit"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn apply_preserves_existing_user_config() {
        // Usuario con su propio permiso y su propio hook UserPromptSubmit.
        let mut root = serde_json::json!({
            "permissions": { "allow": ["Bash(git *)"] },
            "hooks": {
                "UserPromptSubmit": [
                    { "hooks": [{ "type": "command", "command": "echo mio" }] }
                ]
            },
            "theme": "dark"
        });
        apply_aion_settings(&mut root).unwrap();
        let allow = allow_list(&root);
        assert!(
            allow.contains(&"Bash(git *)".to_string()),
            "se perdió permiso del usuario"
        );
        assert!(allow.contains(&"mcp__aion__aion_brief".to_string()));
        // El hook del usuario sigue + el de AION añadido = 2 grupos.
        let ups = root["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 2);
        assert_eq!(root["theme"], "dark", "se perdió un ajuste no relacionado");
    }

    #[test]
    fn remove_undoes_apply_but_keeps_user_config() {
        let mut root = serde_json::json!({
            "permissions": { "allow": ["Bash(git *)"] },
            "hooks": {
                "UserPromptSubmit": [
                    { "hooks": [{ "type": "command", "command": "echo mio" }] }
                ]
            }
        });
        apply_aion_settings(&mut root).unwrap();
        remove_aion_settings(&mut root);
        let allow = allow_list(&root);
        assert_eq!(
            allow,
            vec!["Bash(git *)".to_string()],
            "debe quedar solo lo del usuario"
        );
        let ups = root["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
        assert!(
            !ups.iter().any(group_has_aion_hook),
            "el hook de AION no se quitó"
        );
    }

    #[test]
    fn apply_rejects_incompatible_permissions_type() {
        // permissions con tipo incompatible → Err, no panic, no pisa la config del usuario.
        let mut root = serde_json::json!({ "permissions": "oops" });
        assert!(apply_aion_settings(&mut root).is_err());
    }
}
