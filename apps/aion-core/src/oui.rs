//! **Base de datos OUI local** (IEEE / Wireshark `manuf`): MAC → fabricante, OFFLINE y fiable.
//!
//! Por qué existe: el agente NO debe depender de una API gratuita con rate-limit (que falla a la
//! 3.ª consulta) ni —mucho menos— inventar la marca de un dispositivo. Con esta tabla embebida
//! (~39k OUIs de 24 bits, ~1MB), `net_scan` resuelve la marca de CADA equipo aquí mismo, al instante,
//! sin red y sin riesgo de alucinación. Lo que no esté en la tabla se reporta "fabricante desconocido"
//! con franqueza. Datos de Wireshark `manuf` (libremente redistribuible).

use std::collections::HashMap;
use std::sync::OnceLock;

const DATA: &str = include_str!("oui_data.txt");

fn map() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        DATA.lines()
            .filter_map(|l| {
                let mut it = l.splitn(2, '\t');
                Some((it.next()?, it.next()?))
            })
            .collect()
    })
}

/// Fabricante de una MAC por su OUI (primeros 3 octetos). None si no está en la tabla.
/// Tolera el formato de `arp` de macOS, que OMITE el cero a la izquierda por octeto
/// (p. ej. "6a:b:dd:…" significa 6A:0B:DD): se normaliza cada octeto a 2 hex.
pub fn vendor(mac: &str) -> Option<&'static str> {
    let octets: Vec<&str> = mac.split(':').collect();
    if octets.len() < 3 {
        return None;
    }
    let mut key = String::with_capacity(6);
    for o in &octets[..3] {
        if o.is_empty() || o.len() > 2 || !o.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        if o.len() == 1 {
            key.push('0');
        }
        key.push_str(&o.to_uppercase());
    }
    map().get(key.as_str()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resuelve_y_tolera_ceros_omitidos() {
        // 24-bit estándar
        assert!(vendor("1c:86:9a:3e:51:72").is_some()); // Samsung
                                                        // macOS omite el cero: "6a:b:dd" = 6A:0B:DD (no debe confundirse con 6A:BD:D…)
        let _ = vendor("6a:b:dd:8f:41:95"); // no debe panicar; clave = 6A0BDD
        assert_eq!(vendor("zz:zz:zz"), None);
        assert_eq!(vendor("solo:dos"), None);
    }
}
