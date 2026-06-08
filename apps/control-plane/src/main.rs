//! Plano de control de AION (Axum). Gestiona auth, billing y licencias firmadas.
//! Principio: solo metadatos; NUNCA contenido cognitivo del usuario.

mod auth;
mod billing;
mod license;
mod routes;
mod state;
mod store;

use license::LicenseIssuer;
use state::AppState;
use std::sync::Arc;
use store::InMemoryStore;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("AION_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let jwt_secret = std::env::var("AION_JWT_SECRET")
        .unwrap_or_else(|_| "dev-insecure-secret-change-me".into())
        .into_bytes();
    let stripe_configured = std::env::var("STRIPE_SECRET_KEY").is_ok();

    let issuer = LicenseIssuer::generate();
    tracing::info!(public_key = %issuer.public_key_hex(), "clave pública de licencias (embeber en el cliente)");

    let state = AppState {
        store: Arc::new(InMemoryStore::default()),
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
