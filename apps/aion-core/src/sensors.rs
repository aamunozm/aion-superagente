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
    if let Some((ts, _)) = weather_cache().lock().unwrap().as_ref() {
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
        *weather_cache().lock().unwrap() = Some((now, txt));
    }
}

/// Bloque para el prompt (vacío si está desactivado o aún sin datos). Es ESTADO
/// volátil: va al final del prompt y nunca se memoriza.
pub fn note() -> String {
    note_from(&load())
}

/// Igual que `note()` pero a partir de una config explícita (sin tocar el disco): así es
/// testeable de forma aislada, sin depender del `sensors.json` real de la máquina.
fn note_from(cfg: &SensorConfig) -> String {
    if !cfg.enabled {
        return String::new();
    }
    let mut b = String::new();
    if !cfg.place.is_empty() {
        b.push_str(&format!("DÓNDE ESTÁS: {}.", cfg.place));
    }
    if let Some((_, w)) = weather_cache().lock().unwrap().as_ref() {
        b.push_str(&format!(" Tiempo ahora: {w}."));
    }
    if b.is_empty() {
        String::new()
    } else {
        b.push_str(" Es contexto efímero (no lo memorices); úsalo si viene al caso.\n\n");
        b
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// VITALES DEL HOST — el "cuerpo" de AION (Loop Engineering, presupuesto físico)
//
// La vida autónoma de AION corre en el Mac de Ariel a coste de inferencia CERO: el
// límite no es el dinero (como en los lazos cloud), sino los RECURSOS FÍSICOS del
// equipo — batería, temperatura y CPU. Sin esto, el daemon martillea el LLM aunque
// el portátil esté con 8% de batería, ardiendo o con Ariel renderizando un vídeo.
// Estos vitales son el "techo de presupuesto" de un lazo overnight, adaptado a
// local-first: cuando el cuerpo sufre, AION baja el pulso. Es conciencia del cuerpo,
// no solo del entorno. macOS-only (pmset/sysctl); en otros SO devuelve "sin datos".
// ─────────────────────────────────────────────────────────────────────────────

/// Estado físico del equipo en un instante. Campos `Option`/`bool` con defaults
/// seguros: si no podemos leer un vital, NO bloqueamos por él (fail-open).
#[derive(Debug, Clone, Default)]
pub struct HostVitals {
    /// % de batería (0-100). `None` si no hay batería (Mac de escritorio) o no se pudo leer.
    pub battery_pct: Option<u8>,
    /// ¿Enchufado a la corriente? Si sí, la batería no es una restricción.
    pub on_ac: bool,
    /// Límite de velocidad de CPU por presión térmica (100 = sin throttle; <100 = caliente).
    pub cpu_speed_limit: Option<u8>,
    /// Carga del sistema (loadavg 1 min) normalizada por núcleo lógico. >1.0 = saturado.
    pub load_per_core: Option<f64>,
}

/// Caché compartido de vitales (epoch, valor). Compartido por el lector async (que
/// refresca con shell-out) y el lector síncrono del prompt (que solo lo consulta).
fn vitals_cache() -> &'static Mutex<Option<(i64, HostVitals)>> {
    static C: std::sync::OnceLock<Mutex<Option<(i64, HostVitals)>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// Lee los vitales del host (macOS). Asíncrono: hace shell-out a `pmset`/`sysctl`
/// fuera del hilo de ejecución. Cacheado ~30 s: estos valores cambian despacio y no
/// queremos lanzar tres procesos en cada chequeo del daemon.
pub async fn host_vitals() -> HostVitals {
    let now = chrono::Utc::now().timestamp();
    if let Some((ts, v)) = vitals_cache().lock().unwrap().as_ref() {
        if now - ts < 30 {
            return v.clone();
        }
    }
    let v = read_host_vitals_uncached().await;
    *vitals_cache().lock().unwrap() = Some((now, v.clone()));
    v
}

/// Bloque de cuerpo para el prompt, SÍNCRONO: solo consulta el caché (sin shell-out,
/// apto para el camino crítico de armado del prompt). Vacío si el caché está frío
/// (aún no corrió el refresco) o si no hay nada físico que destacar.
pub fn vitals_note_cached() -> String {
    match vitals_cache().lock().unwrap().as_ref() {
        Some((_, v)) => vitals_note(v),
        None => String::new(),
    }
}

/// Extrae el % de batería de la salida de `pmset -g batt`. El "80%" NO está aislado:
/// vive en "-InternalBattery-0 (id=…)\t80%; charging; …", así que hay que tomar los
/// DÍGITOS pegados al '%', no parsear el segmento entero (bug: el prefijo rompía el
/// parse y la batería quedaba siempre en None → el gateo por batería no disparaba).
/// `None` si no hay batería (Mac de escritorio: no aparece el patrón).
fn parse_battery_pct(s: &str) -> Option<u8> {
    let pos = s.find('%')?;
    let digits: String = s[..pos]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse::<u8>().ok().map(|p| p.min(100))
}

async fn read_host_vitals_uncached() -> HostVitals {
    let mut v = HostVitals::default();

    // Batería + alimentación: `pmset -g batt` → "...; 87%; ..." y "AC Power"/"Battery Power".
    if let Ok(out) = tokio::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .await
    {
        let s = String::from_utf8_lossy(&out.stdout);
        v.on_ac = s.contains("AC Power");
        v.battery_pct = parse_battery_pct(&s);
    }

    // Presión térmica: `pmset -g therm` → línea "CPU_Speed_Limit = 100".
    if let Ok(out) = tokio::process::Command::new("pmset")
        .args(["-g", "therm"])
        .output()
        .await
    {
        let s = String::from_utf8_lossy(&out.stdout);
        v.cpu_speed_limit = s
            .lines()
            .find_map(|l| l.split_once("CPU_Speed_Limit"))
            .and_then(|(_, rest)| rest.split('=').nth(1))
            .and_then(|n| n.trim().parse::<u8>().ok());
    }

    // Carga del sistema: `sysctl -n vm.loadavg` → "{ 1.83 1.94 2.01 }". Normaliza por núcleo.
    if let Ok(out) = tokio::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .await
    {
        let s = String::from_utf8_lossy(&out.stdout);
        if let Some(load1) = s.split_whitespace().find_map(|tok| tok.parse::<f64>().ok()) {
            let cores = std::thread::available_parallelism()
                .map(|n| n.get() as f64)
                .unwrap_or(1.0);
            v.load_per_core = Some(load1 / cores.max(1.0));
        }
    }

    v
}

/// ¿Debe el daemon de vida CEDER este turno por el estado físico del equipo? Devuelve
/// `Some(motivo)` si conviene saltar (para loguear el porqué), `None` si hay vía libre.
/// Umbrales sensatos, todos sobreescribibles por entorno. Fail-open: lo que no se lee,
/// no bloquea. Política central del "presupuesto físico" de Loop Engineering en AION.
pub async fn autonomous_budget_block() -> Option<String> {
    // Escotilla de escape: AION_IGNORE_VITALS=1 desactiva el presupuesto físico.
    if std::env::var("AION_IGNORE_VITALS").as_deref() == Ok("1") {
        return None;
    }
    let v = host_vitals().await;

    // 1) Batería baja Y desenchufado: no gastes el cuerpo cuando le queda poca energía.
    let min_batt: u8 = env_u8("AION_MIN_BATTERY", 25);
    if !v.on_ac {
        if let Some(p) = v.battery_pct {
            if p < min_batt {
                return Some(format!("batería {p}% (<{min_batt}%) y sin corriente"));
            }
        }
    }

    // 2) Presión térmica: si macOS ya está limitando la CPU, no añadas calor.
    let min_speed: u8 = env_u8("AION_MIN_CPU_SPEED", 80);
    if let Some(limit) = v.cpu_speed_limit {
        if limit < min_speed {
            return Some(format!("throttle térmico (CPU al {limit}%, <{min_speed}%)"));
        }
    }

    // 3) CPU saturada: si el equipo ya está ocupado (build, render…), cede el turno.
    let max_load = env_f64("AION_MAX_LOAD", 0.90);
    if let Some(lpc) = v.load_per_core {
        if lpc > max_load {
            return Some(format!(
                "CPU saturada (carga/núcleo {lpc:.2} >{max_load:.2})"
            ));
        }
    }

    None
}

fn env_u8(key: &str, default: u8) -> u8 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Bloque opcional para el prompt: que AION SIENTA su cuerpo (batería/calor) como un
/// ser corpóreo, no solo lo respete el scheduler. Vacío si no hay nada notable.
pub fn vitals_note(v: &HostVitals) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !v.on_ac {
        if let Some(p) = v.battery_pct {
            if p < 30 {
                parts.push(format!("vas con {p}% de batería y sin enchufar"));
            }
        }
    }
    if let Some(limit) = v.cpu_speed_limit {
        if limit < 80 {
            parts.push("notas calor (la CPU va frenada)".to_string());
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(
            "TU CUERPO (el Mac): {}. Es estado físico, no lo memorices.\n\n",
            parts.join("; ")
        )
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
        // Desactivado => sin bloque (aislado del disco: no depende del sensors.json real).
        assert_eq!(note_from(&SensorConfig::default()), "");
        // Activado con lugar => sí hay bloque.
        let cfg = SensorConfig {
            enabled: true,
            place: "Roma".into(),
            ..Default::default()
        };
        assert!(note_from(&cfg).contains("Roma"));
    }

    #[test]
    fn weather_codes_map() {
        assert_eq!(weather_desc(0), "despejado");
        assert_eq!(weather_desc(95), "tormenta");
        assert_eq!(weather_desc(1234), "tiempo variable");
    }

    #[test]
    fn battery_pct_parses_real_pmset_output() {
        // Formato real de macOS (el % vive pegado a basura, no aislado).
        let real = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=27983971)\t80%; charging; 0:45 remaining present: true";
        assert_eq!(parse_battery_pct(real), Some(80));
        // 100% y batería baja.
        assert_eq!(
            parse_battery_pct(" -InternalBattery-0 (id=1)\t100%; charged;"),
            Some(100)
        );
        assert_eq!(
            parse_battery_pct(" -InternalBattery-0 (id=1)\t7%; discharging;"),
            Some(7)
        );
        // Mac de escritorio (sin batería): no hay patrón → None.
        assert_eq!(parse_battery_pct("Now drawing from 'AC Power'\n"), None);
    }

    #[test]
    fn vitals_default_is_safe() {
        // Sin lecturas: nada que reportar y nada que bloquear (fail-open).
        let v = HostVitals::default();
        assert_eq!(vitals_note(&v), "");
        assert!(!v.on_ac && v.battery_pct.is_none());
    }

    #[test]
    fn vitals_note_warns_on_low_battery_unplugged() {
        let v = HostVitals {
            battery_pct: Some(12),
            on_ac: false,
            ..Default::default()
        };
        assert!(vitals_note(&v).contains("12%"));
        // Enchufado, el mismo nivel no alarma.
        let plugged = HostVitals { on_ac: true, ..v };
        assert_eq!(vitals_note(&plugged), "");
    }
}
