//! Catálogo de **acciones** que AION puede intentar sobre el computador, y su
//! clasificación de riesgo. Toda capacidad del PC (archivos, apps, email, shell,
//! compras…) se modela como una `Action` que DEBE pasar por el motor de políticas
//! antes de ejecutarse.

use serde::{Deserialize, Serialize};

/// Categoría de la acción — eje principal de las reglas de gobernanza.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Leer/observar (pantalla, archivos, bandeja de correo, web). Bajo riesgo.
    Read,
    /// Crear/editar documentos y borradores. Riesgo medio (reversible).
    Write,
    /// Enviar comunicaciones en tu nombre (email, mensajes, redes).
    Communicate,
    /// Borrar / mover a papelera / sobrescribir datos.
    Destructive,
    /// Dinero: comprar, pagar, suscribir, transferir.
    Financial,
    /// Sistema/seguridad: instalar, ajustes, sudo, desactivar protecciones.
    System,
    /// Controlar apps/UI por clicks-teclado-automatización.
    Control,
    /// Acceso a datos sensibles (llavero, banca, salud, credenciales).
    Sensitive,
}

/// ¿La acción es reversible? Afecta a la decisión y al requisito de snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reversibility {
    Reversible,
    /// Reversible solo porque AION guarda copia/papelera antes (ver `trash`).
    ReversibleViaBackup,
    Irreversible,
}

/// Una acción concreta que el agente quiere ejecutar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Verbo estable y legible, p. ej. `file.trash`, `email.send`, `shell.run`.
    pub verb: String,
    pub category: Category,
    pub reversibility: Reversibility,
    /// Recurso afectado (ruta, destinatario, URL, app…). Para reglas y audit.
    pub target: String,
    /// Descripción legible de lo que hará (se le muestra al usuario en HITL).
    pub summary: String,
    /// Carga útil opcional (cuerpo del email, contenido, comando…). No se evalúa
    /// como órdenes — es dato. Útil para la previsualización y el dry-run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

impl Action {
    pub fn new(
        verb: impl Into<String>,
        category: Category,
        reversibility: Reversibility,
        target: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            verb: verb.into(),
            category,
            reversibility,
            target: target.into(),
            summary: summary.into(),
            payload: None,
        }
    }

    pub fn with_payload(mut self, payload: impl Into<String>) -> Self {
        self.payload = Some(payload.into());
        self
    }

    // ── Constructores de conveniencia para las capacidades más comunes ──────

    pub fn read_file(path: impl Into<String>) -> Self {
        let p = path.into();
        Self::new(
            "file.read",
            Category::Read,
            Reversibility::Reversible,
            p.clone(),
            format!("Leer el archivo {p}"),
        )
    }

    pub fn write_file(path: impl Into<String>) -> Self {
        let p = path.into();
        Self::new(
            "file.write",
            Category::Write,
            Reversibility::ReversibleViaBackup,
            p.clone(),
            format!("Escribir/editar el archivo {p}"),
        )
    }

    pub fn trash_file(path: impl Into<String>) -> Self {
        let p = path.into();
        Self::new(
            "file.trash",
            Category::Destructive,
            Reversibility::ReversibleViaBackup,
            p.clone(),
            format!("Mover a la papelera de AION (recuperable 30 días): {p}"),
        )
    }

    pub fn email_read() -> Self {
        Self::new(
            "email.read",
            Category::Read,
            Reversibility::Reversible,
            "inbox",
            "Leer/buscar en la bandeja de correo",
        )
    }

    pub fn email_send(to: impl Into<String>, subject: impl Into<String>) -> Self {
        let to = to.into();
        let subject = subject.into();
        Self::new(
            "email.send",
            Category::Communicate,
            Reversibility::Irreversible,
            to.clone(),
            format!("Enviar email a {to} — asunto: {subject}"),
        )
    }

    pub fn purchase(what: impl Into<String>, amount: impl Into<String>) -> Self {
        let what = what.into();
        let amount = amount.into();
        Self::new(
            "purchase",
            Category::Financial,
            Reversibility::Irreversible,
            what.clone(),
            format!("Comprar/pagar {what} por {amount}"),
        )
    }

    pub fn shell(cmd: impl Into<String>) -> Self {
        let cmd = cmd.into();
        Self::new(
            "shell.run",
            Category::System,
            Reversibility::Irreversible,
            cmd.clone(),
            format!("Ejecutar comando: {cmd}"),
        )
        .with_payload(cmd)
    }

    pub fn app_control(app: impl Into<String>, what: impl Into<String>) -> Self {
        let app = app.into();
        let what = what.into();
        Self::new(
            "app.control",
            Category::Control,
            Reversibility::Reversible,
            app.clone(),
            format!("Controlar {app}: {what}"),
        )
    }
}
