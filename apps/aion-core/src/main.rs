//! Binario `aion-core`: punto de entrada del núcleo de AION.
//!
//! En F0 hace el "smoke test" del sistema: inicializa telemetría, verifica la
//! integridad del kernel, levanta el bus de eventos, emite `CoreStarted` y sale.
//! En F1 expondrá la capa IPC (Tauri commands + channels) hacia la UI.

use aion_kernel::{kernel_info, AionEvent, EventBus};
use chrono::Utc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Observabilidad
    aion_telemetry::init();

    // 2. Verificación del kernel (en F5 se comparará el hash firmado)
    let info = kernel_info();
    tracing::info!(
        kernel = info.name,
        version = info.version,
        contract = info.contract_version,
        "núcleo AION verificado"
    );

    // 3. Bus de eventos pub/sub
    let bus = EventBus::default();
    let mut rx = bus.subscribe();

    // 4. Emitir evento de arranque y confirmarlo desde un suscriptor
    bus.publish(AionEvent::CoreStarted {
        kernel_version: info.version.to_string(),
        at: Utc::now(),
    });

    if let Ok(AionEvent::CoreStarted { kernel_version, .. }) = rx.try_recv() {
        tracing::info!(%kernel_version, "✅ AION core arrancó correctamente");
    }

    tracing::info!("smoke test F0 completado — saliendo limpio");
    Ok(())
}
