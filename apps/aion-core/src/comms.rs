//! Gobernanza de COMUNICACIONES por contacto y canal.
//!
//! AION puede leer/escribir Mensajes (iMessage/SMS), WhatsApp, Calendario y Contactos
//! del Mac vía las herramientas de `comms_tools`. Pero —como el modelo de privacidad de
//! Ariel exige— NO puede hablar con quien quiera: aquí se decide CON QUIÉN y EN QUÉ CANAL.
//!
//! Modelo (inspirado en el filtrado de CEO·Intelligence): por defecto las comunicaciones
//! están DESACTIVADAS. Ariel habilita el subsistema y añade una allowlist de contactos;
//! cada contacto declara los canales permitidos y si AION puede solo LEER o también ENVIAR.
//! Un contacto que no esté en la lista queda fuera salvo que se active `default_allow`
//! (modo abierto, explícito). El envío SIEMPRE pasa además por confirmación humana (HITL)
//! en el bucle del agente: esta capa es el filtro de QUIÉN; el HITL, el de CADA acción.
//!
//! Persistido en `comms.json` (0600: contiene teléfonos/emails de contactos).

use serde::{Deserialize, Serialize};

/// Canales de comunicación que AION puede manejar.
pub const CHANNELS: &[&str] = &["imessage", "whatsapp", "calendar", "contacts"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    /// Id estable (slug) para editar/borrar desde la UI.
    pub id: String,
    /// Nombre legible ("Mamá", "Luca", "Equipo ProntoClick").
    pub name: String,
    /// Identificador del canal: teléfono (+39…) o email. Puede ir vacío si solo se filtra por nombre.
    #[serde(default)]
    pub handle: String,
    /// Canales permitidos para este contacto (subconjunto de CHANNELS).
    #[serde(default)]
    pub channels: Vec<String>,
    /// AION puede LEER mensajes/eventos de este contacto.
    #[serde(default)]
    pub allow_read: bool,
    /// AION puede ENVIAR a este contacto (además del HITL por acción).
    #[serde(default)]
    pub allow_send: bool,
    /// Nota libre opcional.
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommsPolicy {
    /// Interruptor maestro: si está desactivado, NINGUNA herramienta de comunicación opera.
    #[serde(default)]
    pub enabled: bool,
    /// Si es true, un contacto no listado se PERMITE (modo abierto). Por defecto false:
    /// solo la allowlist puede comunicarse (privacidad por defecto).
    #[serde(default)]
    pub default_allow: bool,
    /// Allowlist de contactos.
    #[serde(default)]
    pub contacts: Vec<Contact>,
}

impl Default for CommsPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            default_allow: false,
            contacts: Vec::new(),
        }
    }
}

/// Normaliza un handle para comparar (teléfonos sin espacios/guiones, emails en minúscula).
fn norm(handle: &str) -> String {
    let h = handle.trim().to_lowercase();
    if h.contains('@') {
        h
    } else {
        h.chars().filter(|c| c.is_ascii_digit()).collect()
    }
}

impl CommsPolicy {
    /// Busca un contacto por handle (teléfono/email) o por nombre (contains, case-insensitive).
    pub fn find(&self, target: &str) -> Option<&Contact> {
        let t = target.trim();
        let tn = norm(t);
        let tl = t.to_lowercase();
        self.contacts.iter().find(|c| {
            (!c.handle.is_empty() && norm(&c.handle) == tn && !tn.is_empty())
                || (!c.name.is_empty() && c.name.to_lowercase() == tl)
                || (!c.name.is_empty() && !tl.is_empty() && c.name.to_lowercase().contains(&tl))
        })
    }

    /// ¿Puede AION operar `channel` con `target` para la acción dada (send=true → enviar)?
    /// Devuelve Ok(contacto resuelto o "todos") o Err(motivo legible para el agente).
    pub fn authorize(&self, channel: &str, target: &str, send: bool) -> Result<String, String> {
        if !self.enabled {
            return Err(
                "las comunicaciones están desactivadas. Actívalas en el menú «Comunicaciones» \
                 de AION y añade a los contactos con quien puedo hablar."
                    .into(),
            );
        }
        match self.find(target) {
            Some(c) => {
                if !c.channels.iter().any(|ch| ch == channel) {
                    return Err(format!(
                        "«{}» no tiene habilitado el canal {channel}. Ajusta sus canales en el \
                         menú Comunicaciones.",
                        c.name
                    ));
                }
                if send && !c.allow_send {
                    return Err(format!(
                        "tengo permiso para LEER de «{}», pero no para ENVIARLE. Habilita el envío \
                         en el menú Comunicaciones si quieres que le escriba.",
                        c.name
                    ));
                }
                if !send && !c.allow_read {
                    return Err(format!(
                        "«{}» no tiene habilitada la lectura. Ajusta sus permisos en Comunicaciones.",
                        c.name
                    ));
                }
                Ok(c.name.clone())
            }
            None => {
                if self.default_allow {
                    Ok(target.to_string())
                } else {
                    Err(format!(
                        "«{target}» no está en mi lista de contactos permitidos. Añádelo en el menú \
                         Comunicaciones para que pueda comunicarme por ahí."
                    ))
                }
            }
        }
    }
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("comms.json")
}

pub fn load() -> CommsPolicy {
    match std::fs::read_to_string(path()) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
        Err(_) => CommsPolicy::default(),
    }
}

pub fn save(p: &CommsPolicy) -> std::io::Result<()> {
    // Contiene teléfonos/emails de contactos → 0600 (owner-only), rename atómico.
    let json = serde_json::to_string_pretty(p)?;
    crate::write_atomic_secret(&path(), &json);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pol() -> CommsPolicy {
        CommsPolicy {
            enabled: true,
            default_allow: false,
            contacts: vec![Contact {
                id: "mama".into(),
                name: "Mamá".into(),
                handle: "+56 9 1234 5678".into(),
                channels: vec!["imessage".into(), "whatsapp".into()],
                allow_read: true,
                allow_send: false,
                note: String::new(),
            }],
        }
    }

    #[test]
    fn disabled_blocks_everything() {
        let mut p = pol();
        p.enabled = false;
        assert!(p.authorize("imessage", "Mamá", false).is_err());
    }

    #[test]
    fn read_allowed_send_blocked() {
        let p = pol();
        assert!(p.authorize("imessage", "Mamá", false).is_ok());
        assert!(p.authorize("imessage", "Mamá", true).is_err()); // allow_send=false
    }

    #[test]
    fn channel_must_be_enabled() {
        let p = pol();
        assert!(p.authorize("calendar", "Mamá", false).is_err()); // canal no habilitado
    }

    #[test]
    fn match_by_normalized_phone() {
        let p = pol();
        assert!(p.authorize("whatsapp", "+56912345678", false).is_ok());
    }

    #[test]
    fn unknown_contact_blocked_unless_default_allow() {
        let mut p = pol();
        assert!(p.authorize("imessage", "Desconocido", false).is_err());
        p.default_allow = true;
        assert!(p.authorize("imessage", "Desconocido", false).is_ok());
    }
}
