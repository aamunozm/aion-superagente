//! Plano de control de AION (Axum). Gestiona auth, billing y licencias firmadas.
//! Principio: solo metadatos; NUNCA contenido cognitivo del usuario.

mod auth;
mod billing;
mod routes;
mod state;
mod store;

use aion_control_client::LicenseIssuer;
use state::AppState;
use std::sync::Arc;
use store::FileStore;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("AION_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let jwt_secret = std::env::var("AION_JWT_SECRET")
        .unwrap_or_else(|_| "dev-insecure-secret-change-me".into())
        .into_bytes();
    let stripe_configured = std::env::var("STRIPE_SECRET_KEY").is_ok();

    // Clave de firma PERSISTENTE: la clave pública es estable entre reinicios,
    // de modo que el cliente puede validar licencias offline contra ella.
    let issuer = load_or_create_issuer();
    tracing::info!(public_key = %issuer.public_key_hex(), "clave pública de licencias (embeber en el cliente)");

    // Almacén de usuarios PERSISTENTE (las cuentas sobreviven a reinicios).
    let users_path = std::env::var("AION_USERS").unwrap_or_else(|_| {
        std::env::var("HOME")
            .map(|h| format!("{h}/Library/Application Support/AION/users.jsonl"))
            .unwrap_or_else(|_| "data/users.jsonl".into())
    });
    let store = FileStore::open(&users_path);
    tracing::info!(%users_path, usuarios = store.len(), "store de usuarios cargado");

    let state = AppState {
        store: Arc::new(store),
        jwt_secret: Arc::new(jwt_secret),
        issuer: Arc::new(issuer),
        stripe_configured,
    };

    let app = routes::router(state);
    let addr = std::env::var("AION_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    tracing::info!(%addr, stripe = stripe_configured, "control-plane escuchando");
    axum::serve(listener, app).await.expect("serve");
}

/// Carga la clave de firma de licencias desde disco (o `AION_LICENSE_KEY`),
/// generándola y guardándola la primera vez. Garantiza una clave pública estable.
fn load_or_create_issuer() -> LicenseIssuer {
    if let Ok(hexk) = std::env::var("AION_LICENSE_KEY") {
        if let Ok(issuer) = LicenseIssuer::from_hex(&hexk) {
            return issuer;
        }
    }
    let path = std::env::var("AION_LICENSE_KEY_FILE").unwrap_or_else(|_| {
        std::env::var("HOME")
            .map(|h| format!("{h}/Library/Application Support/AION/license_signing_key.hex"))
            .unwrap_or_else(|_| "data/license_signing_key.hex".into())
    });
    if let Ok(hexk) = std::fs::read_to_string(&path) {
        if let Ok(issuer) = LicenseIssuer::from_hex(hexk.trim()) {
            return issuer;
        }
    }
    let issuer = LicenseIssuer::generate();
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&path, issuer.signing_key_hex()).is_ok() {
        tracing::info!(%path, "clave de firma de licencias generada y persistida");
    } else {
        tracing::warn!("no se pudo persistir la clave de firma; será efímera");
    }
    issuer
}
