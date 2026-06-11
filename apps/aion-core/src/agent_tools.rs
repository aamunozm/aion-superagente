//! Herramientas que dan al **agente conversacional** sus verdaderas capacidades:
//! memoria cognitiva (recordar), **auto-escritura de skills** (se escribe código a
//! sí mismo, validado en sandbox+tests) e invocación de las skills que ha forjado.
//! Esto unifica los "órganos" de AION dentro del agente con el que hablas.

use aion_browser::{BrowserDriver, WebClient};
use aion_evolution::{Candidate, EvolutionEngine};
use aion_kernel::traits::{GenerateRequest, LlmEngine, MemoryStore, SkillHost};
use aion_kernel::types::Message;
use aion_llm::OllamaEngine;
use aion_memory::VectorMemory;
use aion_orchestrator::Tool;
use aion_skills::{SkillManifest, WasmSkillHost};
use async_trait::async_trait;
use std::sync::Arc;

// ── Archivos: listar/contar en carpetas del usuario (solo lectura, gobernado) ─

/// Permite al agente LISTAR y CONTAR archivos de una carpeta del usuario (p. ej.
/// "cuántos PDF hay en el Escritorio"). Solo lectura y restringido a la carpeta de
/// usuario (HOME) por seguridad. Resuelve alias en español.
pub struct FilesTool;

impl FilesTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FilesTool {
    fn name(&self) -> &str {
        "files_list"
    }
    fn description(&self) -> &str {
        "Lista y cuenta archivos de una carpeta del usuario (solo lectura). Entrada: \
         \"carpeta [extensión]\", p. ej. \"escritorio pdf\", \"documentos\", \"descargas png\". \
         Carpetas válidas: escritorio, documentos, descargas, imágenes, inicio (o una ruta)."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let home =
            std::env::var("HOME").map_err(|_| "no encuentro tu carpeta de usuario".to_string())?;
        let home = std::path::PathBuf::from(home);
        let mut it = input.split_whitespace();
        let folder = it.next().unwrap_or("").to_lowercase();
        let ext = it.next().map(|s| s.trim_start_matches('.').to_lowercase());

        // Resuelve alias en español/inglés a rutas dentro de HOME.
        let dir = match folder.as_str() {
            "escritorio" | "desktop" => home.join("Desktop"),
            "documentos" | "documents" => home.join("Documents"),
            "descargas" | "downloads" => home.join("Downloads"),
            "imágenes" | "imagenes" | "pictures" | "fotos" => home.join("Pictures"),
            "inicio" | "home" | "~" => home.clone(),
            other => {
                // Ruta literal: solo se permite dentro de HOME.
                let p = if let Some(rest) = other.strip_prefix("~/") {
                    home.join(rest)
                } else {
                    std::path::PathBuf::from(other)
                };
                p
            }
        };
        // Seguridad: la carpeta debe estar dentro de HOME.
        let canon = dir
            .canonicalize()
            .map_err(|_| format!("no encuentro la carpeta «{folder}»"))?;
        if !canon.starts_with(&home) {
            return Err("por seguridad solo puedo leer dentro de tu carpeta de usuario".into());
        }

        let entries =
            std::fs::read_dir(&canon).map_err(|e| format!("no pude leer la carpeta: {e}"))?;
        let mut names: Vec<String> = Vec::new();
        for e in entries.flatten() {
            if !e.path().is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue; // ocultos
            }
            if let Some(ref x) = ext {
                if !name.to_lowercase().ends_with(&format!(".{x}")) {
                    continue;
                }
            }
            names.push(name);
        }
        names.sort();
        let total = names.len();
        let label = match &ext {
            Some(x) => format!("archivos .{x}"),
            None => "archivos".into(),
        };
        let sample = names
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        Ok(format!(
            "{total} {label} en {}{}",
            folder,
            if sample.is_empty() {
                String::new()
            } else {
                format!(": {sample}")
            }
        ))
    }
}

// ── Red local: descubrir equipos conectados y sus IPs (solo lectura) ────────

/// Escanea la **red local** (LAN) y devuelve los equipos conectados con su IP y
/// MAC. Es real: hace un barrido de ping de la subred para poblar la tabla ARP y
/// luego la lee (`arp -a`). Solo lectura, no modifica nada de la red.
pub struct NetTool;

impl NetTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NetTool {
    fn default() -> Self {
        Self::new()
    }
}

/// IP IPv4 de la interfaz principal (macOS: en0/en1; Linux: vía `hostname -I`).
async fn local_ipv4() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        for iface in ["en0", "en1", "en2"] {
            if let Ok(out) = tokio::process::Command::new("ipconfig")
                .args(["getifaddr", iface])
                .output()
                .await
            {
                let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !ip.is_empty() {
                    return Some(ip);
                }
            }
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        let out = tokio::process::Command::new("hostname")
            .arg("-I")
            .output()
            .await
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .find(|s| s.contains('.'))
            .map(|s| s.to_string())
    }
}

#[async_trait]
impl Tool for NetTool {
    fn name(&self) -> &str {
        "net_scan"
    }
    fn description(&self) -> &str {
        "Escanea TU red local y devuelve los equipos conectados con su IP y MAC \
         (cuántos hay y cuáles son). Úsala para «cuántos equipos hay en la red», \
         «qué dispositivos están conectados», «sus IPs». No necesita entrada."
    }
    async fn run(&self, _input: &str) -> Result<String, String> {
        let my_ip = local_ipv4()
            .await
            .ok_or_else(|| "no detecté tu IP local (¿estás conectado a una red?)".to_string())?;
        // Prefijo /24 (los tres primeros octetos) para barrer la subred.
        let prefix = {
            let mut it = my_ip.rsplitn(2, '.');
            let _last = it.next();
            it.next().map(|p| p.to_string())
        }
        .ok_or_else(|| format!("IP local con formato inesperado: {my_ip}"))?;

        // Barrido de ping en paralelo (.1–.254) para poblar la tabla ARP. Timeout
        // corto por host; el conjunto termina en ~1–2 s.
        let mut pings = Vec::new();
        for host in 1u8..=254 {
            let ip = format!("{prefix}.{host}");
            pings.push(tokio::spawn(async move {
                #[cfg(target_os = "macos")]
                let args = ["-c", "1", "-t", "1", &ip];
                #[cfg(not(target_os = "macos"))]
                let args = ["-c", "1", "-W", "1", &ip];
                let _ = tokio::process::Command::new("ping")
                    .args(args)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await;
            }));
        }
        for p in pings {
            let _ = p.await;
        }

        // Lee la tabla ARP poblada.
        let out = tokio::process::Command::new("arp")
            .arg("-a")
            .output()
            .await
            .map_err(|e| format!("no pude leer la tabla ARP: {e}"))?;
        let table = String::from_utf8_lossy(&out.stdout);

        // Parse: "host (192.168.1.5) at aa:bb:.. on en0 ..." — nos quedamos con los
        // de NUESTRA subred y descartamos entradas incompletas.
        let mut devices: Vec<(String, String, String)> = Vec::new(); // (ip, mac, host)
        for line in table.lines() {
            let Some(open) = line.find('(') else { continue };
            let Some(close) = line.find(')') else {
                continue;
            };
            let ip = line[open + 1..close].trim().to_string();
            if !ip.starts_with(&format!("{prefix}.")) {
                continue;
            }
            // Descarta red (.0), broadcast (.255) y multicast — no son equipos.
            let last: u8 = ip
                .rsplit('.')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if last == 0 || last == 255 {
                continue;
            }
            let mac = line
                .split(" at ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("")
                .to_string();
            if mac.is_empty() || mac.contains("incomplete") {
                continue;
            }
            let host = line[..open].trim().trim_end_matches('?').trim().to_string();
            if devices.iter().any(|(i, _, _)| i == &ip) {
                continue;
            }
            devices.push((ip, mac, host));
        }

        // Incluye este equipo (puede no salir en su propia tabla ARP).
        if !devices.iter().any(|(i, _, _)| i == &my_ip) {
            devices.push((my_ip.clone(), "—".into(), "este equipo".into()));
        }
        devices.sort_by(|a, b| {
            let pa: u8 =
                a.0.rsplit('.')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
            let pb: u8 =
                b.0.rsplit('.')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
            pa.cmp(&pb)
        });

        if devices.is_empty() {
            return Ok("no detecté equipos activos en la red (tabla ARP vacía).".into());
        }
        let list = devices
            .iter()
            .map(|(ip, mac, host)| {
                let tag = if ip == &my_ip { " (este equipo)" } else { "" };
                let h = if host.is_empty() {
                    String::new()
                } else {
                    format!(" — {host}")
                };
                format!("{ip} [{mac}]{h}{tag}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!(
            "{} equipos conectados en la red {prefix}.0/24:\n{list}",
            devices.len()
        ))
    }
}

// ── Leer un archivo de texto del usuario (solo lectura, gobernado) ──────────

/// Lee el contenido de un archivo de texto del usuario (memoria tipo filesystem:
/// Letta 2025 mostró que dar al agente grep/leer ficheros iguala o supera a memorias
/// especializadas). Solo lectura, restringido a HOME, con tope de tamaño.
pub struct FileReadTool;

impl FileReadTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }
    fn description(&self) -> &str {
        "Lee el contenido de un archivo de texto del usuario. Entrada: la ruta \
         (admite ~). Solo lectura, dentro de tu carpeta de usuario. Útil para leer \
         notas, código o documentos antes de responder."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let home =
            std::env::var("HOME").map_err(|_| "no encuentro tu carpeta de usuario".to_string())?;
        let home = std::path::PathBuf::from(&home);
        let raw = input.trim();
        if raw.is_empty() {
            return Err("indica la ruta del archivo".into());
        }
        let p = if let Some(rest) = raw.strip_prefix("~/") {
            home.join(rest)
        } else {
            std::path::PathBuf::from(raw)
        };
        let canon = p
            .canonicalize()
            .map_err(|_| format!("no encuentro el archivo «{raw}»"))?;
        if !canon.starts_with(&home) {
            return Err("por seguridad solo puedo leer dentro de tu carpeta de usuario".into());
        }
        if !canon.is_file() {
            return Err("eso no es un archivo".into());
        }
        let meta = std::fs::metadata(&canon).map_err(|e| e.to_string())?;
        if meta.len() > 200_000 {
            return Err("el archivo es demasiado grande (>200 KB) para leerlo de una vez".into());
        }
        let content = std::fs::read_to_string(&canon)
            .map_err(|_| "no pude leerlo (¿es binario?)".to_string())?;
        let mut out = content;
        out.truncate(8000); // tope de contexto
        Ok(out)
    }
}

// ── Biblioteca (Academias): consultar documentos/libros ingeridos ──────────

/// Permite al agente CONSULTAR la biblioteca de conocimiento (libros, PDFs, notas
/// ingeridas). Multilingüe: recupera pasajes relevantes aunque estén en otro idioma,
/// con cita (dominio · fuente · fragmento). Así el agente fundamenta en TUS documentos.
pub struct LibrarySearchTool;

impl LibrarySearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LibrarySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for LibrarySearchTool {
    fn name(&self) -> &str {
        "library_search"
    }
    fn description(&self) -> &str {
        "Busca en TU biblioteca de documentos/libros ingeridos (Academias) y devuelve \
         pasajes relevantes con su fuente, para fundamentar la respuesta. Entrada: la \
         consulta (en cualquier idioma). Úsala cuando la pregunta sea sobre el contenido \
         de libros, PDFs o notas que el usuario ha cargado."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let lib = crate::library::Library::open(crate::knowledge_path());
        if lib.total_chunks() == 0 {
            return Ok("la biblioteca está vacía (aún no se han ingerido documentos)".into());
        }
        let hits = lib.search(input.trim(), 5, None).await?;
        if hits.is_empty() {
            return Ok("(sin pasajes relevantes en la biblioteca)".into());
        }
        Ok(hits
            .iter()
            .map(|p| {
                format!(
                    "• [{} · {} · frag.{}] {}",
                    p.domain,
                    p.source,
                    p.idx,
                    p.content.chars().take(400).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

// ── Computer-use: ver la PANTALLA y controlar ratón/teclado (gobernado + HITL) ─

/// Directorio de gobernanza del control (Governor/audit).
fn control_dir() -> std::path::PathBuf {
    crate::app_data_dir().join("control")
}

/// VE la pantalla del escritorio (captura) y la describe con el modelo de visión.
/// Lectura, bajo el Governor. Para asistirte mirando lo que hay en tu Mac.
pub struct ScreenSeeTool;
impl ScreenSeeTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for ScreenSeeTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for ScreenSeeTool {
    fn name(&self) -> &str {
        "screen_see"
    }
    fn description(&self) -> &str {
        "MIRA la pantalla del escritorio (todo tu Mac, no solo el navegador) y la \
         describe. Úsalo para ver qué hay en pantalla y asistir. Entrada opcional: qué \
         quieres saber. Requiere permiso de Grabación de pantalla."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        // Captura (bloqueante) en hilo aparte; pasa por el Governor (look()).
        let b64 = tokio::task::spawn_blocking(|| {
            aion_control::Computer::open(control_dir())
                .map_err(|e| e.to_string())
                .and_then(|c| c.look().map_err(|e| e.to_string()))
        })
        .await
        .map_err(|e| e.to_string())??;
        let prompt = if input.trim().is_empty() {
            "Describe lo que ves en la pantalla del escritorio."
        } else {
            input.trim()
        };
        let model = std::env::var("AION_VISION_MODEL")
            .unwrap_or_else(|_| "huihui_ai/gemma-4-abliterated:12b".into());
        let engine = OllamaEngine::new(OllamaEngine::base_url_from_env(), model);
        engine
            .generate_with_image(prompt, &b64)
            .await
            .map(|m| m.content)
            .map_err(|e| e.to_string())
    }
}

/// Lista elementos interactivos (botones) de la ventana en primer plano con su
/// posición central (a11y de macOS vía System Events). El agente luego usa pc_click
/// con esas coordenadas. Best-effort (solo botones de la ventana frontal).
pub struct ScreenElementsTool;
impl ScreenElementsTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for ScreenElementsTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for ScreenElementsTool {
    fn name(&self) -> &str {
        "screen_elements"
    }
    fn description(&self) -> &str {
        "Lista TODOS los elementos interactivos (botones, campos, casillas, menús, \
         enlaces…) de la ventana en primer plano con su posición central (x,y), usando \
         la accesibilidad de macOS. El SO da las coordenadas exactas (sin adivinar). \
         Úsalo para saber DÓNDE hacer clic; luego pc_click con esas coordenadas. \
         Requiere permiso de Accesibilidad."
    }
    async fn run(&self, _input: &str) -> Result<String, String> {
        // 1) NATIVO primero: API de accesibilidad del SO (macOS AX / Windows UIA).
        //    Rápido y desbloquea Electron (AXManualAccessibility). Si da resultados,
        //    los usamos; si no, caemos al AppleScript (solo macOS).
        let native = tokio::task::spawn_blocking(|| aion_control::ui_tree::frontmost_elements(60))
            .await
            .unwrap_or_default();
        if !native.is_empty() {
            let list = native
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let name = if e.name.is_empty() {
                        "(sin etiqueta)"
                    } else {
                        &e.name
                    };
                    format!("[{}] {} «{name}» en ({}, {})", i + 1, e.role, e.x, e.y)
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(format!(
                "Elementos interactivos de la ventana frontal ({}, vía accesibilidad nativa):\n{list}",
                native.len()
            ));
        }

        // 2) FALLBACK (macOS): recorre el árbol vía AppleScript/System Events.
        // Recorre el árbol de accesibilidad RECURSIVAMENTE por `UI elements` (más
        // fiable que `entire contents`, que falla en varias apps) y se queda con los
        // roles interactivos + su posición central. El grounding lo da el SO: cero
        // coste de RAM, funciona con cualquier modelo.
        let script = r#"on collectEls(el, depth)
  set acc to ""
  if depth > 5 then return acc
  tell application "System Events"
    set roleset to {"AXButton","AXTextField","AXTextArea","AXCheckBox","AXRadioButton","AXPopUpButton","AXMenuButton","AXComboBox","AXLink","AXTabButton","AXDisclosureTriangle"}
    try
      repeat with e in (UI elements of el)
        try
          set rc to role of e
          if rc is in roleset then
            set p to position of e
            set s to size of e
            if (item 1 of s) > 0 and (item 2 of s) > 0 then
              set nm to ""
              try
                set nm to name of e
              end try
              if nm is missing value or nm is "" then
                try
                  set nm to (value of e) as string
                end try
              end if
              if nm is missing value then set nm to ""
              set cx to (item 1 of p) + ((item 1 of s) / 2)
              set cy to (item 2 of p) + ((item 2 of s) / 2)
              set acc to acc & rc & "|" & nm & "|" & (cx as integer) & "|" & (cy as integer) & linefeed
            end if
          end if
        end try
        set acc to acc & my collectEls(e, depth + 1)
      end repeat
    end try
  end tell
  return acc
end collectEls

tell application "System Events"
  set w to window 1 of (first application process whose frontmost is true)
end tell
return my collectEls(w, 0)"#;
        let out = tokio::task::spawn_blocking(move || {
            std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output()
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("no pude leer la accesibilidad: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut items = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() == 4 {
                // rol legible (quita el prefijo AX) + etiqueta + coordenadas.
                let kind = parts[0].trim_start_matches("AX").to_lowercase();
                let name = if parts[1].trim().is_empty() {
                    "(sin etiqueta)"
                } else {
                    parts[1].trim()
                };
                items.push(format!(
                    "[{}] {kind} «{name}» en ({}, {})",
                    items.len() + 1,
                    parts[2],
                    parts[3]
                ));
            }
            if items.len() >= 60 {
                break; // tope para no saturar el contexto
            }
        }
        if items.is_empty() {
            return Ok(
                "(no detecté elementos accesibles en la ventana frontal; quizá la app no \
                 expone accesibilidad o falta el permiso de Accesibilidad. Usa screen_see \
                 para mirar la pantalla en su lugar)"
                    .into(),
            );
        }
        Ok(format!(
            "Elementos interactivos de la ventana frontal ({}):\n{}",
            items.len(),
            items.join("\n")
        ))
    }
}

/// Ejecuta una intención de control (clic/teclear/tecla) vía el Computer gobernado.
/// Cada acción pasa por confirmación HITL (needs_confirm) antes de ejecutarse.
fn run_control(intent: aion_control::ControlIntent) -> Result<String, String> {
    let mut comp = aion_control::Computer::open(control_dir()).map_err(|e| e.to_string())?;
    comp.dry_run = false; // acción REAL (ya aprobada por el usuario vía HITL)
    match comp.execute_confirmed(intent) {
        aion_control::ControlOutcome::Executed { summary, .. } => Ok(format!("hecho: {summary}")),
        aion_control::ControlOutcome::Denied { reason } => Err(format!("denegado: {reason}")),
        aion_control::ControlOutcome::NeedsConfirmation { reason, .. } => {
            Err(format!("requiere confirmación: {reason}"))
        }
        aion_control::ControlOutcome::Failed { error } => Err(format!("falló: {error}")),
    }
}

/// Clic del ratón en coordenadas de pantalla. Entrada: "x y".
pub struct PcClickTool;
impl PcClickTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for PcClickTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for PcClickTool {
    fn name(&self) -> &str {
        "pc_click"
    }
    fn description(&self) -> &str {
        "Hace clic del ratón en una posición de la PANTALLA del escritorio. Entrada: \
         \"x y\" (coordenadas, de screen_elements). Requiere tu confirmación."
    }
    fn needs_confirm(&self, input: &str) -> Option<String> {
        Some(format!("Hacer clic en la pantalla en ({})", input.trim()))
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let mut it = input.split_whitespace();
        let x: i32 = it
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or("uso: \"x y\"")?;
        let y: i32 = it
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or("uso: \"x y\"")?;
        tokio::task::spawn_blocking(move || {
            run_control(aion_control::ControlIntent::Click { x, y })
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// Escribe texto con el teclado en la app en primer plano. Entrada: el texto.
pub struct PcTypeTool;
impl PcTypeTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for PcTypeTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for PcTypeTool {
    fn name(&self) -> &str {
        "pc_type"
    }
    fn description(&self) -> &str {
        "Escribe texto con el teclado en la app en primer plano. Entrada: el texto. \
         Requiere tu confirmación."
    }
    fn needs_confirm(&self, input: &str) -> Option<String> {
        Some(format!("Escribir con el teclado: «{}»", input.trim()))
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let text = input.trim().to_string();
        if text.is_empty() {
            return Err("nada que escribir".into());
        }
        tokio::task::spawn_blocking(move || run_control(aion_control::ControlIntent::Type { text }))
            .await
            .map_err(|e| e.to_string())?
    }
}

/// Pulsa una tecla especial (enter, tab, esc, cmd+c…). Entrada: el nombre de la tecla.
pub struct PcKeyTool;
impl PcKeyTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for PcKeyTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for PcKeyTool {
    fn name(&self) -> &str {
        "pc_key"
    }
    fn description(&self) -> &str {
        "Pulsa una tecla (p. ej. \"enter\", \"tab\", \"esc\"). Entrada: el nombre. \
         Requiere tu confirmación."
    }
    fn needs_confirm(&self, input: &str) -> Option<String> {
        Some(format!("Pulsar la tecla: {}", input.trim()))
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let name = input.trim().to_string();
        if name.is_empty() {
            return Err("indica la tecla".into());
        }
        tokio::task::spawn_blocking(move || run_control(aion_control::ControlIntent::Key { name }))
            .await
            .map_err(|e| e.to_string())?
    }
}

// ── Crear documentos (robusto: archivo + abrir, sin tocar el teclado) ────────

/// Crea un DOCUMENTO de texto en el Mac: lo escribe en un archivo del Escritorio y
/// lo ABRE (p. ej. en TextEdit). Es la forma robusta de "hacer un documento": el
/// agente redacta el contenido y se guarda; NO necesita ver la pantalla ni simular
/// el teclado (cero permisos de Grabación de pantalla/Accesibilidad).
pub struct MakeDocumentTool;
impl MakeDocumentTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for MakeDocumentTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for MakeDocumentTool {
    fn name(&self) -> &str {
        "make_document"
    }
    fn description(&self) -> &str {
        "Crea un DOCUMENTO en el Mac y lo ABRE. Úsalo para «hazme/escribe un documento, \
         carta, informe… sobre X», también «en PDF» o «en Word/Pages». Entrada: \
         «Título ::: contenido completo ::: formato». Formatos: txt (def.), md, rtf, docx \
         (Word/Pages), pdf. TÚ redactas el contenido entero. No necesita permisos de \
         pantalla ni de teclado."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let parts: Vec<&str> = input.splitn(3, ":::").collect();
        let (title, body, fmt_raw) = if parts.len() == 1 {
            ("Documento", input.trim().to_string(), "")
        } else {
            (
                parts[0].trim(),
                parts[1].trim().to_string(),
                parts.get(2).map(|s| s.trim()).unwrap_or(""),
            )
        };
        if body.is_empty() {
            return Err("falta el contenido del documento (usa «Título ::: contenido»)".into());
        }
        let fmt = match fmt_raw.to_lowercase().as_str() {
            "md" | "markdown" => "md",
            "rtf" => "rtf",
            "docx" | "word" | "pages" => "docx",
            "pdf" => "pdf",
            _ => "txt",
        };
        let home =
            std::env::var("HOME").map_err(|_| "no encuentro tu carpeta de usuario".to_string())?;
        let safe: String = title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let name: String = {
            let t = safe.trim().trim_matches(|c| c == '_' || c == ' ');
            if t.is_empty() {
                "Documento".to_string()
            } else {
                t.chars().take(60).collect()
            }
        };
        let desktop = std::path::Path::new(&home).join("Desktop");
        let path = desktop.join(format!("{name}.{fmt}"));

        // Formatos de texto plano: escritura directa.
        if fmt == "txt" || fmt == "md" {
            std::fs::write(&path, &body)
                .map_err(|e| format!("no pude escribir el documento: {e}"))?;
            open_file(&path, fmt == "txt");
            return Ok(format!(
                "documento creado y abierto en el Escritorio: {}",
                path.display()
            ));
        }

        // Formatos enriquecidos (rtf/docx/pdf): se generan con herramientas del SO.
        #[cfg(target_os = "macos")]
        {
            let tmp = std::env::temp_dir().join(format!("aion_doc_{}.txt", uuid::Uuid::new_v4()));
            std::fs::write(&tmp, &body).map_err(|e| format!("no pude preparar el texto: {e}"))?;
            let ok = match fmt {
                "pdf" => match std::fs::File::create(&path) {
                    Ok(f) => std::process::Command::new("cupsfilter")
                        .arg(&tmp)
                        .stdout(std::process::Stdio::from(f))
                        .stderr(std::process::Stdio::null())
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false),
                    Err(_) => false,
                },
                // textutil crea RTF/DOCX REALES (abren en Word/Pages/TextEdit).
                _ => std::process::Command::new("textutil")
                    .args(["-convert", fmt, "-output"])
                    .arg(&path)
                    .arg(&tmp)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false),
            };
            let _ = std::fs::remove_file(&tmp);
            if !ok {
                return Err(format!("no pude generar el documento en {fmt}"));
            }
            open_file(&path, false);
            Ok(format!(
                "documento {fmt} creado y abierto en el Escritorio: {}",
                path.display()
            ))
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Otros SO: sin conversor nativo, guardamos como .txt (no se pierde el contenido).
            let txt = desktop.join(format!("{name}.txt"));
            std::fs::write(&txt, &body)
                .map_err(|e| format!("no pude escribir el documento: {e}"))?;
            open_file(&txt, true);
            Ok(format!(
                "guardado como .txt en el Escritorio (el formato {fmt} solo está disponible en macOS): {}",
                txt.display()
            ))
        }
    }
}

/// Abre un archivo en el Mac (TextEdit para texto plano, app por defecto para el resto).
fn open_file(path: &std::path::Path, text_in_textedit: bool) {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if text_in_textedit {
            cmd.arg("-a").arg("TextEdit");
        }
        let _ = cmd.arg(path).status();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = text_in_textedit;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .status();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (path, text_in_textedit);
    }
}

/// Crea una NOTA en la app Notas (Apple Notes) con el contenido que redacta el agente.
/// Requiere el permiso (una vez) de Automatización para «Notes». Sin tecleo simulado.
pub struct MakeNoteTool;
impl MakeNoteTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for MakeNoteTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for MakeNoteTool {
    fn name(&self) -> &str {
        "make_note"
    }
    fn description(&self) -> &str {
        "Crea una NOTA en la app Notas del Mac. Úsalo para «créame/guarda una nota sobre X». \
         Entrada: «Título ::: contenido». TÚ redactas el contenido. (Pide una vez permiso de \
         Automatización para Notas.)"
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let (title, body) = match input.split_once(":::") {
            Some((t, b)) => (t.trim().to_string(), b.trim().to_string()),
            None => ("Nota".to_string(), input.trim().to_string()),
        };
        if body.is_empty() {
            return Err("falta el contenido de la nota (usa «Título ::: contenido»)".into());
        }
        #[cfg(target_os = "macos")]
        {
            // El cuerpo de Notas es HTML: saltos de línea → <br>, y escapamos comillas.
            let esc = |s: &str| {
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "<br>")
            };
            let html = format!("<div><b>{}</b></div>{}", esc(&title), esc(&body));
            let script = format!(
                "tell application \"Notes\" to make new note with properties {{body:\"{html}\"}}"
            );
            let out = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
                .map_err(|e| format!("no pude crear la nota: {e}"))?;
            if out.status.success() {
                Ok(format!("nota «{title}» creada en la app Notas"))
            } else {
                Err(format!(
                    "no pude crear la nota (¿falta permiso de Automatización para Notas? \
                     actívalo en Ajustes del Sistema → Privacidad y seguridad → Automatización → AION → Notas): {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (title, body);
            Err("la app Notas solo está disponible en macOS; usa make_document".into())
        }
    }
}

// ── Navegador agéntico real (Chrome headless vía CDP) ───────────────────────

/// Formatea una instantánea de accesibilidad para el LLM: texto visible + lista de
/// elementos interactivos NUMERADOS (el agente actúa por número, no por selector CSS).
fn format_snapshot(s: &aion_browser::Snapshot) -> String {
    let mut out = format!("[{}] {}\n{}\n", s.view.title, s.view.url, s.view.text);
    if s.elements.is_empty() {
        out.push_str("\n(sin elementos interactivos detectados)");
        return out;
    }
    out.push_str("\nElementos interactivos (usa el número con browser_click / browser_type):\n");
    for e in &s.elements {
        let name = if e.name.is_empty() {
            "(sin etiqueta)"
        } else {
            &e.name
        };
        out.push_str(&format!("[{}] {} «{}»\n", e.ref_id, e.kind, name));
    }
    out
}

/// Convierte la entrada del agente (un NÚMERO de ref, o un selector CSS) en un
/// selector usable por el driver. Un número → [data-aion-ref="N"].
fn resolve_target(input: &str) -> String {
    let t = input.trim();
    if t.chars().all(|c| c.is_ascii_digit()) && !t.is_empty() {
        format!("[data-aion-ref=\"{t}\"]")
    } else {
        t.to_string()
    }
}

/// Abre una URL en un navegador REAL (ejecuta JS) y devuelve texto + elementos
/// interactivos numerados (snapshot de accesibilidad).
pub struct BrowserOpenTool {
    driver: Arc<dyn BrowserDriver>,
}
impl BrowserOpenTool {
    pub fn new(driver: Arc<dyn BrowserDriver>) -> Self {
        Self { driver }
    }
}
#[async_trait]
impl Tool for BrowserOpenTool {
    fn name(&self) -> &str {
        "browser_open"
    }
    fn description(&self) -> &str {
        "Abre una URL en un navegador REAL (con JavaScript) y devuelve el texto visible \
         MÁS la lista de elementos interactivos numerados. Úsalo para webs dinámicas o \
         para interactuar (luego browser_click / browser_type usando esos números). \
         Entrada: la URL."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        self.driver
            .open(input.trim())
            .await
            .map_err(|e| e.to_string())?;
        let s = self.driver.snapshot().await.map_err(|e| e.to_string())?;
        Ok(format_snapshot(&s))
    }
}

/// Re-lee la página + sus elementos interactivos (tras un clic o carga dinámica).
pub struct BrowserReadTool {
    driver: Arc<dyn BrowserDriver>,
}
impl BrowserReadTool {
    pub fn new(driver: Arc<dyn BrowserDriver>) -> Self {
        Self { driver }
    }
}
#[async_trait]
impl Tool for BrowserReadTool {
    fn name(&self) -> &str {
        "browser_read"
    }
    fn description(&self) -> &str {
        "Vuelve a leer la página abierta: texto visible + elementos interactivos \
         numerados (úsalo tras browser_click o cuando la página cargó más contenido)."
    }
    async fn run(&self, _input: &str) -> Result<String, String> {
        let s = self.driver.snapshot().await.map_err(|e| e.to_string())?;
        Ok(format_snapshot(&s))
    }
}

/// Hace clic en un elemento por NÚMERO (del snapshot) o selector CSS.
pub struct BrowserClickTool {
    driver: Arc<dyn BrowserDriver>,
}
impl BrowserClickTool {
    pub fn new(driver: Arc<dyn BrowserDriver>) -> Self {
        Self { driver }
    }
}
#[async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str {
        "browser_click"
    }
    fn description(&self) -> &str {
        "Hace clic en un elemento de la página abierta. Entrada: el NÚMERO del elemento \
         (de la lista que dio browser_open/browser_read), p. ej. \"3\"; o un selector CSS."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        self.driver
            .click(&resolve_target(input))
            .await
            .map_err(|e| e.to_string())?;
        let s = self.driver.snapshot().await.map_err(|e| e.to_string())?;
        Ok(format!("clic hecho.\n{}", format_snapshot(&s)))
    }
}

/// Escribe texto en un campo por NÚMERO (del snapshot) o selector CSS.
pub struct BrowserTypeTool {
    driver: Arc<dyn BrowserDriver>,
}
impl BrowserTypeTool {
    pub fn new(driver: Arc<dyn BrowserDriver>) -> Self {
        Self { driver }
    }
}
#[async_trait]
impl Tool for BrowserTypeTool {
    fn name(&self) -> &str {
        "browser_type"
    }
    fn description(&self) -> &str {
        "Escribe texto en un campo de la página abierta. Entrada: \"objetivo ::: texto\", \
         donde objetivo es el NÚMERO del campo (del snapshot) o un selector CSS. \
         P. ej. \"3 ::: gemma 12B\" o \"#email ::: ariel@ejemplo.com\"."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let (target, text) = input.split_once(":::").ok_or_else(|| {
            "usa el formato \"objetivo ::: texto\" (objetivo = número o selector)".to_string()
        })?;
        self.driver
            .type_text(&resolve_target(target), text.trim())
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "escrito «{}» en el objetivo {}",
            text.trim(),
            target.trim()
        ))
    }
}

/// VISIÓN SELECTIVA: captura la página actual y la describe con el modelo multimodal
/// (para cuando el texto/snapshot no basta: gráficos, captchas visuales, layout).
pub struct BrowserSeeTool {
    driver: Arc<dyn BrowserDriver>,
}
impl BrowserSeeTool {
    pub fn new(driver: Arc<dyn BrowserDriver>) -> Self {
        Self { driver }
    }
}
#[async_trait]
impl Tool for BrowserSeeTool {
    fn name(&self) -> &str {
        "browser_see"
    }
    fn description(&self) -> &str {
        "MIRA la página abierta como una imagen (visión) y la describe. Úsalo cuando el \
         texto no baste: gráficos, diagramas, disposición visual, elementos sin etiqueta. \
         Entrada opcional: qué quieres saber de la imagen."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let b64 = self
            .driver
            .screenshot_b64()
            .await
            .map_err(|e| e.to_string())?;
        let prompt = if input.trim().is_empty() {
            "Describe lo que se ve en esta página web."
        } else {
            input.trim()
        };
        let model = std::env::var("AION_VISION_MODEL")
            .unwrap_or_else(|_| "huihui_ai/gemma-4-abliterated:12b".into());
        let engine = OllamaEngine::new(OllamaEngine::base_url_from_env(), model);
        engine
            .generate_with_image(prompt, &b64)
            .await
            .map(|m| m.content)
            .map_err(|e| e.to_string())
    }
}

/// Inicia sesión en la página abierta usando las credenciales GUARDADAS del usuario
/// (bóveda en el Llavero). El agente NUNCA ve la contraseña: esta herramienta solo
/// rellena el formulario y confirma; los valores van directos al navegador.
pub struct CredentialLoginTool {
    driver: Arc<dyn BrowserDriver>,
}
impl CredentialLoginTool {
    pub fn new(driver: Arc<dyn BrowserDriver>) -> Self {
        Self { driver }
    }
}
#[async_trait]
impl Tool for CredentialLoginTool {
    fn name(&self) -> &str {
        "credential_login"
    }
    fn description(&self) -> &str {
        "Inicia sesión en la página ABIERTA con las credenciales GUARDADAS del usuario \
         para ese sitio. Entrada: el sitio/host (p. ej. \"amazon.it\"). Rellena usuario y \
         contraseña en el formulario; luego usa browser_click para enviar. NUNCA verás la \
         contraseña: solo se rellena. Si no hay credenciales guardadas, pídele al usuario \
         que las añada en Ajustes → Credenciales (NUNCA pidas la contraseña por el chat)."
    }
    fn needs_confirm(&self, input: &str) -> Option<String> {
        let host = crate::credentials::normalize_host(input);
        Some(format!(
            "Iniciar sesión en «{host}» con tus credenciales guardadas"
        ))
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let host = crate::credentials::normalize_host(input);
        // get() solo lo llama el backend; el valor jamás se devuelve al agente.
        let Some((user, pass)) = crate::credentials::get(&host) else {
            return Ok(format!(
                "no hay credenciales guardadas para «{host}». Pídele al usuario que las añada \
                 en Ajustes → Credenciales (no las pidas por el chat)."
            ));
        };
        let filled = self
            .driver
            .fill_login(&user, &pass)
            .await
            .map_err(|e| e.to_string())?;
        // Solo informamos de los campos rellenados, nunca de los valores.
        Ok(format!(
            "credenciales de «{host}» introducidas en el formulario (campos: {filled}). \
             Ahora pulsa el botón de iniciar sesión con browser_click."
        ))
    }
}

/// Puerta de CONFIRMACIÓN para acciones sensibles (comprar, pagar, enviar, borrar,
/// algo irreversible). El agente DEBE llamarla antes de hacerlas; el usuario aprueba
/// o rechaza en la UI. Si aprueba, el agente procede; si no, se detiene.
pub struct ConfirmActionTool;
impl ConfirmActionTool {
    pub fn new() -> Self {
        Self
    }
}
impl Default for ConfirmActionTool {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl Tool for ConfirmActionTool {
    fn name(&self) -> &str {
        "confirm_action"
    }
    fn description(&self) -> &str {
        "Pide al USUARIO confirmación antes de una acción sensible o irreversible \
         (comprar, pagar, enviar un formulario de pedido, borrar). Entrada: descripción \
         clara de la acción y su coste (p. ej. \"comprar «Libro X» por 19,90€ en Amazon\"). \
         Solo procede con la acción si esto devuelve «aprobado»."
    }
    fn needs_confirm(&self, input: &str) -> Option<String> {
        Some(input.trim().to_string())
    }
    async fn run(&self, _input: &str) -> Result<String, String> {
        // Si llegamos aquí, el usuario YA aprobó (el gate se evaluó antes de run).
        Ok("aprobado por el usuario: procede con la acción.".into())
    }
}

// ── Lugares/negocios por dirección: OpenStreetMap (Nominatim) ───────────────

/// Encuentra QUÉ negocio/lugar hay en una dirección (o busca lugares por nombre),
/// vía OpenStreetMap. Fiable para direcciones, a diferencia de la búsqueda web
/// general. Devuelve nombre, categoría (restaurante, tienda…) y dirección completa.
pub struct PlaceLookupTool {
    web: Arc<WebClient>,
}

impl PlaceLookupTool {
    pub fn new(web: Arc<WebClient>) -> Self {
        Self { web }
    }
}

#[async_trait]
impl Tool for PlaceLookupTool {
    fn name(&self) -> &str {
        "place_lookup"
    }
    fn description(&self) -> &str {
        "Averigua qué negocio/lugar hay en una DIRECCIÓN (o busca lugares por nombre) \
         usando mapas (OpenStreetMap). Úsala para «qué negocio está en tal calle», \
         «dónde queda X», tipo de local. Entrada: la dirección o el lugar. Más fiable \
         que web_search para direcciones."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let places = self
            .web
            .search_place(input.trim(), 5)
            .await
            .map_err(|e| e.to_string())?;
        if places.is_empty() {
            return Ok("(no encontré ningún lugar/negocio en esa dirección en el mapa)".into());
        }
        Ok(places
            .iter()
            .map(|p| {
                let name = if p.name.is_empty() {
                    "(sin nombre registrado)"
                } else {
                    &p.name
                };
                format!("• {name} — {} · {}", p.kind, p.address)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

// ── 0) Buscar en la web: investigación real (multi-fuente) ──────────────────

/// Buscador web real (DuckDuckGo con respaldo en Wikipedia). Devuelve títulos,
/// URLs y fragmentos; el agente luego puede leer las URLs con web_fetch.
pub struct SearchTool {
    web: Arc<WebClient>,
}

impl SearchTool {
    pub fn new(web: Arc<WebClient>) -> Self {
        Self { web }
    }
}

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Busca en internet. Entrada: una consulta de búsqueda. Devuelve los \
         resultados (título, URL, fragmento). Luego usa web_fetch para leer una URL."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let results = self
            .web
            .search(input.trim(), 5)
            .await
            .map_err(|e| e.to_string())?;
        if results.is_empty() {
            return Ok("(sin resultados)".into());
        }
        Ok(results
            .iter()
            .map(|r| format!("• {} — {}\n  {}", r.title, r.url, r.snippet))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

// ── 1) Recordar: el agente escribe en su memoria de largo plazo ─────────────

/// Permite al agente **persistir** algo que aprendió (memoria cognitiva activa).
pub struct RememberTool {
    memory: Arc<VectorMemory>,
}

impl RememberTool {
    pub fn new(memory: Arc<VectorMemory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }
    fn description(&self) -> &str {
        "Guarda un hecho o aprendizaje en tu memoria de largo plazo para recordarlo \
         en el futuro. Entrada: el texto a recordar."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        if input.trim().is_empty() {
            return Err("nada que recordar".into());
        }
        self.memory
            .store(input.trim())
            .await
            .map(|_| format!("recordado: «{}»", input.trim()))
            .map_err(|e| e.to_string())
    }
}

// ── 2) Forjar skill: el agente SE ESCRIBE una herramienta nueva ─────────────

/// El agente describe una capacidad numérica que necesita; AION genera un módulo
/// WASM (WAT) con el LLM, lo **valida en sandbox con tests** y, si pasa, lo
/// registra para poder usarlo. Auto-escritura segura (gated por sandbox+oráculo).
pub struct SkillForgeTool {
    engine: Arc<OllamaEngine>,
    host: Arc<WasmSkillHost>,
}

impl SkillForgeTool {
    pub fn new(engine: Arc<OllamaEngine>, host: Arc<WasmSkillHost>) -> Self {
        Self { engine, host }
    }
}

#[async_trait]
impl Tool for SkillForgeTool {
    fn name(&self) -> &str {
        "skill_forge"
    }
    fn description(&self) -> &str {
        "Crea (te escribes a ti mismo) una skill nueva que calcula sobre un entero y \
         devuelve un entero. Entrada JSON: \
         {\"name\":\"factorial\",\"description\":\"n!\",\"tests\":[[3,6],[4,24]]}. \
         La skill se valida en sandbox con esos tests antes de integrarse; si pasa, \
         queda disponible para skill_invoke."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct Spec {
            name: String,
            description: String,
            tests: Vec<(i64, i64)>,
        }
        let spec: Spec = serde_json::from_str(input.trim()).map_err(|e| {
            format!(
                "entrada inválida ({e}). Usa JSON: \
                 {{\"name\":\"...\",\"description\":\"...\",\"tests\":[[in,out],...]}}"
            )
        })?;
        if spec.tests.is_empty() {
            return Err("necesito al menos un test (entrada,salida) como oráculo".into());
        }

        let mut last = String::new();
        for _ in 0..3 {
            let prompt = format!(
                "Escribe un módulo WebAssembly en formato WAT que exporte una función `run` \
                 que reciba un i64 y devuelva un i64, implementando: {}.\n\n\
                 Ejemplo de formato VÁLIDO (esto duplica n):\n\
                 (module (func (export \"run\") (param $n i64) (result i64) \
                 (i64.mul (local.get $n) (i64.const 2))))\n\n\
                 Responde SOLO con el módulo WAT, sin explicación ni markdown.",
                spec.description
            );
            let msg = self
                .engine
                .generate(GenerateRequest {
                    messages: vec![Message::user(prompt)],
                    think: false,
                    temperature: Some(0.3),
                    max_tokens: Some(256),
                })
                .await
                .map_err(|e| e.to_string())?;
            let Some(code) = crate::extract_wat(&msg.content) else {
                last = "el LLM no produjo WAT válido".into();
                continue;
            };
            let mut evo = EvolutionEngine::new(self.host.clone());
            let report = evo
                .propose(Candidate {
                    manifest: SkillManifest {
                        name: spec.name.clone(),
                        description: spec.description.clone(),
                    },
                    code: code.clone(),
                    tests: spec.tests.clone(),
                })
                .await
                .map_err(|e| e.to_string())?;
            if report.accepted {
                // RATCHET (MOSS/Ratchet): no aceptar una versión que rinda por debajo
                // de la mejor previa — la auto-mejora nunca regresa.
                let best = crate::skill_store::best_passed(&spec.name);
                if report.passed < best {
                    last = format!(
                        "candidata pasó {} tests pero la mejor versión previa pasó {best} (ratchet: no regreso)",
                        report.passed
                    );
                    continue;
                }
                // PERSISTE la skill (con su marca): la caja de herramientas crece y
                // solo mejora.
                let _ =
                    crate::skill_store::save(&spec.name, &spec.description, &code, report.passed);
                return Ok(format!(
                    "✅ skill «{}» creada, validada ({} tests ok) y GUARDADA para el futuro. \
                     Úsala con skill_invoke. (auto-generada, aprobada por sandbox+tests+ratchet)",
                    spec.name, report.passed
                ));
            }
            last = report.reason;
        }
        Err(format!(
            "no logré crear una skill válida tras 3 intentos: {last} (el sistema queda intacto)"
        ))
    }
}

// ── 3) Invocar skill: usar una skill (semilla o forjada) ────────────────────

/// Invoca una skill registrada (incluidas las que el agente acaba de forjar).
pub struct SkillInvokeTool {
    host: Arc<WasmSkillHost>,
}

impl SkillInvokeTool {
    pub fn new(host: Arc<WasmSkillHost>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for SkillInvokeTool {
    fn name(&self) -> &str {
        "skill_invoke"
    }
    fn description(&self) -> &str {
        "Ejecuta una skill por nombre sobre un entero. Entrada: \"nombre numero\" \
         (p. ej. \"factorial 5\")."
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        let mut it = input.trim().splitn(2, char::is_whitespace);
        let name = it.next().unwrap_or("").trim();
        let num: i64 = it
            .next()
            .unwrap_or("")
            .trim()
            .parse()
            .map_err(|_| "uso: \"nombre numero\" (el número debe ser entero)".to_string())?;
        if name.is_empty() {
            return Err("falta el nombre de la skill".into());
        }
        let out = self
            .host
            .invoke(name, serde_json::json!(num))
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!("{} = {}", name, out.output["result"]))
    }
}
