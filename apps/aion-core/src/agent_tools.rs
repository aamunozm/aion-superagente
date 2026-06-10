//! Herramientas que dan al **agente conversacional** sus verdaderas capacidades:
//! memoria cognitiva (recordar), **auto-escritura de skills** (se escribe código a
//! sí mismo, validado en sandbox+tests) e invocación de las skills que ha forjado.
//! Esto unifica los "órganos" de AION dentro del agente con el que hablas.

use aion_browser::WebClient;
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
