//! **Navegador agéntico real** (CDP nativo en Rust vía chromiumoxide).
//!
//! A diferencia de `WebClient::fetch_text` (descarga HTML estático), esto controla un
//! Chrome headless de verdad: ejecuta JavaScript, navega, hace clic y rellena formularios.
//! 100% local (usa tu Chrome instalado), sin API keys ni sidecar. Es la base del trait
//! `BrowserDriver` del plan maestro (F5): backend nativo chromiumoxide.
//!
//! Mantiene una SESIÓN persistente (un navegador + página actual) para que `open` →
//! `click`/`type` operen sobre la misma página entre llamadas de herramientas.

use aion_kernel::{AionError, Result};
use async_trait::async_trait;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures_util::StreamExt;
use std::sync::OnceLock;
use tokio::sync::Mutex;

/// Contrato de un driver de navegador (permite cambiar de backend: nativo/sidecar).
#[async_trait]
pub trait BrowserDriver: Send + Sync {
    async fn open(&self, url: &str) -> Result<PageView>;
    async fn read(&self) -> Result<PageView>;
    /// Árbol de accesibilidad: elementos INTERACTIVOS numerados (el LLM elige por
    /// etiqueta, no por selector CSS). Es la forma robusta de navegar (2026).
    async fn snapshot(&self) -> Result<Snapshot>;
    async fn click(&self, selector: &str) -> Result<()>;
    async fn type_text(&self, selector: &str, text: &str) -> Result<()>;
    /// Desplaza la página: `dir` = "down"|"up"|"top"|"bottom"|"text:<texto>". Devuelve la vista
    /// tras desplazar. Esencial para páginas largas/infinitas (patrón browser-use).
    async fn scroll(&self, _dir: &str) -> Result<PageView> {
        Err(AionError::Internal("scroll no soportado".into()))
    }
    /// Extrae el CONTENIDO principal legible de la página (estilo lectura): quita menús,
    /// barras, scripts y deja el texto que importa. Para leer artículos/productos sin ruido.
    async fn extract(&self) -> Result<String> {
        Err(AionError::Internal("extract no soportado".into()))
    }
    /// Vuelve a la página anterior del historial. Devuelve la nueva vista.
    async fn back(&self) -> Result<PageView> {
        Err(AionError::Internal("back no soportado".into()))
    }
    /// Rellena el formulario de login de la página actual con usuario+contraseña.
    /// Los valores se inyectan en la PÁGINA (vía CDP) y NUNCA se devuelven: la función
    /// solo informa de qué campos rellenó. Base de la bóveda de credenciales.
    async fn fill_login(&self, username: &str, password: &str) -> Result<String>;
    async fn screenshot_b64(&self) -> Result<String>;
    async fn close(&self) -> Result<()>;
}

/// Vista legible de una página (para que el LLM razone): título + texto visible.
#[derive(Debug, Clone)]
pub struct PageView {
    pub title: String,
    pub url: String,
    pub text: String,
}

/// Un elemento interactivo etiquetado (ref estable para click/type por número).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct El {
    #[serde(rename = "ref")]
    pub ref_id: u32,
    pub role: String,
    pub name: String,
    #[serde(default)]
    pub kind: String,
}

/// Instantánea de accesibilidad: vista + elementos interactivos numerados.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub view: PageView,
    pub elements: Vec<El>,
}

const MAX_TEXT: usize = 6000;

// ── Sesión global (un navegador vivo + página actual) ───────────────────────

struct Session {
    browser: Browser,
    page: Option<Page>,
    _handler: tokio::task::JoinHandle<()>,
}

fn cell() -> &'static Mutex<Option<Session>> {
    static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(None))
}

/// Carpeta de datos de AION (`~/Library/Application Support/AION` en macOS).
fn aion_data_dir() -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    let base = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join("Library/Application Support/AION"))
        .unwrap_or_else(|_| std::env::temp_dir());
    #[cfg(not(target_os = "macos"))]
    let base = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/aion"))
        .unwrap_or_else(|_| std::env::temp_dir());
    base
}

/// Chromium PROPIO empaquetado dentro de AION.app (Contents/Resources/chromium-runtime),
/// relativo al ejecutable del sidecar. Es el camino de producción: el navegador es de AION,
/// independiente del Chrome personal del usuario.
fn bundled_chromium() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    // .../AION.app/Contents/MacOS/aion-core → subir a Contents/ y entrar a Resources/
    let contents = exe.parent()?.parent()?; // Contents
    let candidates = [
        contents.join("Resources/chromium-runtime/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
        contents.join("Resources/chromium-runtime/Chromium.app/Contents/MacOS/Chromium"),
    ];
    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Chromium PROPIO descargado en los datos de AION (`.../AION/chromium`). Sirve en desarrollo
/// y como red de seguridad si el bundle no lo trae.
fn downloaded_chromium() -> Option<String> {
    let p = aion_data_dir().join(
        "chromium/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    );
    p.exists().then(|| p.to_string_lossy().into_owned())
}

/// Localiza el ejecutable del navegador. PRIORIZA el Chromium PROPIO de AION (interno, solo de
/// los agentes); el Chrome del sistema queda como último recurso (es "de afuera").
fn chrome_path() -> Option<String> {
    if let Ok(p) = std::env::var("AION_CHROME") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    if let Some(p) = bundled_chromium() {
        return Some(p);
    }
    if let Some(p) = downloaded_chromium() {
        return Some(p);
    }
    // Último recurso: navegador del sistema (no ideal; el navegador debe ser propio de AION).
    #[cfg(target_os = "macos")]
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    ];
    #[cfg(target_os = "windows")]
    let candidates = [
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
    ];
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let candidates = [
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];

    candidates
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|p| p.to_string())
}

/// UA realista de Chrome estable (NO "HeadlessChrome", que delata al bot). Override
/// con AION_BROWSER_UA.
fn stealth_ua() -> String {
    std::env::var("AION_BROWSER_UA").unwrap_or_else(|_| {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36"
            .into()
    })
}

/// Perfil PERSISTENTE del navegador de AION. A diferencia de un dir temporal por proceso,
/// este sobrevive reinicios: las cookies y sesiones (logins de Google/GitHub/Reddit/redes)
/// se conservan, que es justo lo que pide un navegador "de verdad". Es un perfil DEDICADO de
/// AION (no el de Chrome del usuario), así que no hay conflicto con el navegador personal.
/// Override con AION_BROWSER_PROFILE.
fn profile_dir() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("AION_BROWSER_PROFILE") {
        if !p.is_empty() {
            return p.into();
        }
    }
    aion_data_dir().join("browser-profile")
}

/// ¿Lanzar el navegador VISIBLE (headful) en vez de headless? Necesario para que Ariel haga
/// login a mano en sus cuentas (la sesión queda guardada en el perfil persistente y luego el
/// agente la reutiliza headless). Headful además es aún menos detectable. Activa con
/// AION_BROWSER_HEADFUL=1.
fn headful() -> bool {
    std::env::var("AION_BROWSER_HEADFUL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// ¿Comportarse como humano (escritura con ritmo, pausas)? Activado por defecto: es lo que más
/// reduce la detección por PATRÓN. Se puede desactivar (AION_BROWSER_HUMAN=0) para máxima
/// velocidad cuando no importa parecer humano.
fn human_mode() -> bool {
    std::env::var("AION_BROWSER_HUMAN")
        .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true)
}

/// Lanza el navegador si aún no hay sesión. Devuelve error claro si falta Chrome.
async fn ensure(sess: &mut Option<Session>) -> Result<()> {
    if sess.is_some() {
        return Ok(());
    }
    let mut builder = BrowserConfig::builder();
    if let Some(p) = chrome_path() {
        builder = builder.chrome_executable(p);
    }
    // PERFIL PERSISTENTE Y DEDICADO: conserva sesiones entre reinicios y evita el conflicto
    // SingletonLock con el Chrome personal del usuario. Si una sesión previa de AION dejó un
    // lock huérfano, se limpia (somos instancia única local).
    let profile = profile_dir();
    let _ = std::fs::create_dir_all(&profile);
    for lock in ["SingletonLock", "SingletonCookie", "SingletonSocket"] {
        let _ = std::fs::remove_file(profile.join(lock));
    }
    builder = builder
        .user_data_dir(profile)
        // STEALTH (legítimo): hace que el navegador del agente parezca uno normal.
        // Son las mismas medidas estándar de puppeteer-stealth/undetected-chromedriver.
        .arg("--disable-blink-features=AutomationControlled") // quita navigator.webdriver
        .arg(format!("--user-agent={}", stealth_ua()))
        .arg("--lang=es-ES,es")
        .arg("--window-size=1280,800")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg("--disable-infobars");
    // Headless "new": más fiel a un Chrome real que el headless antiguo (menos detectable).
    // En modo headful (login manual) se omite para que la ventana sea visible.
    if !headful() {
        builder = builder.new_headless_mode();
    } else {
        builder = builder.with_head();
    }
    let config = builder
        .build()
        .map_err(|e| AionError::Internal(format!("no encuentro Chrome para el navegador: {e}. Instala Google Chrome o define AION_CHROME.")))?;
    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| AionError::Internal(format!("no pude lanzar el navegador: {e}")))?;
    // El handler DEBE bombearse continuamente o el navegador se cuelga.
    let h = tokio::spawn(async move { while handler.next().await.is_some() {} });
    *sess = Some(Session {
        browser,
        page: None,
        _handler: h,
    });
    Ok(())
}

async fn page_view(page: &Page) -> Result<PageView> {
    let title = page.get_title().await.ok().flatten().unwrap_or_default();
    let url = page.url().await.ok().flatten().unwrap_or_default();
    // Texto VISIBLE (no HTML crudo): lo que un humano leería.
    let text = page
        .evaluate("document.body ? document.body.innerText : ''")
        .await
        .ok()
        .and_then(|r| r.into_value::<String>().ok())
        .unwrap_or_default();
    let mut text = text;
    if text.len() > MAX_TEXT {
        text.truncate(MAX_TEXT);
        text.push_str("\n…(texto truncado)");
    }
    Ok(PageView { title, url, text })
}

/// Driver nativo basado en chromiumoxide (Chrome DevTools Protocol).
pub struct ChromiumoxideDriver;

impl Default for ChromiumoxideDriver {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl BrowserDriver for ChromiumoxideDriver {
    async fn open(&self, url: &str) -> Result<PageView> {
        crate::guard_url(url)?;
        let mut guard = cell().lock().await;
        ensure(&mut guard).await?;
        let sess = guard.as_mut().unwrap();
        // Página en blanco → inyecta stealth ANTES de navegar → navega. Así el script
        // que oculta las señales de automatización se ejecuta en cada documento nuevo.
        let page = sess
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| AionError::Internal(format!("no pude crear la página: {e}")))?;
        let _ = page.evaluate_on_new_document(STEALTH_JS).await;
        // Override fiable del UA por CDP (elimina "HeadlessChrome").
        let _ = page.set_user_agent(stealth_ua()).await;
        page.goto(url)
            .await
            .map_err(|e| AionError::Internal(format!("no pude abrir la página: {e}")))?;
        let _ = page.wait_for_navigation().await;
        let view = page_view(&page).await?;
        sess.page = Some(page);
        Ok(view)
    }

    async fn read(&self) -> Result<PageView> {
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        page_view(page).await
    }

    async fn snapshot(&self) -> Result<Snapshot> {
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        let view = page_view(page).await?;
        // Etiqueta cada elemento interactivo VISIBLE con data-aion-ref=N y devuelve la
        // lista. Luego click/type usan el selector [data-aion-ref="N"].
        let json = page
            .evaluate(AX_SNAPSHOT_JS)
            .await
            .ok()
            .and_then(|r| r.into_value::<String>().ok())
            .unwrap_or_else(|| "[]".into());
        let elements: Vec<El> = serde_json::from_str(&json).unwrap_or_default();
        Ok(Snapshot { view, elements })
    }

    async fn click(&self, selector: &str) -> Result<()> {
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        let el = page.find_element(selector).await.map_err(|e| {
            AionError::Internal(format!("no encontré el elemento «{selector}»: {e}"))
        })?;
        if human_mode() {
            // Pausa humana antes de actuar (un humano no hace clic en 0 ms tras ver el elemento).
            tokio::time::sleep(std::time::Duration::from_millis(220)).await;
        }
        el.click()
            .await
            .map_err(|e| AionError::Internal(format!("no pude hacer clic: {e}")))?;
        let _ = page.wait_for_navigation().await;
        Ok(())
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<()> {
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        let el = page
            .find_element(selector)
            .await
            .map_err(|e| AionError::Internal(format!("no encontré el campo «{selector}»: {e}")))?;
        el.click().await.ok();
        // ESCRITURA HUMANA: tecla a tecla con ritmo variable (lo que más despista a la detección
        // por patrón). Para textos muy largos o si se desactiva el modo humano, escribe de golpe.
        if human_mode() && text.chars().count() <= 200 {
            tokio::time::sleep(std::time::Duration::from_millis(180)).await; // foco → primera tecla
            for ch in text.chars() {
                el.type_str(&ch.to_string())
                    .await
                    .map_err(|e| AionError::Internal(format!("no pude escribir: {e}")))?;
                // 45–110 ms por tecla, variando con el carácter (sin dependencias de azar).
                let jitter = 45 + (ch as u64 % 66);
                tokio::time::sleep(std::time::Duration::from_millis(jitter)).await;
            }
        } else {
            el.type_str(text)
                .await
                .map_err(|e| AionError::Internal(format!("no pude escribir: {e}")))?;
        }
        Ok(())
    }

    async fn scroll(&self, dir: &str) -> Result<PageView> {
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        // Construye el JS de desplazamiento según la dirección pedida.
        let js = match dir.trim() {
            "top" => "window.scrollTo(0, 0)".to_string(),
            "bottom" => "window.scrollTo(0, document.body.scrollHeight)".to_string(),
            "up" => "window.scrollBy(0, -Math.round(innerHeight*0.9))".to_string(),
            d if d.starts_with("text:") => {
                let needle = d.trim_start_matches("text:").trim();
                format!(
                    "(() => {{ const t={}; const el=[...document.querySelectorAll('body *')]\
                     .find(e=>e.innerText&&e.innerText.toLowerCase().includes(t.toLowerCase())); \
                     if(el){{el.scrollIntoView({{block:'center'}});return true;}} return false; }})()",
                    serde_json::to_string(needle).unwrap_or_else(|_| "\"\"".into())
                )
            }
            _ => "window.scrollBy(0, Math.round(innerHeight*0.9))".to_string(), // "down" por defecto
        };
        let _ = page.evaluate(js).await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await; // deja cargar lazy-load
        page_view(page).await
    }

    async fn extract(&self) -> Result<String> {
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        let text = page
            .evaluate(EXTRACT_CONTENT_JS)
            .await
            .ok()
            .and_then(|r| r.into_value::<String>().ok())
            .unwrap_or_default();
        let mut text = text;
        if text.len() > MAX_TEXT * 2 {
            text.truncate(MAX_TEXT * 2);
            text.push_str("\n…(contenido truncado)");
        }
        Ok(text)
    }

    async fn back(&self) -> Result<PageView> {
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        let _ = page.evaluate("history.back()").await;
        let _ = page.wait_for_navigation().await;
        page_view(page).await
    }

    async fn fill_login(&self, username: &str, password: &str) -> Result<String> {
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        // Valores embebidos de forma segura (JSON escapa comillas). Se quedan en la
        // página; esta función solo devuelve los nombres de los campos rellenados.
        let js = format!(
            "({FILL_LOGIN_FN})({}, {})",
            serde_json::to_string(username).unwrap_or_else(|_| "\"\"".into()),
            serde_json::to_string(password).unwrap_or_else(|_| "\"\"".into()),
        );
        let filled = page
            .evaluate(js)
            .await
            .ok()
            .and_then(|r| r.into_value::<String>().ok())
            .unwrap_or_else(|| "[]".into());
        Ok(filled)
    }

    async fn screenshot_b64(&self) -> Result<String> {
        use base64::Engine;
        let guard = cell().lock().await;
        let sess = guard.as_ref().ok_or_else(no_page)?;
        let page = sess.page.as_ref().ok_or_else(no_page)?;
        let bytes = page
            .screenshot(
                chromiumoxide::page::ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .build(),
            )
            .await
            .map_err(|e| AionError::Internal(format!("captura falló: {e}")))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    async fn close(&self) -> Result<()> {
        let mut guard = cell().lock().await;
        if let Some(mut sess) = guard.take() {
            let _ = sess.browser.close().await;
        }
        Ok(())
    }
}

fn no_page() -> AionError {
    AionError::Internal("no hay ninguna página abierta; usa browser_open primero".into())
}

/// Resultado crudo extraído del DOM del SERP (se mapea a `crate::SearchResult`).
#[derive(serde::Deserialize)]
struct RawHit {
    title: String,
    url: String,
    #[serde(default)]
    snippet: String,
}

/// Buscadores PERMISIVOS con el acceso automatizado (a diferencia de Google/Bing, que muestran
/// challenge al instante). Índices propios o proxys que dan datos web REALES. Se prueban EN ORDEN
/// y nos quedamos con el primero que responda; si uno pide captcha, rotamos al siguiente. Cada uno
/// es `(nombre, url_con_{q})`.
const ENGINES: &[(&str, &str)] = &[
    ("Brave", "https://search.brave.com/search?q={q}"),
    ("Startpage", "https://www.startpage.com/sp/search?query={q}"), // resultados de Google (proxy)
    ("DuckDuckGo", "https://lite.duckduckgo.com/lite/?q={q}"),
    ("Mojeek", "https://www.mojeek.com/search?q={q}"),
    ("Ecosia", "https://www.ecosia.org/search?q={q}"),
];

/// Una sola consulta a un buscador con el navegador real. Renderiza el SERP (JS), gestiona el
/// consentimiento de cookies (UE/Italia), detecta bloqueo anti-bot (captcha/`/sorry/`) y extrae
/// los resultados con un extractor GENÉRICO semántico (robusto a cambios de CSS). Si está
/// bloqueado, devuelve `Err` para que el llamador rote a otro motor. NUNCA resuelve captchas.
async fn serp_one(url: &str, limit: usize) -> Result<Vec<crate::SearchResult>> {
    let mut guard = cell().lock().await;
    ensure(&mut guard).await?;
    let sess = guard.as_mut().unwrap();
    let page =
        sess.browser.new_page("about:blank").await.map_err(|e| {
            AionError::Internal(format!("no pude crear la página de búsqueda: {e}"))
        })?;
    let _ = page.evaluate_on_new_document(STEALTH_JS).await;
    let _ = page.set_user_agent(stealth_ua()).await;
    let nav_ok = page.goto(url).await.is_ok();
    let _ = page.wait_for_navigation().await;
    // Comportamiento humano: deja respirar al SERP para que cargue su JS (y no parezca ráfaga).
    tokio::time::sleep(std::time::Duration::from_millis(1400)).await;
    let _ = page.evaluate(CONSENT_JS).await; // cierra banner de cookies si aparece
    let final_url = page.url().await.ok().flatten().unwrap_or_default();
    let title = page.get_title().await.ok().flatten().unwrap_or_default();
    let blocked = final_url.contains("/sorry/")
        || final_url.contains("/captcha")
        || title.to_lowercase().contains("captcha")
        || title.contains("403")
        || title.contains("Un último paso");
    if !nav_ok {
        let _ = page.close().await;
        return Err(AionError::Internal("no pude abrir el buscador".into()));
    }
    if blocked {
        let _ = page.close().await;
        return Err(AionError::Internal(
            "buscador pidió verificación (rotar)".into(),
        ));
    }
    let json = page
        .evaluate(GENERIC_SERP_JS)
        .await
        .ok()
        .and_then(|r| r.into_value::<String>().ok())
        .unwrap_or_else(|| "[]".into());
    let _ = page.close().await; // página temporal
    let hits: Vec<RawHit> = serde_json::from_str(&json).unwrap_or_default();
    Ok(hits
        .into_iter()
        .filter(|h| h.url.starts_with("http") && !h.title.is_empty())
        .take(limit)
        .map(|h| crate::SearchResult {
            title: h.title,
            url: h.url,
            snippet: h.snippet,
        })
        .collect())
}

/// **Búsqueda web REAL** de AION: prueba los buscadores permisivos EN ORDEN y devuelve los
/// resultados del primero que responda con datos (rotando si alguno pide captcha). Datos web de
/// verdad (no Wikipedia), con el navegador propio e invisible de AION. Respeta `AION_PROXY` (Tor)
/// para rotar IP si se configura.
pub async fn web_search(query: &str, limit: usize) -> Result<Vec<crate::SearchResult>> {
    let q = crate::urlencode(query.trim());
    let mut last_err = None;
    for (name, tmpl) in ENGINES {
        let url = tmpl.replace("{q}", &q);
        match serp_one(&url, limit).await {
            Ok(hits) if hits.len() >= 3 => {
                tracing::debug!(engine = name, n = hits.len(), "web_search ok");
                return Ok(hits);
            }
            Ok(hits) => last_err = Some(format!("{name}: solo {} resultados", hits.len())),
            Err(e) => last_err = Some(format!("{name}: {e}")),
        }
    }
    // Ningún motor permisivo respondió: que el fan-out caiga a las otras fuentes (académicas, etc.).
    Err(AionError::Internal(format!(
        "ningún buscador respondió ({})",
        last_err.unwrap_or_default()
    )))
}

/// DEBUG: abre una URL en página temporal y devuelve el innerHTML de un selector (para
/// inspeccionar la estructura de un SERP al escribir su extractor). No para producción.
pub async fn debug_html(url: &str, selector: &str) -> Result<String> {
    let mut guard = cell().lock().await;
    ensure(&mut guard).await?;
    let sess = guard.as_mut().unwrap();
    let page = sess
        .browser
        .new_page("about:blank")
        .await
        .map_err(|e| AionError::Internal(format!("page: {e}")))?;
    let _ = page.evaluate_on_new_document(STEALTH_JS).await;
    let _ = page.goto(url).await;
    let _ = page.wait_for_navigation().await;
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await; // SERP carga JS async
    let js = if let Some(expr) = selector.strip_prefix("js:") {
        format!(
            "(() => {{ try {{ return String({expr}); }} catch (e) {{ return '(err) '+e; }} }})()"
        )
    } else {
        format!(
            "(() => {{ const e = document.querySelector({}); return e ? e.outerHTML.slice(0,4000) : '(no encontrado)'; }})()",
            serde_json::to_string(selector).unwrap_or_else(|_| "\"body\"".into())
        )
    };
    let html = page
        .evaluate(js)
        .await
        .ok()
        .and_then(|r| r.into_value::<String>().ok())
        .unwrap_or_default();
    let _ = page.close().await;
    Ok(html)
}

/// Búsqueda REAL en **Google**. Headless sin sesión, Google muestra challenge y devuelve `Err`;
/// funciona cuando Ariel está logueado (perfil persistente). Disponible para ese caso.
pub async fn google_search(query: &str, limit: usize) -> Result<Vec<crate::SearchResult>> {
    let q = crate::urlencode(query.trim());
    serp_one(
        &format!("https://www.google.com/search?q={q}&hl=es&gl=it"),
        limit,
    )
    .await
}

/// Búsqueda REAL en **Bing**. Igual que Google, suele pedir challenge sin sesión. Disponible para
/// uso logueado.
pub async fn bing_search(query: &str, limit: usize) -> Result<Vec<crate::SearchResult>> {
    let q = crate::urlencode(query.trim());
    serp_one(
        &format!("https://www.bing.com/search?q={q}&setlang=es&cc=it"),
        limit,
    )
    .await
}

/// Función JS que localiza los campos de usuario y contraseña del login y los rellena,
/// disparando los eventos input/change (para que el sitio detecte el valor). Devuelve
/// SOLO los nombres de los campos rellenados (nunca los valores).
const FILL_LOGIN_FN: &str = r#"(u, p) => {
  const set = (el, val) => {
    el.focus();
    const proto = Object.getPrototypeOf(el);
    const setter = Object.getOwnPropertyDescriptor(proto, 'value') &&
      Object.getOwnPropertyDescriptor(proto, 'value').set;
    if (setter) setter.call(el, val); else el.value = val;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const vis = (el) => el && el.getBoundingClientRect().width > 0;
  const done = [];
  const pass = [...document.querySelectorAll('input[type=password]')].find(vis);
  let user = [...document.querySelectorAll(
    'input[autocomplete=username], input[type=email], input[name*=user i], input[name*=email i], input[id*=user i], input[id*=email i]'
  )].find(vis);
  if (!user) user = [...document.querySelectorAll('input[type=text], input:not([type])')].find(vis);
  if (user && u) { set(user, u); done.push('usuario'); }
  if (pass && p) { set(pass, p); done.push('contraseña'); }
  return JSON.stringify(done);
}"#;

/// JS de **stealth** inyectado en cada documento nuevo: oculta las señales típicas de
/// automatización (las mismas que parchea puppeteer-stealth). Hace que el navegador del
/// agente parezca uno normal — legítimo para automatizar tus propias sesiones.
const STEALTH_JS: &str = r#"
(() => {
  const def = (obj, prop, val) => {
    try { Object.defineProperty(obj, prop, { get: () => val, configurable: true }); } catch (e) {}
  };
  try {
    // 1) Señales de automatización (lo que delata a un bot headless).
    def(navigator, 'webdriver', undefined);
    def(navigator, 'languages', ['es-ES', 'es', 'it', 'en']);
    def(navigator, 'plugins', [1, 2, 3, 4, 5]);
    // 2) Hardware realista de un Mac Apple Silicon (un headless suele reportar valores raros).
    def(navigator, 'hardwareConcurrency', 8);
    def(navigator, 'deviceMemory', 8);
    def(navigator, 'platform', 'MacIntel');
    def(navigator, 'maxTouchPoints', 0);
    window.chrome = window.chrome || { runtime: {}, app: {}, csi: () => {}, loadTimes: () => {} };
    // 3) Permisos: que notifications no delate el modo automatizado.
    const q = navigator.permissions && navigator.permissions.query;
    if (q) {
      navigator.permissions.query = (p) =>
        p && p.name === 'notifications'
          ? Promise.resolve({ state: Notification.permission })
          : q(p);
    }
    // 4) WebGL vendor/renderer realistas (señal habitual de headless).
    const patchGL = (proto) => {
      if (!proto) return;
      const gp = proto.getParameter;
      proto.getParameter = function (p) {
        if (p === 37445) return 'Apple Inc.';            // UNMASKED_VENDOR_WEBGL
        if (p === 37446) return 'Apple M-series GPU';    // UNMASKED_RENDERER_WEBGL
        return gp.call(this, p);
      };
    };
    patchGL(window.WebGLRenderingContext && WebGLRenderingContext.prototype);
    patchGL(window.WebGL2RenderingContext && WebGL2RenderingContext.prototype);
    // 5) Canvas: ruido mínimo y estable para frustrar el fingerprinting por canvas, sin romper
    //    el render real (solo altera 1 bit de unos pocos píxeles del export).
    const origToDataURL = HTMLCanvasElement.prototype.toDataURL;
    HTMLCanvasElement.prototype.toDataURL = function (...args) {
      try {
        const ctx = this.getContext('2d');
        if (ctx && this.width && this.height) {
          const w = Math.min(this.width, 16);
          const img = ctx.getImageData(0, 0, w, 1);
          for (let i = 0; i < img.data.length; i += 40) img.data[i] = img.data[i] ^ 1;
          ctx.putImageData(img, 0, 0);
        }
      } catch (e) {}
      return origToDataURL.apply(this, args);
    };
  } catch (e) {}
})();
"#;

/// JS que cierra el banner de consentimiento de cookies (Google UE y banners comunes):
/// busca un botón cuyo texto sea de aceptar/rechazar y lo pulsa. Best-effort, idempotente.
const CONSENT_JS: &str = r#"
(() => {
  try {
    const rx = /(accept all|accetta tutto|aceptar todo|reject all|rifiuta tutto|rechazar todo|i agree|acconsento|ho capito|accetto)/i;
    const cand = [...document.querySelectorAll('button, div[role=button], input[type=submit], a[role=button]')];
    const b = cand.find(el => rx.test((el.innerText || el.value || el.getAttribute('aria-label') || '')));
    if (b) { b.click(); return 'clicked'; }
  } catch (e) {}
  return 'none';
})()
"#;

/// Extrae el CONTENIDO principal legible (readability-lite): prioriza <article>/<main>, si no
/// elige el bloque con más texto, y limpia menús/scripts/estilos. Devuelve texto plano.
const EXTRACT_CONTENT_JS: &str = r#"
(() => {
  const clean = (s) => (s || '').replace(/\s+\n/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
  // 1) Contenedores semánticos preferentes.
  let root = document.querySelector('article, main, [role=main]');
  // 2) Si no hay, busca el bloque con MÁS texto (heurística de densidad).
  if (!root) {
    let best = document.body, bestLen = 0;
    for (const el of document.querySelectorAll('div, section')) {
      const t = el.innerText || '';
      if (t.length > bestLen && el.querySelectorAll('p, li, h1, h2, h3').length >= 3) {
        best = el; bestLen = t.length;
      }
    }
    root = best;
  }
  // Clona y quita ruido no informativo antes de leer el texto.
  const c = root.cloneNode(true);
  c.querySelectorAll('script, style, nav, header, footer, aside, form, noscript, [aria-hidden=true]')
    .forEach(e => e.remove());
  const title = (document.title || '').trim();
  return clean((title ? title + '\n\n' : '') + (c.innerText || ''));
})()
"#;

/// Extractor GENÉRICO de SERP, robusto a cambios de CSS: no depende de clases concretas (que
/// cada motor cambia), sino de la SEMÁNTICA común a todos los buscadores — un enlace externo con
/// un título de longitud razonable, y como snippet el texto del bloque contenedor. Funciona para
/// Brave, Startpage, DuckDuckGo, Mojeek, Ecosia… Filtra navegación, enlaces del propio motor y
/// dominios de utilidad. Devuelve JSON `[{title,url,snippet}]` por orden de aparición (relevancia).
const GENERIC_SERP_JS: &str = r#"
(() => {
  const out = [];
  const seen = new Set();
  const self = location.hostname.replace(/^www\./, '');
  const badHost = /(google|bing|brave|mojeek|startpage|duckduckgo|ecosia|gstatic|googleusercontent|microsoft|yahoo)\./i;
  const badPath = /\/(search|sp\/search|settings|preferences|about|privacy|help|login|signin|account|maps|images|videos|news|imghp)\b/i;
  for (const a of document.querySelectorAll('a[href^="http"]')) {
    let url = a.href || '';
    let host;
    try { host = new URL(url).hostname.replace(/^www\./, ''); } catch (e) { continue; }
    if (host === self || badHost.test(host)) continue;
    if (badPath.test(url)) continue;
    const title = (a.innerText || '').trim().replace(/\s+/g, ' ');
    if (title.length < 12) continue; // descarta enlaces de navegación / iconos
    if (seen.has(url)) continue;
    seen.add(url);
    // Snippet: sube unos niveles al bloque del resultado y toma su texto sin el título.
    let box = a;
    for (let k = 0; k < 4 && box.parentElement; k++) box = box.parentElement;
    let snip = (box.innerText || '').replace(title, ' ').replace(/\s+/g, ' ').trim().slice(0, 300);
    out.push({ title, url, snippet: snip });
    if (out.length >= 15) break;
  }
  return JSON.stringify(out);
})()
"#;

/// JS que recorre el DOM, etiqueta los elementos INTERACTIVOS visibles con
/// `data-aion-ref` y devuelve un JSON `[{ref, role, name, kind}]`. Es el "árbol de
/// accesibilidad" práctico: el LLM ve etiquetas legibles y actúa por número.
const AX_SNAPSHOT_JS: &str = r#"
(() => {
  const q = 'a,button,input,textarea,select,[role=button],[role=link],[role=tab],[role=checkbox],[onclick]';
  const els = Array.from(document.querySelectorAll(q));
  const out = [];
  let i = 0;
  for (const el of els) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) continue;
    if (el.disabled || el.getAttribute('aria-hidden') === 'true') continue;
    i++;
    el.setAttribute('data-aion-ref', String(i));
    const tag = el.tagName.toLowerCase();
    const role = el.getAttribute('role') || tag;
    let name = (el.innerText || el.value || el.placeholder ||
      el.getAttribute('aria-label') || el.getAttribute('name') || el.title || '')
      .trim().replace(/\s+/g, ' ').slice(0, 80);
    const kind = (tag === 'input' ? (el.getAttribute('type') || 'text') : tag);
    out.push({ ref: i, role, name, kind });
    if (i >= 60) break;
  }
  return JSON.stringify(out);
})()
"#;
