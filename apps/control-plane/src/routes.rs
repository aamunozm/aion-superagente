//! Rutas HTTP de auth y licencias.

use crate::auth;
use crate::billing;
use crate::state::AppState;
use aion_control_client::LicenseClaims;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

const JWT_TTL_SECS: i64 = 3600;

pub fn router(state: AppState) -> Router {
    // CORS abierto: la UI (app Tauri / web) hace fetch desde otro origen.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/reset", post(reset))
        .route("/auth/me", get(me))
        .route("/billing/license", get(license))
        .route("/billing/checkout", post(billing::checkout))
        .route("/billing/webhook", post(billing::webhook))
        .layer(cors)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

#[derive(Deserialize)]
pub struct Credentials {
    email: String,
    password: String,
}

async fn register(State(st): State<AppState>, Json(body): Json<Credentials>) -> impl IntoResponse {
    if body.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "contraseña mínima 8 caracteres"})),
        )
            .into_response();
    }
    let hash = match auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response()
        }
    };
    match st.store.create_user(&body.email, &hash) {
        Ok(user) => {
            // Código de recuperación (local-first, sin email): se muestra UNA vez y
            // solo se guarda su hash. Es la forma de recuperar la contraseña.
            let recovery_code = gen_recovery_code();
            if let Ok(rhash) = auth::hash_password(&recovery_code) {
                let _ = st.store.set_recovery(&user.id, &rhash);
            }
            let token = auth::issue_jwt(&st.jwt_secret, &user.id, &user.email, JWT_TTL_SECS)
                .unwrap_or_default();
            (
                StatusCode::CREATED,
                Json(json!({
                    "id": user.id, "email": user.email, "token": token,
                    "recovery_code": recovery_code
                })),
            )
                .into_response()
        }
        Err(e) => (StatusCode::CONFLICT, Json(json!({"error": e}))).into_response(),
    }
}

/// Genera un código de recuperación legible tipo XXXX-XXXX-XXXX.
fn gen_recovery_code() -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string().to_uppercase();
    format!("{}-{}-{}", &raw[0..4], &raw[4..8], &raw[8..12])
}

#[derive(Deserialize)]
pub struct ResetReq {
    email: String,
    recovery_code: String,
    new_password: String,
}

/// Recuperación: email + código de recuperación → nueva contraseña.
async fn reset(State(st): State<AppState>, Json(body): Json<ResetReq>) -> impl IntoResponse {
    if body.new_password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "contraseña mínima 8 caracteres"})),
        )
            .into_response();
    }
    let user = match st.store.find_by_email(&body.email) {
        Some(u) => u,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "email o código inválido"})))
                .into_response()
        }
    };
    let code = body.recovery_code.trim().to_uppercase();
    if user.recovery_hash.is_empty() || !auth::verify_password(&code, &user.recovery_hash) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "email o código inválido"})))
            .into_response();
    }
    let new_hash = match auth::hash_password(&body.new_password) {
        Ok(h) => h,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response()
        }
    };
    match st.store.update_password(&user.id, &new_hash) {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn login(State(st): State<AppState>, Json(body): Json<Credentials>) -> impl IntoResponse {
    let user = match st.store.find_by_email(&body.email) {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "credenciales inválidas"})),
            )
                .into_response()
        }
    };
    if !auth::verify_password(&body.password, &user.password_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "credenciales inválidas"})),
        )
            .into_response();
    }
    let token =
        auth::issue_jwt(&st.jwt_secret, &user.id, &user.email, JWT_TTL_SECS).unwrap_or_default();
    (
        StatusCode::OK,
        Json(json!({"id": user.id, "email": user.email, "token": token})),
    )
        .into_response()
}

/// Extrae y valida el bearer token, devolviendo el user id.
fn authed_user_id(st: &AppState, headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "falta bearer token".into()))?;
    let claims =
        auth::verify_jwt(&st.jwt_secret, token).map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
    Ok(claims.sub)
}

async fn me(State(st): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match authed_user_id(&st, &headers) {
        Ok(uid) => match st.store.find_by_id(&uid) {
            Some(u) => (
                StatusCode::OK,
                Json(json!({"id": u.id, "email": u.email, "tier": u.tier})),
            )
                .into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "usuario no encontrado"})),
            )
                .into_response(),
        },
        Err((code, msg)) => (code, Json(json!({"error": msg}))).into_response(),
    }
}

/// Emite la licencia firmada (Ed25519) del usuario autenticado, validable offline.
async fn license(State(st): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let uid = match authed_user_id(&st, &headers) {
        Ok(u) => u,
        Err((code, msg)) => return (code, Json(json!({"error": msg}))).into_response(),
    };
    let user = match st.store.find_by_id(&uid) {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "usuario no encontrado"})),
            )
                .into_response()
        }
    };
    let claims = LicenseClaims {
        user_id: user.id.clone(),
        tier: user.tier.clone(),
        seats: 1,
        valid_until: (chrono::Utc::now() + chrono::Duration::days(30)).timestamp(),
    };
    match st.issuer.issue(claims) {
        Ok(lic) => (StatusCode::OK, Json(lic)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}
