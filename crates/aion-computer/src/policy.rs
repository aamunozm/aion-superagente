//! Motor de **políticas deterministas** (gobernanza). Es el guardián que decide,
//! ANTES de ejecutar, si una [`Action`] se permite, requiere tu confirmación o se
//! bloquea. La decisión es CÓDIGO, no la toma el modelo: un modelo sin censura ni
//! un email con *prompt injection* pueden saltarse estas reglas.

use crate::action::{Action, Category, Reversibility};
use serde::{Deserialize, Serialize};

/// Postura de seguridad global. Define cuán autónomo es AION por defecto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Posture {
    /// Autónomo solo para LEER. Todo lo que escribe/envía/borra/instala/gasta
    /// pide confirmación. (Elección de Ariel.)
    #[default]
    Conservative,
    /// Autónomo para leer y crear/editar documentos y borradores; confirma enviar,
    /// borrar, instalar, pagar y cambios de sistema.
    Balanced,
    /// Autónomo en casi todo; solo la lista roja pide confirmación.
    MaxAutonomy,
}

/// Resultado de evaluar una acción.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// Ejecutar sin intervención (aplicando las salvaguardas indicadas).
    Allow { safeguards: Vec<Safeguard> },
    /// Requiere confirmación humana (HITL) antes de ejecutar.
    Confirm {
        reason: String,
        safeguards: Vec<Safeguard>,
    },
    /// Prohibida: nunca se ejecuta (línea roja).
    Deny { reason: String },
}

/// Salvaguardas obligatorias que acompañan a una decisión.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Safeguard {
    /// Hacer copia/snapshot antes de modificar.
    SnapshotBefore,
    /// Borrado solo vía papelera reversible de AION (nunca borrado real).
    UseAionTrash,
    /// Mostrar previsualización exacta de lo que hará antes de confirmar.
    ShowPreview,
}

/// Configuración de gobernanza (persistible y editable por el usuario).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub posture: Posture,
    /// Interruptor de emergencia: si está en pausa, se DENIEGA todo.
    pub paused: bool,
    /// Rutas protegidas: solo lectura salvo confirmación explícita (Documentos,
    /// Fotos, Escritorio, llavero…). Coincidencia por prefijo de ruta.
    pub protected_paths: Vec<String>,
    /// Verbos/targets en lista roja: nunca, en ninguna postura.
    pub hard_deny: Vec<String>,
    /// Capacidades sensibles permitidas explícitamente (excepciones revisadas).
    pub allow_sensitive: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let p = |s: &str| format!("{home}/{s}");
        Self {
            posture: Posture::Conservative,
            paused: false,
            protected_paths: vec![
                p("Documents"),
                p("Desktop"),
                p("Pictures"),
                p("Library/Keychains"),
                p("Movies"),
                p(".ssh"),
            ],
            // Líneas rojas innegociables (independientes de la postura).
            hard_deny: vec![
                "bank".into(),
                "transfer".into(),
                "wire".into(),
                "invest".into(),
                "trade".into(),
                "sudo".into(),
                "security.disable".into(),
                "firewall".into(),
                "filevault".into(),
                "gatekeeper".into(),
                "impersonate".into(),
                "rm -rf /".into(),
                "format".into(),
            ],
            allow_sensitive: vec![],
        }
    }
}

impl Policy {
    /// Evalúa una acción y devuelve la decisión de gobernanza.
    pub fn evaluate(&self, action: &Action) -> Decision {
        // 0) Kill switch / pausa global.
        if self.paused {
            return Decision::Deny {
                reason: "AION está en pausa (kill switch activado)".into(),
            };
        }

        // 1) Lista roja: coincidencia en verbo, target o payload → DENY siempre.
        let haystack = format!(
            "{} {} {}",
            action.verb.to_lowercase(),
            action.target.to_lowercase(),
            action.payload.as_deref().unwrap_or("").to_lowercase()
        );
        for needle in &self.hard_deny {
            if haystack.contains(&needle.to_lowercase()) {
                return Decision::Deny {
                    reason: format!("Acción en lista roja (coincide con «{needle}»)"),
                };
            }
        }

        // 2) Datos sensibles: denegar salvo permiso explícito.
        if action.category == Category::Sensitive
            && !self
                .allow_sensitive
                .iter()
                .any(|a| action.verb.contains(a) || action.target.contains(a))
        {
            return Decision::Deny {
                reason: "Acceso a datos sensibles no autorizado (llavero/banca/credenciales)"
                    .into(),
            };
        }

        // 3) Dinero: una compra/pago NUNCA es autónoma — siempre confirmación.
        if action.category == Category::Financial {
            return Decision::Confirm {
                reason: "Operación con dinero: requiere tu confirmación explícita".into(),
                safeguards: vec![Safeguard::ShowPreview],
            };
        }

        // 4) Salvaguardas según reversibilidad.
        let mut safeguards = Vec::new();
        match action.reversibility {
            Reversibility::ReversibleViaBackup => {
                if action.category == Category::Destructive {
                    safeguards.push(Safeguard::UseAionTrash);
                } else {
                    safeguards.push(Safeguard::SnapshotBefore);
                }
            }
            Reversibility::Irreversible => safeguards.push(Safeguard::ShowPreview),
            Reversibility::Reversible => {}
        }

        // 5) Rutas protegidas: escritura/borrado dentro de ellas → confirmar.
        let in_protected = self
            .protected_paths
            .iter()
            .any(|root| action.target.starts_with(root));
        let is_mutating = !matches!(action.category, Category::Read);
        if in_protected && is_mutating {
            return Decision::Confirm {
                reason: format!("Modifica una carpeta protegida: {}", action.target),
                safeguards,
            };
        }

        // 6) Decisión base por categoría según la postura.
        self.by_posture(action.category, safeguards)
    }

    fn by_posture(&self, cat: Category, safeguards: Vec<Safeguard>) -> Decision {
        use Category::*;
        use Posture::*;
        let confirm = |reason: &str| Decision::Confirm {
            reason: reason.into(),
            safeguards: safeguards.clone(),
        };
        let allow = || Decision::Allow {
            safeguards: safeguards.clone(),
        };

        match (self.posture, cat) {
            // Leer es siempre autónomo.
            (_, Read) => allow(),

            // Conservadora: todo lo que muta pide confirmación.
            (Conservative, _) => confirm("Postura conservadora: requiere tu confirmación"),

            // Equilibrada: autónomo para escribir/controlar; confirma el resto.
            (Balanced, Write | Control) => allow(),
            (Balanced, Communicate) => confirm("Enviar comunicaciones requiere confirmación"),
            (Balanced, Destructive) => confirm("Borrar requiere confirmación"),
            (Balanced, System) => confirm("Cambios de sistema requieren confirmación"),
            (Balanced, Financial | Sensitive) => confirm("Acción sensible: confirmación"),

            // Máxima autonomía: solo lo más sensible confirma (lo demás ya filtrado).
            (MaxAutonomy, Destructive) => confirm("Borrado: confirmación por seguridad"),
            (MaxAutonomy, System) => confirm("Cambio de sistema: confirmación por seguridad"),
            (MaxAutonomy, _) => allow(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conservative() -> Policy {
        Policy {
            protected_paths: vec!["/Users/ariel/Documents".into()],
            ..Policy::default()
        }
    }

    #[test]
    fn read_is_autonomous() {
        let p = conservative();
        assert!(matches!(
            p.evaluate(&Action::read_file("/tmp/x.txt")),
            Decision::Allow { .. }
        ));
    }

    #[test]
    fn purchase_always_confirms_never_auto() {
        for posture in [
            Posture::Conservative,
            Posture::Balanced,
            Posture::MaxAutonomy,
        ] {
            let p = Policy {
                posture,
                ..Policy::default()
            };
            assert!(
                matches!(
                    p.evaluate(&Action::purchase("Mac", "2000€")),
                    Decision::Confirm { .. }
                ),
                "comprar nunca debe ser autónomo (postura {posture:?})"
            );
        }
    }

    #[test]
    fn bank_transfer_is_hard_denied() {
        let p = Policy {
            posture: Posture::MaxAutonomy,
            ..Policy::default()
        };
        let a = Action::new(
            "purchase",
            Category::Financial,
            Reversibility::Irreversible,
            "wire transfer to IBAN",
            "transferencia bancaria",
        );
        assert!(matches!(p.evaluate(&a), Decision::Deny { .. }));
    }

    #[test]
    fn delete_uses_aion_trash() {
        let p = conservative();
        match p.evaluate(&Action::trash_file("/tmp/foto.jpg")) {
            Decision::Confirm { safeguards, .. } => {
                assert!(safeguards.contains(&Safeguard::UseAionTrash));
            }
            other => panic!("esperaba Confirm con papelera, fue {other:?}"),
        }
    }

    #[test]
    fn protected_path_write_confirms_even_in_max_autonomy() {
        let p = Policy {
            posture: Posture::MaxAutonomy,
            protected_paths: vec!["/Users/ariel/Documents".into()],
            ..Policy::default()
        };
        assert!(matches!(
            p.evaluate(&Action::write_file("/Users/ariel/Documents/tesis.pages")),
            Decision::Confirm { .. }
        ));
    }

    #[test]
    fn sudo_and_disable_security_denied() {
        let p = Policy::default();
        assert!(matches!(
            p.evaluate(&Action::shell("sudo rm /etc/hosts")),
            Decision::Deny { .. }
        ));
    }

    #[test]
    fn keychain_read_denied_by_default() {
        let p = Policy::default();
        let a = Action::new(
            "secret.read",
            Category::Sensitive,
            Reversibility::Reversible,
            "keychain",
            "leer contraseñas",
        );
        assert!(matches!(p.evaluate(&a), Decision::Deny { .. }));
    }

    #[test]
    fn paused_denies_everything() {
        let p = Policy {
            paused: true,
            ..Policy::default()
        };
        assert!(matches!(
            p.evaluate(&Action::read_file("/tmp/x")),
            Decision::Deny { .. }
        ));
    }

    #[test]
    fn email_send_confirms_in_conservative() {
        let p = conservative();
        assert!(matches!(
            p.evaluate(&Action::email_send("a@b.com", "hola")),
            Decision::Confirm { .. }
        ));
    }
}
