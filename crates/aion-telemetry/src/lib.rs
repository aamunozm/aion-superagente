//! Observabilidad de AION. En F0: tracing estructurado + filtro por env.
//! En fases posteriores se añade exportación OpenTelemetry (OTLP) y el audit log
//! persistente de todas las acciones del agente y del bucle de evolución.

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Inicializa el subsistema de tracing. Idempotente-seguro: llamar una sola vez
/// al arranque. Respeta la variable de entorno `AION_LOG` (por defecto `info`).
pub fn init() {
    let filter = EnvFilter::try_from_env("AION_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true))
        .with(filter)
        .init();

    tracing::info!("aion-telemetry inicializado");
}
