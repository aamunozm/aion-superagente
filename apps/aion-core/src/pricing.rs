//! **Precios de tokens + tipos de cambio** para el panel de coste/ahorro de Claude Code.
//!
//! Honestidad (mandato de Ariel):
//! - **Precios por token**: Anthropic NO tiene API pública de precios. Se SIEMBRAN con los valores
//!   vigentes conocidos y son EDITABLES desde la UI (`/api/claude-code/prices`). Eso es "siempre
//!   actualizado" honesto: el usuario los mantiene al día; nunca se finge un auto-fetch inexistente.
//! - **Tipos de cambio (FX)**: SÍ en tiempo real, vía API gratuita (open.er-api.com), cacheados 12h.
//!   Fail-open a la última caché o a valores razonables si no hay red.
//! El coste se calcula sobre INPUT tokens (lo que el puente inyecta en Claude) — el lado del coste
//! que AION conoce y puede medir.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Precio de un modelo en USD por 1.000.000 de tokens (entrada/salida).
#[derive(Serialize, Deserialize, Clone)]
pub struct ModelPrice {
    pub model: String,
    pub label: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Prices {
    pub models: Vec<ModelPrice>,
    /// Nota de vigencia (editable). El usuario la actualiza al cambiar precios.
    pub updated_at: String,
}

fn prices_path() -> PathBuf {
    crate::app_data_dir().join("token_prices.json")
}

/// Precios SEMBRADOS (USD/1M). Valores conocidos a 2026-06; EDITABLES en la UI. El usuario debe
/// verificarlos contra la tabla oficial de Anthropic, que es la fuente de verdad.
pub fn defaults() -> Prices {
    Prices {
        updated_at: "sembrado 2026-06 · edítalos con los precios vigentes de Anthropic".into(),
        models: vec![
            ModelPrice {
                model: "claude-opus".into(),
                label: "Claude Opus 4.x".into(),
                input_per_m: 15.0,
                output_per_m: 75.0,
            },
            ModelPrice {
                model: "claude-sonnet".into(),
                label: "Claude Sonnet 4.x".into(),
                input_per_m: 3.0,
                output_per_m: 15.0,
            },
            ModelPrice {
                model: "claude-haiku".into(),
                label: "Claude Haiku 4.x".into(),
                input_per_m: 0.80,
                output_per_m: 4.0,
            },
            ModelPrice {
                model: "deepseek".into(),
                label: "DeepSeek (proveedor externo)".into(),
                input_per_m: 0.27,
                output_per_m: 1.10,
            },
            ModelPrice {
                model: "local".into(),
                label: "Gemma local (AION) — gratis".into(),
                input_per_m: 0.0,
                output_per_m: 0.0,
            },
        ],
    }
}

pub fn load() -> Prices {
    std::fs::read_to_string(prices_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(defaults)
}

pub fn save(p: &Prices) {
    if let Ok(j) = serde_json::to_string_pretty(p) {
        crate::write_atomic(&prices_path(), &j);
    }
}

// ── Tipos de cambio (FX) en tiempo real ─────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct Fx {
    /// 1 USD = `usd_eur` EUR.
    pub usd_eur: f64,
    /// 1 USD = `usd_clp` CLP.
    pub usd_clp: f64,
    pub fetched_at: String,
    /// true = recién obtenido de la API; false = caché vieja o valores por defecto (sin red).
    pub live: bool,
}

fn fx_path() -> PathBuf {
    crate::app_data_dir().join("fx_rates.json")
}

fn read_fx() -> Option<Fx> {
    std::fs::read_to_string(fx_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

/// ¿La caché FX tiene < 12 h? (los tipos de cambio se mueven despacio; 12h sobra para esto).
fn fresh(fetched_at: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(fetched_at)
        .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_hours() < 12)
        .unwrap_or(false)
}

async fn fetch_fx() -> Option<Fx> {
    let v: serde_json::Value = reqwest::Client::new()
        .get("https://open.er-api.com/v6/latest/USD")
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let eur = v["rates"]["EUR"].as_f64()?;
    let clp = v["rates"]["CLP"].as_f64()?;
    if eur <= 0.0 || clp <= 0.0 {
        return None;
    }
    Some(Fx {
        usd_eur: eur,
        usd_clp: clp,
        fetched_at: chrono::Utc::now().to_rfc3339(),
        live: true,
    })
}

/// Tipos de cambio actuales: caché si < 12h, si no los re-obtiene de la API (fail-open a la caché
/// vieja marcada `live:false`, o a valores de respaldo si nunca hubo red).
pub async fn fx() -> Fx {
    if let Some(c) = read_fx() {
        if fresh(&c.fetched_at) {
            return c;
        }
    }
    match fetch_fx().await {
        Some(f) => {
            if let Ok(j) = serde_json::to_string_pretty(&f) {
                crate::write_atomic(&fx_path(), &j);
            }
            f
        }
        None => read_fx()
            .map(|mut c| {
                c.live = false;
                c
            })
            .unwrap_or(Fx {
                usd_eur: 0.92,
                usd_clp: 955.0,
                fetched_at: String::new(),
                live: false,
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_models_and_local_is_free() {
        let p = defaults();
        assert!(p.models.len() >= 4);
        let local = p.models.iter().find(|m| m.model == "local").unwrap();
        assert_eq!(local.input_per_m, 0.0);
    }

    #[test]
    fn fresh_rejects_old_and_garbage() {
        assert!(!fresh(""));
        assert!(!fresh("2020-01-01T00:00:00+00:00"));
        assert!(fresh(&chrono::Utc::now().to_rfc3339()));
    }
}
