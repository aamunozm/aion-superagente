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
        tracing::warn!("senses: no pude iniciar el daemon mDNS (descubrimiento de red omitido)");
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
        tracing::warn!("senses: no pude enumerar USB (nusb falló)");
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

#[derive(Serialize, Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub path: String,
}

/// Discos/volúmenes montados (externos y de red aparecen bajo /Volumes). Solo lectura.
pub fn list_disks() -> Vec<DiskInfo> {
    if !crate::governance::request(
        crate::governance::Capability::DeviceList,
        "listar discos/volúmenes montados",
    )
    .allowed()
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            out.push(DiskInfo {
                path: format!("/Volumes/{name}"),
                name,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Cámaras del sistema (integrada + externas). Vía system_profiler (consulta fija de solo lectura).
/// Lenta (~1s): llamar desde spawn_blocking.
pub fn list_cameras() -> Vec<String> {
    if !crate::governance::request(
        crate::governance::Capability::DeviceList,
        "detectar cámaras del sistema",
    )
    .allowed()
    {
        return Vec::new();
    }
    let Ok(o) = std::process::Command::new("system_profiler")
        .args(["SPCameraDataType", "-json"])
        .output()
    else {
        tracing::warn!("senses: no pude consultar cámaras (system_profiler)");
        return Vec::new();
    };
    serde_json::from_slice::<serde_json::Value>(&o.stdout)
        .ok()
        .and_then(|v| {
            v.get("SPCameraDataType")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| c.get("_name").and_then(|n| n.as_str()).map(String::from))
                        .collect::<Vec<_>>()
                })
        })
        .unwrap_or_default()
}

/// `percibir`: AION SIENTE su entorno — red, USB, discos y cámara — y lo deja en su corriente de
/// conciencia (GWT) para saber qué le rodea. Su paso fuera de la ventana: solo mirar.
pub async fn sense_environment_once() -> (bool, String) {
    let (net, usb, disks, cams) = tokio::task::spawn_blocking(|| {
        (
            discover_network(4),
            list_usb(),
            list_disks(),
            list_cameras(),
        )
    })
    .await
    .unwrap_or_else(|_| (Vec::new(), Vec::new(), Vec::new(), Vec::new()));

    let nombres: Vec<String> = net.iter().take(8).map(|d| short_name(&d.name)).collect();
    let summary = if net.is_empty() && usb.is_empty() && disks.is_empty() && cams.is_empty() {
        "miré a mi alrededor: por ahora no percibo nada en la red, USB, discos ni cámara"
            .to_string()
    } else {
        format!(
            "percibo {} dispositivos en la red{}, {} USB, {} discos montados y {} cámara(s)",
            net.len(),
            if nombres.is_empty() {
                String::new()
            } else {
                format!(" ({})", nombres.join(", "))
            },
            usb.len(),
            disks.len(),
            cams.len()
        )
    };
    crate::workspace::publish(crate::workspace::StreamEvent::now(
        "vida",
        "percepcion",
        &summary,
    ));

    // 🧰 ¿Ariel instaló algo nuevo que AION pueda usar? Si sí, lo NOTA (corriente + memoria) para
    // estar siempre al día de su caja de herramientas.
    let nuevos = crate::computer::detect_new_installs();
    if !nuevos.is_empty() {
        let aviso = format!(
            "Ariel instaló algo nuevo que puedo usar: {}",
            nuevos.join(", ")
        );
        crate::workspace::publish(crate::workspace::StreamEvent::now(
            "vida",
            "pensamiento",
            &aviso,
        ));
        if let Ok(mem) = crate::shared_memory() {
            let _ = mem
                .store_with_origin(&format!("[entorno] {aviso}"), "entorno", 0.6)
                .await;
        }
        return (true, format!("{summary}. {aviso}"));
    }

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
        "disco extern",
        "discos extern",
        "discos montad",
        "qué discos",
        "que discos",
        "cámara",
        "camara",
        "webcam",
        "usb",
        "dispositivos conectad",
    ];
    CUES.iter().any(|c| p.contains(c))
}

/// Formatea lo percibido como CONTEXTO para el prompt: AION responde desde datos reales, no memoria.
pub fn grounding_note(
    net: &[NetDevice],
    usb: &[UsbDevice],
    disks: &[DiskInfo],
    cams: &[String],
) -> String {
    if net.is_empty() && usb.is_empty() && disks.is_empty() && cams.is_empty() {
        return "LO QUE PERCIBO AHORA (tus sentidos, solo lectura): no detecto dispositivos en la \
                red, USB, discos ni cámara en este instante."
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
    if !disks.is_empty() {
        s.push_str(&format!("Discos/volúmenes montados — {}:\n", disks.len()));
        for d in disks.iter().take(25) {
            s.push_str(&format!("- {} ({})\n", d.name, d.path));
        }
    }
    if !cams.is_empty() {
        s.push_str(&format!("Cámaras — {}: {}\n", cams.len(), cams.join(", ")));
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

    #[test]
    fn is_senses_query_detecta_red_y_no_otras() {
        assert!(is_senses_query("¿qué ves en mi red local?"));
        assert!(is_senses_query("escanea la red por favor"));
        assert!(is_senses_query("¿qué dispositivos hay?"));
        assert!(!is_senses_query("cuéntame un chiste"));
        assert!(!is_senses_query("¿cómo estás hoy?"));
    }

    #[test]
    fn service_label_traduce_los_comunes() {
        assert_eq!(service_label("_ssh._tcp.local."), "SSH");
        assert_eq!(service_label("_airplay._tcp.local."), "AirPlay");
        assert_eq!(service_label("_googlecast._tcp.local."), "Chromecast");
    }
}
