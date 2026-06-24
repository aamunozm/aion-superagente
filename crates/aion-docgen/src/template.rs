//! Motor de plantillas (minijinja). Las plantillas viven embebidas en el binario
//! (`include_str!`) y se renderizan en runtime con el contexto del documento.
//!
//! Autoescape HTML activado: los campos de texto (título, datos del cliente…) se escapan
//! automáticamente, y SOLO el cuerpo —que ya es HTML generado por nosotros desde Markdown—
//! se marca como seguro con el filtro `|safe` dentro de la plantilla.

use minijinja::{AutoEscape, Environment};
use serde::Serialize;

const LAYOUT: &str = include_str!("../templates/_layout.html");
const BASE: &str = include_str!("../templates/base.html");
const PREVENTIVO: &str = include_str!("../templates/preventivo.html");

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    // Escapa SIEMPRE como HTML (también plantillas sin extensión, que registramos por nombre).
    env.set_auto_escape_callback(|_name| AutoEscape::Html);
    // El layout es el padre del que heredan las demás (cabecera + CSS de marca + bloques).
    env.add_template("_layout.html", LAYOUT)
        .expect("layout válido en build");
    env.add_template("base", BASE)
        .expect("plantilla base válida en build");
    env.add_template("preventivo", PREVENTIVO)
        .expect("plantilla preventivo válida en build");
    env
}

/// Renderiza la plantilla `name` con el contexto serializable dado. Devuelve el HTML final.
pub fn render<C: Serialize>(name: &str, ctx: &C) -> Result<String, String> {
    let env = environment();
    let tmpl = env
        .get_template(name)
        .map_err(|e| format!("la plantilla «{name}» no existe: {e}"))?;
    tmpl.render(ctx)
        .map_err(|e| format!("no pude renderizar «{name}»: {e}"))
}

/// Nombres de plantilla disponibles (para validar entrada del usuario/agente).
pub const AVAILABLE: &[&str] = &["base", "preventivo"];
