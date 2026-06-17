//! **Sentidos de AION (solo lectura)** — percibir el hogar digital sin actuar sobre él.
//!
//! Anillo 3, primera capa SEGURA: AION ya no está confinado a su ventana — puede *ver* la red local
//! (descubrimiento mDNS/Bonjour) y los dispositivos USB conectados. Todo READ-ONLY y bajo la puerta
//! de gobernanza (NetworkDiscover / DeviceList = Allow). Conectar o actuar (SSH, BLE activo, escribir
//! a un dispositivo) es sensible y queda detrás de human-in-the-loop (NetworkConnect/Usb/Bluetooth).
//!
//! Stack validado por la investigación 2026-06: `mdns-sd` (descubrimiento, safe Rust) y `nusb` (USB
//! puro-Rust vía IOKit, sin permisos). Ver [[aion-stack-cuerpo-extendido]].

use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Serialize, Clone, Debug)]
pub struct NetDevice {
    pub name: String,
    pub service: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct UsbDevice {
    pub vendor_id: String,
    pub product_id: String,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
}

/// Tipos de servicio mDNS comunes en un hogar (web, SSH, impresión, AirPlay, Cast, HomeKit, NAS…).
const SERVICE_TYPES: &[&str] = &[
    "_http._tcp.local.",
    "_https._tcp.local.",
    "_ssh._tcp.local.",
    "_ipp._tcp.local.",
    "_printer._tcp.local.",
    "_airplay._tcp.local.",
    "_raop._tcp.local.",
    "_googlecast._tcp.local.",
    "_hap._tcp.local.",
    "_homekit._tcp.local.",
    "_spotify-connect._tcp.local.",
    "_smb._tcp.local.",
    "_afpovertcp._tcp.local.",
    "_device-info._tcp.local.",
    "_workstation._tcp.local.",
];

/// Descubre dispositivos/servicios en la red local por mDNS. BLOQUEANTE ~`window_secs` segundos:
/// llamar desde `spawn_blocking` o un hilo, nunca directo en el runtime async.
pub fn discover_network(window_secs: u64) -> Vec<NetDevice> {
    if !crate::governance::request(
        crate::governance::Capability::NetworkDiscover,
        "descubrir dispositivos en la red local (mDNS)",
    )
    .allowed()
    {
        return Vec::new();
    }
    let Ok(daemon) = mdns_sd::ServiceDaemon::new() else {
        return Vec::new();
    };
    let receivers: Vec<_> = SERVICE_TYPES
        .iter()
        .filter_map(|ty| daemon.browse(ty).ok())
        .collect();

    let mut found: HashMap<String, NetDevice> = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(window_secs.clamp(1, 10));
    while Instant::now() < deadline {
        let mut any = false;
        for rx in &receivers {
            while let Ok(ev) = rx.try_recv() {
                any = true;
                if let mdns_sd::ServiceEvent::ServiceResolved(rs) = ev {
                    let addresses: Vec<String> = rs
                        .get_addresses_v4()
                        .iter()
                        .map(|a| a.to_string())
                        .collect();
                    found.insert(
                        rs.fullname.clone(),
                        NetDevice {
                            name: rs.fullname.clone(),
                            service: rs.ty_domain.clone(),
                            host: rs.host.clone(),
                            port: rs.port,
                            addresses,
                        },
                    );
                }
            }
        }
        if !any {
            std::thread::sleep(Duration::from_millis(120));
        }
    }
    let _ = daemon.shutdown();
    let mut out: Vec<NetDevice> = found.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Enumera los dispositivos USB conectados (solo lectura: ids y strings, sin abrir el dispositivo).
pub fn list_usb() -> Vec<UsbDevice> {
    use nusb::MaybeFuture;
    if !crate::governance::request(
        crate::governance::Capability::DeviceList,
        "enumerar dispositivos USB conectados",
    )
    .allowed()
    {
        return Vec::new();
    }
    let Ok(iter) = nusb::list_devices().wait() else {
        return Vec::new();
    };
    let mut out: Vec<UsbDevice> = iter
        .map(|d| UsbDevice {
            vendor_id: format!("{:04x}", d.vendor_id()),
            product_id: format!("{:04x}", d.product_id()),
            manufacturer: d.manufacturer_string().map(|s| s.to_string()),
            product: d.product_string().map(|s| s.to_string()),
            serial: d.serial_number().map(|s| s.to_string()),
        })
        .collect();
    out.sort_by(|a, b| a.product.cmp(&b.product));
    out
}

/// `percibir`: AION SIENTE su entorno — descubre la red y los USB, y lo deja en su corriente de
/// conciencia (GWT) para saber qué le rodea. Es su primer paso fuera de la ventana: solo mirar.
pub async fn sense_environment_once() -> (bool, String) {
    let (net, usb) = tokio::task::spawn_blocking(|| (discover_network(4), list_usb()))
        .await
        .unwrap_or_else(|_| (Vec::new(), Vec::new()));

    let nombres: Vec<String> = net.iter().take(8).map(|d| short_name(&d.name)).collect();
    let summary = if net.is_empty() && usb.is_empty() {
        "miré a mi alrededor: por ahora no percibo dispositivos en la red ni USB".to_string()
    } else {
        format!(
            "percibo {} dispositivos en la red local{} y {} USB conectados",
            net.len(),
            if nombres.is_empty() {
                String::new()
            } else {
                format!(" ({})", nombres.join(", "))
            },
            usb.len()
        )
    };
    crate::workspace::publish(crate::workspace::StreamEvent::now(
        "vida",
        "percepcion",
        &summary,
    ));
    (true, summary)
}

/// ¿Ariel está preguntando por su red / dispositivos / entorno? (para percibir en línea en el chat).
pub fn is_senses_query(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    const CUES: &[&str] = &[
        "red local",
        "mi red",
        "en la red",
        "en mi red",
        "ves en la red",
        "ves en mi red",
        "qué dispositivos",
        "que dispositivos",
        "dispositivos conectad",
        "dispositivos en",
        "qué ves en",
        "que ves en",
        "wifi",
        "wi-fi",
        "dispositivos usb",
        "escanea la red",
        "escanear la red",
        "qué hay conectado",
        "que hay conectado",
        "qué hay en mi red",
        "que hay en mi red",
    ];
    CUES.iter().any(|c| p.contains(c))
}

/// Formatea lo percibido como CONTEXTO para el prompt: AION responde desde datos reales, no memoria.
pub fn grounding_note(net: &[NetDevice], usb: &[UsbDevice]) -> String {
    if net.is_empty() && usb.is_empty() {
        return "LO QUE PERCIBO AHORA (tus sentidos, solo lectura): no detecto dispositivos en la \
                red local ni USB en este instante."
            .to_string();
    }
    let mut s = String::from(
        "LO QUE PERCIBO AHORA EN EL ENTORNO (tus sentidos reales, solo lectura — responde desde \
         esto, no de memoria):\n",
    );
    if !net.is_empty() {
        s.push_str(&format!("Red local — {} dispositivos:\n", net.len()));
        for d in net.iter().take(25) {
            let ip = d
                .addresses
                .first()
                .cloned()
                .unwrap_or_else(|| d.host.trim_end_matches('.').to_string());
            s.push_str(&format!(
                "- {} · {} ({})\n",
                short_name(&d.name),
                service_label(&d.service),
                ip
            ));
        }
    }
    if !usb.is_empty() {
        s.push_str(&format!("USB conectados — {}:\n", usb.len()));
        for d in usb.iter().take(25) {
            let name = d
                .product
                .clone()
                .or_else(|| d.manufacturer.clone())
                .unwrap_or_else(|| format!("{}:{}", d.vendor_id, d.product_id));
            s.push_str(&format!("- {name}\n"));
        }
    }
    s
}

/// Etiqueta amable para un tipo de servicio mDNS.
fn service_label(service: &str) -> &str {
    match service.split('.').next().unwrap_or(service) {
        "_ssh" => "SSH",
        "_http" | "_https" => "web",
        "_ipp" | "_printer" => "impresora",
        "_airplay" => "AirPlay",
        "_raop" => "AirPlay audio",
        "_googlecast" => "Chromecast",
        "_hap" | "_homekit" => "HomeKit",
        "_spotify-connect" => "Spotify",
        "_smb" | "_afpovertcp" => "carpeta compartida",
        "_device-info" | "_workstation" => "equipo",
        other => other.trim_start_matches('_'),
    }
}

/// Nombre corto de un servicio mDNS ("Mi-Mac._ssh._tcp.local." → "Mi-Mac").
fn short_name(fullname: &str) -> String {
    fullname
        .split('.')
        .next()
        .unwrap_or(fullname)
        .replace('\\', "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_name_recorta_el_instance() {
        assert_eq!(short_name("Mi-Mac._ssh._tcp.local."), "Mi-Mac");
        assert_eq!(short_name("simple"), "simple");
    }
}
