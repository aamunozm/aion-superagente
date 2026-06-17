//! **Sensores del entorno** (conciencia situacional): clima y ubicación aproximada,
//! SIEMPRE opt-in y local-first. Desactivados por defecto. Cuando Ariel los activa,
//! AION sabe dónde está y qué tiempo hace —contexto que un compañero real tiene— sin
//! que nada salga del dispositivo salvo la consulta de clima, que respeta `AION_PROXY`
//! (Tor/VPN) igual que el resto del tráfico web. El resultado es EFÍMERO: se cachea en
//! memoria, nunca se persiste en la memoria de largo plazo (es estado, no conocimiento).

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SensorConfig {
    /// ¿Conciencia de ubicación/clima activada? (opt-in explícito de Ariel).
    #[serde(default)]
    pub enabled: bool,
    /// Latitud/longitud fijadas por el usuario (privacidad: él decide la precisión).
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    /// Etiqueta legible del lugar (p. ej. "Roma"), para el prompt.
    #[serde(default)]
    pub place: String,
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("sensors.json")
}

pub fn load() -> SensorConfig {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &SensorConfig) {
    if let Ok(body) = serde_json::to_string_pretty(cfg) {
        crate::write_atomic(&path(), &body);
    }
}

/// Clima cacheado (texto listo para el prompt, epoch de captura). El clima cambia
/// despacio: una consulta por hora basta y sobra.
fn weather_cache() -> &'static Mutex<Option<(i64, String)>> {
    static C: std::sync::OnceLock<Mutex<Option<(i64, String)>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

const WEATHER_TTL: i64 = 3600;

/// Refresca el clima si hace falta (asíncrono, fuera del camino crítico). Usa
/// Open-Meteo: sin API key, gratis, y sale por `AION_PROXY` vía `WebClient`.
pub async fn refresh_weather() {
    let cfg = load();
    let (Some(lat), Some(lon)) = (cfg.lat, cfg.lon) else {
        return;
    };
    if !cfg.enabled {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    if let Some((ts, _)) = weather_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        if now - ts < WEATHER_TTL {
            return;
        }
    }
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.3}&longitude={lon:.3}\
         &current=temperature_2m,weather_code,wind_speed_10m"
    );
    // WebClient respeta AION_PROXY (Tor/VPN): la ubicación no filtra la IP real.
    let client = aion_browser::WebClient::new();
    let Ok(body) = client.fetch_raw(&url).await else {
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
        return;
    };
    let cur = &v["current"];
    let temp = cur["temperature_2m"].as_f64();
    let code = cur["weather_code"].as_i64().unwrap_or(-1);
    if let Some(t) = temp {
        let desc = weather_desc(code);
        let txt = format!("{desc}, {t:.0}°C");
        *weather_cache().lock().unwrap_or_else(|e| e.into_inner()) = Some((now, txt));
    }
}

/// Bloque para el prompt (vacío si está desactivado o aún sin datos). Es ESTADO
/// volátil: va al final del prompt y nunca se memoriza.
pub fn note() -> String {
    note_from(&load())
}

/// Construye la nota a partir de una config DADA — función pura (no toca disco), para poder
/// testear la lógica de forma aislada (antes el test leía `sensors.json` real del Mac y fallaba
/// en la máquina de Ariel con los sensores activados).
fn note_from(cfg: &SensorConfig) -> String {
    if !cfg.enabled {
        return String::new();
    }
    let mut b = String::new();
    if !cfg.place.is_empty() {
        b.push_str(&format!("DÓNDE ESTÁS: {}.", cfg.place));
    } else if cfg.lat.is_some() && cfg.lon.is_some() {
        // Solo coordenadas (sin etiqueta de ciudad): aun así AION SABE que tiene tu
        // ubicación precisa — no debe pedirte la ciudad para el clima.
        b.push_str(
            "DÓNDE ESTÁS: tu ubicación precisa está configurada (úsala para el clima; no pidas la ciudad).",
        );
    }
    if let Some((_, w)) = weather_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        b.push_str(&format!(" Tiempo ahora: {w}."));
    }
    if b.is_empty() {
        String::new()
    } else {
        b.push_str(" Es contexto efímero (no lo memorices); úsalo si viene al caso.\n\n");
        b
    }
}

/// Mapea el código WMO de Open-Meteo a una descripción breve en español.
fn weather_desc(code: i64) -> &'static str {
    match code {
        0 => "despejado",
        1 | 2 => "parcialmente nublado",
        3 => "nublado",
        45 | 48 => "niebla",
        51 | 53 | 55 => "llovizna",
        61 | 63 | 65 => "lluvia",
        66 | 67 => "lluvia helada",
        71 | 73 | 75 | 77 => "nieve",
        80..=82 => "chubascos",
        85 | 86 => "chubascos de nieve",
        95 => "tormenta",
        96 | 99 => "tormenta con granizo",
        _ => "tiempo variable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default() {
        assert!(!SensorConfig::default().enabled);
    }

    #[test]
    fn note_empty_when_disabled() {
        // Lógica PURA (sin leer disco): desactivado => sin bloque. Antes esto llamaba a note(),
        // que lee el sensors.json real → fallaba en el Mac de Ariel con los sensores activados.
        assert_eq!(note_from(&SensorConfig::default()), "");
    }

    #[test]
    fn weather_codes_map() {
        assert_eq!(weather_desc(0), "despejado");
        assert_eq!(weather_desc(95), "tormenta");
        assert_eq!(weather_desc(1234), "tiempo variable");
    }
}
