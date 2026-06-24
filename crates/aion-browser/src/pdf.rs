//! **Render HTML → PDF** con el Chromium headless propio (CDP vía chromiumoxide).
//!
//! Reutiliza el localizador de Chrome del navegador agéntico ([`crate::driver::chrome_path`])
//! pero lanza una instancia DEDICADA y efímera para imprimir: así no interfiere con la
//! página que el agente esté navegando. 100% local, sin sidecar ni API keys. Es la base
//! de la generación de documentos (crate `aion-docgen`): el contenido se compone como
//! HTML+CSS (con los design tokens de AION) y aquí se convierte en un PDF de marca real.
//!
//! Multiplataforma por construcción (a diferencia del viejo `make_document`, que dependía
//! de `textutil`/`cupsfilter` de macOS): donde haya Chrome/Chromium, hay PDF.

use aion_kernel::{AionError, Result};
use chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams;
use chromiumoxide::{Browser, BrowserConfig};
use futures_util::StreamExt;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

/// Opciones de impresión a PDF. Por defecto: A4 implícito vía CSS `@page`, fondo activado
/// (para que se respeten los colores de marca) y márgenes a cargo del CSS.
#[derive(Debug, Clone)]
pub struct PdfOptions {
    /// Imprime los fondos CSS (colores/sombras de marca). Casi siempre `true`.
    pub print_background: bool,
    /// Respeta el tamaño de página declarado en CSS `@page` (recomendado).
    pub prefer_css_page_size: bool,
    /// Apaisado.
    pub landscape: bool,
    /// Margen uniforme en pulgadas (unidad del CDP). `0.0` = deja que mande el CSS `@page`.
    pub margin_in: f64,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            print_background: true,
            prefer_css_page_size: true,
            landscape: false,
            margin_in: 0.0,
        }
    }
}

/// Contador de procesos para nombrar archivos temporales sin colisión (no necesitamos
/// aleatoriedad criptográfica, solo unicidad dentro del proceso).
fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Renderiza un documento HTML COMPLETO (`<!doctype html>…`) a PDF y devuelve los bytes.
///
/// El HTML se escribe a un archivo temporal y se carga por `file://`, de modo que el
/// navegador puede resolver recursos locales (CSS embebido, imágenes `data:` o `file://`).
/// Lanza un Chromium headless dedicado y lo cierra al terminar.
pub async fn html_to_pdf(html: &str, opts: &PdfOptions) -> Result<Vec<u8>> {
    // 1) HTML → archivo temporal (evita límites de longitud de las `data:` URL y permite
    //    cargar assets relativos).
    let dir = std::env::temp_dir().join("aion-docgen");
    std::fs::create_dir_all(&dir)
        .map_err(|e| AionError::Internal(format!("no pude crear el temporal de docgen: {e}")))?;
    let file = dir.join(format!("doc-{}-{}.html", std::process::id(), next_id()));
    {
        let mut f = std::fs::File::create(&file)
            .map_err(|e| AionError::Internal(format!("no pude escribir el HTML temporal: {e}")))?;
        f.write_all(html.as_bytes())
            .map_err(|e| AionError::Internal(format!("no pude volcar el HTML: {e}")))?;
    }
    let url = format!("file://{}", file.display());

    // 2) Chromium headless DEDICADO (no toca la sesión del navegador agéntico).
    let mut builder = BrowserConfig::builder();
    if let Some(p) = crate::driver::chrome_path() {
        builder = builder.chrome_executable(p);
    }
    let profile =
        std::env::temp_dir().join(format!("aion-pdf-{}-{}", std::process::id(), next_id()));
    let _ = std::fs::create_dir_all(&profile);
    let config = builder
        .user_data_dir(profile)
        .new_headless_mode()
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .build()
        .map_err(|e| {
            AionError::Internal(format!(
                "no encuentro Chrome para generar el PDF: {e}. Instala Google Chrome o define AION_CHROME."
            ))
        })?;
    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| AionError::Internal(format!("no pude lanzar Chrome para el PDF: {e}")))?;
    // El handler DEBE bombearse o el navegador se cuelga.
    let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });

    // 3) Render + impresión (aislado para limpiar siempre, pase lo que pase).
    let result = render_inner(&browser, &url, opts).await;

    // 4) Limpieza incondicional.
    let _ = browser.close().await;
    pump.abort();
    let _ = std::fs::remove_file(&file);

    result
}

async fn render_inner(browser: &Browser, url: &str, opts: &PdfOptions) -> Result<Vec<u8>> {
    let page = browser
        .new_page(url)
        .await
        .map_err(|e| AionError::Internal(format!("no pude abrir el HTML para imprimir: {e}")))?;
    // Espera a que el documento termine de cargar (fuentes, imágenes embebidas, layout).
    let _ = page.wait_for_navigation().await;

    let mut params = PrintToPdfParams::builder()
        .print_background(opts.print_background)
        .prefer_css_page_size(opts.prefer_css_page_size)
        .landscape(opts.landscape);
    if opts.margin_in > 0.0 {
        params = params
            .margin_top(opts.margin_in)
            .margin_bottom(opts.margin_in)
            .margin_left(opts.margin_in)
            .margin_right(opts.margin_in);
    }
    let params = params.build();

    let bytes = page
        .pdf(params)
        .await
        .map_err(|e| AionError::Internal(format!("printToPDF falló: {e}")))?;
    let _ = page.close().await;
    Ok(bytes)
}
