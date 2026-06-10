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

/// Lanza el navegador si aún no hay sesión. Devuelve error claro si falta Chrome.
async fn ensure(sess: &mut Option<Session>) -> Result<()> {
    if sess.is_some() {
        return Ok(());
    }
    let mut builder = BrowserConfig::builder();
    if let Some(p) = chrome_path() {
        builder = builder.chrome_executable(p);
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
        let page = sess
            .browser
            .new_page(url)
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
