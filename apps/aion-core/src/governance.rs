//! **Gobernanza de capacidades** — la PUERTA por la que pasa toda acción con poder de AION.
//!
//! Informada por la investigación 2026-06 (CaMeL/DeepMind: contenido externo es DATOS, nunca
//! órdenes, con un modelo de CAPACIDADES de mínimo privilegio; Anthropic: feedback ambiental +
//! *stopping conditions*/circuit breakers contra runaway/drift; OWASP MCP05: toda salida del
//! agente se sanea antes de tocar el sistema). Cuanto más cuerpo gana AION (computador,
//! dispositivos, redes), más estricta esta puerta.
//!
//! Cada acción autónoma pide permiso ANTES de ejecutarse: `request(cap, "qué voy a hacer")`.
//! Devuelve una decisión (Allow / AskAriel / Deny) según la política de la capacidad y un
//! circuit breaker anti-runaway, y la DEJA registrada en la auditoría. Las capacidades sensibles
//! (computador, bluetooth, USB, conectar a la red, shell) son AskAriel por defecto: human-in-the-
//! loop. Las de solo-lectura/bajo riesgo (investigar en la web, descubrir dispositivos, sensores)
//! son Allow. Hoy solo "investigar" pasa por aquí de verdad; el resto queda listo para los anillos
//! 2-3 (computador, dispositivos/redes), que se conectarán a esta misma puerta.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Lo que AION puede llegar a hacer. El nombre estable (`as_str`) es la clave en política/auditoría.
/// Varias variantes son scaffolding de los anillos 2-3 (computador, dispositivos/redes): la puerta
/// ya las contempla aunque aún no haya código que las ejerza.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Capability {
    /// Investigar en la web (solo lectura). Anillo 1.
    Research,
    /// Investigación PROFUNDA autónoma (pipeline multi-fuente, ~5 min y decenas de llamadas LLM).
    /// Capacidad propia con tope bajo: una deep research no debe gastar el mismo cupo que una
    /// búsqueda ligera (revisión 2026-06). Anillo 1.
    DeepResearch,
    /// Descubrir dispositivos/servicios en la red local (solo lectura). Anillo 3.
    NetworkDiscover,
    /// Enumerar dispositivos conectados (USB/BLE: listar ids y nombres, sin abrirlos). Solo lectura.
    DeviceList,
    /// Leer sensores del host (batería, térmica, etc.). Bajo riesgo.
    SensorRead,
    /// PERCIBIR el computador (qué apps están abiertas / en primer plano). Solo lectura. Anillo 2.
    ComputerRead,
    /// Conectarse a un dispositivo de la red (SSH, API local, IoT). Anillo 3, sensible.
    NetworkConnect,
    /// Controlar el computador y sus apps (Accessibility/teclado/ratón). Anillo 2, sensible.
    Computer,
    /// Bluetooth/BLE. Anillo 3, sensible.
    Bluetooth,
    /// USB. Anillo 3, sensible.
    Usb,
    /// Ejecutar shell. Sensible (OWASP MCP05).
    Shell,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Research => "research",
            Capability::DeepResearch => "research.deep",
            Capability::NetworkDiscover => "network.discover",
            Capability::DeviceList => "device.list",
            Capability::SensorRead => "sensor.read",
            Capability::ComputerRead => "computer.read",
            Capability::NetworkConnect => "network.connect",
            Capability::Computer => "computer",
            Capability::Bluetooth => "bluetooth",
            Capability::Usb => "usb",
            Capability::Shell => "shell",
        }
    }
}

/// Veredicto de la puerta para una acción concreta.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Adelante.
    Allow,
    /// Necesita el visto bueno de Ariel (human-in-the-loop): se le pregunta por la Bandeja.
    AskAriel,
    /// Bloqueado (política o circuit breaker).
    Deny,
}

impl Decision {
    pub fn allowed(self) -> bool {
        self == Decision::Allow
    }
    fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::AskAriel => "ask",
            Decision::Deny => "deny",
        }
    }
}

/// Política por defecto (mínimo privilegio): solo-lectura/bajo riesgo = Allow; lo que toca el
/// mundo (computador, dispositivos, conectar, shell) = AskAriel. Nada es Deny duro por defecto
/// —Ariel manda—, pero se puede endurecer por capacidad vía `AION_DENY` (lista separada por comas).
fn base_policy(cap: Capability) -> Decision {
    if std::env::var("AION_DENY")
        .map(|s| s.split(',').any(|c| c.trim() == cap.as_str()))
        .unwrap_or(false)
    {
        return Decision::Deny;
    }
    match cap {
        Capability::Research
        | Capability::DeepResearch
        | Capability::NetworkDiscover
        | Capability::DeviceList
        | Capability::SensorRead
        | Capability::ComputerRead => Decision::Allow,
        Capability::NetworkConnect
        | Capability::Computer
        | Capability::Bluetooth
        | Capability::Usb
        | Capability::Shell => Decision::AskAriel,
    }
}

/// Circuit breaker (anti-runaway): (máximo de acciones, ventana en segundos) por capacidad.
/// Evita que un bucle autónomo se desboque. La investigación autónoma se acota fuerte.
fn rate_limit(cap: Capability) -> (usize, i64) {
    match cap {
        Capability::Research => (6, 3600), // 6/hora (búsqueda ligera)
        Capability::DeepResearch => (2, 86_400), // 2/día (pesada: ~5 min, decenas de LLM)
        Capability::NetworkDiscover => (12, 3600), // 12/hora
        Capability::DeviceList => (60, 3600), // enumerar: barato, frecuente
        Capability::SensorRead => (240, 3600), // sensores: frecuente, barato
        Capability::ComputerRead => (120, 3600), // percibir apps: barato
        _ => (30, 3600),                   // sensibles: tope de cortesía (igual piden HITL)
    }
}

fn breaker() -> &'static Mutex<HashMap<&'static str, Vec<i64>>> {
    static B: OnceLock<Mutex<HashMap<&'static str, Vec<i64>>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(HashMap::new()))
}

/// ¿Queda cupo para esta capacidad en su ventana? Registra el intento si lo hay.
fn within_rate(cap: Capability) -> bool {
    let (max, window) = rate_limit(cap);
    let now = chrono::Utc::now().timestamp();
    let mut g = breaker().lock().unwrap_or_else(|e| e.into_inner());
    let hits = g.entry(cap.as_str()).or_default();
    hits.retain(|&t| now - t < window);
    if hits.len() >= max {
        return false;
    }
    hits.push(now);
    true
}

/// **La puerta.** Pide permiso para una acción de `cap`, describiendo qué se va a hacer.
/// Audita SIEMPRE la decisión. Si es AskAriel, deja la pregunta en la Bandeja (HITL).
pub fn request(cap: Capability, action: &str) -> Decision {
    let mut decision = base_policy(cap);

    // Circuit breaker: incluso lo permitido tiene tope anti-desborde.
    if decision == Decision::Allow && !within_rate(cap) {
        decision = Decision::Deny;
        audit(cap, "deny:circuit-breaker", action);
        return decision;
    }

    audit(cap, decision.as_str(), action);

    if decision == Decision::AskAriel {
        // Human-in-the-loop: AION le pregunta a Ariel por la Bandeja antes de actuar.
        if let Ok(ibx) = crate::inbox::Inbox::open(crate::inbox_path()) {
            let _ = ibx.push(
                "permiso",
                &format!("¿Me autorizas a {action}? (capacidad: {})", cap.as_str()),
            );
        }
    }
    decision
}

fn audit(cap: Capability, decision: &str, action: &str) {
    let log = aion_telemetry::AuditLog::default_local();
    log.record("gobernanza", cap.as_str(), format!("{decision}: {action}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lectura_es_allow_y_sensibles_piden_hitl() {
        assert_eq!(base_policy(Capability::Research), Decision::Allow);
        assert_eq!(base_policy(Capability::NetworkDiscover), Decision::Allow);
        assert_eq!(base_policy(Capability::Computer), Decision::AskAriel);
        assert_eq!(base_policy(Capability::Shell), Decision::AskAriel);
    }

    #[test]
    fn circuit_breaker_corta_tras_el_tope() {
        // SensorRead tiene tope alto pero finito; agotarlo debe cortar.
        let (max, _) = rate_limit(Capability::SensorRead);
        for _ in 0..max {
            assert!(within_rate(Capability::SensorRead));
        }
        assert!(!within_rate(Capability::SensorRead)); // el siguiente ya no cabe
    }

    #[test]
    fn decision_allowed_helper() {
        assert!(Decision::Allow.allowed());
        assert!(!Decision::AskAriel.allowed());
        assert!(!Decision::Deny.allowed());
    }
}
