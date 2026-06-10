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

/// Localiza el ejecutable de Chrome/Chromium (AION_CHROME o rutas habituales).
fn chrome_path() -> Option<String> {
    if let Ok(p) = std::env::var("AION_CHROME") {
        if !p.is_empty() {
            return Some(p);
        }
    }
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

/// Lanza el navegador si aún no hay sesión. Devuelve error claro si falta Chrome.
async fn ensure(sess: &mut Option<Session>) -> Result<()> {
    if sess.is_some() {
        return Ok(());
    }
    let mut builder = BrowserConfig::builder();
    if let Some(p) = chrome_path() {
        builder = builder.chrome_executable(p);
    }
    // PERFIL DEDICADO: evita el conflicto SingletonLock con el Chrome del usuario o con
    // una instancia previa. Cada proceso de AION usa su propio user-data-dir temporal.
    let profile = std::env::temp_dir().join(format!("aion-chrome-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&profile);
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
    builder = builder.new_headless_mode();
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
        el.type_str(text)
            .await
            .map_err(|e| AionError::Internal(format!("no pude escribir: {e}")))?;
        Ok(())
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

/// JS de **stealth** inyectado en cada documento nuevo: oculta las señales típicas de
/// automatización (las mismas que parchea puppeteer-stealth). Hace que el navegador del
/// agente parezca uno normal — legítimo para automatizar tus propias sesiones.
const STEALTH_JS: &str = r#"
(() => {
  try {
    Object.defineProperty(navigator, 'webdriver', { get: () => undefined });
    Object.defineProperty(navigator, 'languages', { get: () => ['es-ES','es','en'] });
    Object.defineProperty(navigator, 'plugins', { get: () => [1,2,3,4,5] });
    window.chrome = window.chrome || { runtime: {} };
    const q = navigator.permissions && navigator.permissions.query;
    if (q) {
      navigator.permissions.query = (p) =>
        p && p.name === 'notifications'
          ? Promise.resolve({ state: Notification.permission })
          : q(p);
    }
    // WebGL vendor/renderer realistas (otra señal habitual de headless).
    const gp = WebGLRenderingContext.prototype.getParameter;
    WebGLRenderingContext.prototype.getParameter = function (p) {
      if (p === 37445) return 'Intel Inc.';
      if (p === 37446) return 'Intel Iris OpenGL Engine';
      return gp.call(this, p);
    };
  } catch (e) {}
})();
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
