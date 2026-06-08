//! Billing (Stripe). Scaffolding: los handlers existen pero se activan solo si
//! `STRIPE_SECRET_KEY` está configurada. Sin clave → 501 Not Implemented.
//!
//! Flujo objetivo (F1): POST /billing/checkout crea Stripe Checkout Session →
//! webhook /billing/webhook (verificado + idempotente vía billing_events) →
//! actualiza subscription y emite licencia firmada.

use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

/// Crea una sesión de checkout (stub hasta tener claves Stripe).
pub async fn checkout(State(st): State<AppState>) -> impl IntoResponse {
    if !st.stripe_configured {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({"error": "Stripe no configurado. Define STRIPE_SECRET_KEY."})),
        );
    }
    // TODO(F1): crear Checkout Session real con stripe-rust.
    (
        StatusCode::OK,
        Json(json!({"checkout_url": "https://checkout.stripe.com/..."})),
    )
}

/// Recibe webhooks de Stripe (idempotentes). Stub hasta tener claves.
pub async fn webhook(State(st): State<AppState>) -> impl IntoResponse {
    if !st.stripe_configured {
        return (StatusCode::NOT_IMPLEMENTED, "Stripe no configurado");
    }
    // TODO(F1): verificar firma del webhook + idempotencia + actualizar suscripción.
    (StatusCode::OK, "ok")
}
