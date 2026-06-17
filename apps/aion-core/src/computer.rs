//! **Percepción del computador (Anillo 2, solo lectura)** — AION ve qué apps tienes abiertas y
//! cuál está en primer plano, sin tocarlas.
//!
//! Primer paso seguro del Anillo 2: percibir la pantalla antes de poder actuar sobre ella. Usa
//! `NSWorkspace.runningApplications` (AppKit, vía objc2) — listar apps NO requiere permiso de
//! Accesibilidad (TCC); eso solo hará falta para ACTUAR (clicar, escribir) en una fase futura, que
//! irá detrás de human-in-the-loop. Todo bajo la puerta de gobernanza (ComputerRead = Allow).

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub bundle: Option<String>,
    pub frontmost: bool,
}

/// Lista las aplicaciones con UI abiertas (política "Regular"), marcando la de primer plano.
#[cfg(target_os = "macos")]
pub fn list_apps() -> Vec<AppInfo> {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};

    if !crate::governance::request(
        crate::governance::Capability::ComputerRead,
        "ver qué aplicaciones tienes abiertas",
    )
    .allowed()
    {
        return Vec::new();
    }

    let mut out: Vec<AppInfo> = objc2::rc::autoreleasepool(|_| unsafe {
        let ws = NSWorkspace::sharedWorkspace();
        let front_bundle: Option<String> = ws
            .frontmostApplication()
            .and_then(|a| a.bundleIdentifier())
            .map(|s| s.to_string());
        let apps = ws.runningApplications();
        let mut v = Vec::new();
        for i in 0..apps.count() {
            let app = apps.objectAtIndex(i);
            // Solo apps "regulares" (con Dock/UI), no daemons ni accesorios de fondo.
            if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
                continue;
            }
            let Some(name) = app.localizedName().map(|s| s.to_string()) else {
                continue;
            };
            let bundle = app.bundleIdentifier().map(|s| s.to_string());
            let frontmost = bundle.is_some() && bundle == front_bundle;
            v.push(AppInfo {
                name,
                bundle,
                frontmost,
            });
        }
        v
    });
    // Primero la app en primer plano, luego alfabético.
    out.sort_by(|a, b| b.frontmost.cmp(&a.frontmost).then(a.name.cmp(&b.name)));
    out
}

#[cfg(not(target_os = "macos"))]
pub fn list_apps() -> Vec<AppInfo> {
    Vec::new()
}

// ── ACTUACIÓN (Anillo 2): abrir/enfocar una app ──────────────────────────────
// Reversible y de bajo riesgo. Cuando Ariel lo PIDE por el chat, su orden directa ES el
// human-in-the-loop (no hace falta volver a preguntar); queda auditado. Si fuera AION por su
// cuenta, pasaría por la puerta (Capability::Computer = AskAriel → Bandeja).

/// Lanza o trae al frente una aplicación por nombre (NSWorkspace). No requiere Accesibilidad.
#[cfg(target_os = "macos")]
pub fn open_app(name: &str) -> bool {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    objc2::rc::autoreleasepool(|_| unsafe {
        NSWorkspace::sharedWorkspace().launchApplication(&NSString::from_str(name))
    })
}

#[cfg(not(target_os = "macos"))]
pub fn open_app(_name: &str) -> bool {
    false
}

/// Nombres de apps conocidas: las abiertas + las instaladas (carpetas *.app). Para reconocer con
/// precisión a qué app se refiere Ariel y NO lanzar cosas arbitrarias.
fn known_app_names() -> Vec<String> {
    let mut names: Vec<String> = list_apps().into_iter().map(|a| a.name).collect();
    for dir in [
        "/Applications",
        "/System/Applications",
        "/Applications/Utilities",
    ] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if let Some(stem) = n.strip_suffix(".app") {
                    names.push(stem.to_string());
                }
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(rd) = std::fs::read_dir(format!("{home}/Applications")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if let Some(stem) = n.strip_suffix(".app") {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Si el prompt es una ORDEN de abrir/enfocar una app conocida, devuelve su nombre exacto.
/// Exige un verbo de apertura + el nombre de una app real mencionado → preciso, sin falsos disparos.
pub fn match_open_command(prompt: &str) -> Option<String> {
    let p = prompt.to_lowercase();
    const VERBS: &[&str] = &[
        "abre",
        "abrí",
        "ábre",
        "abrir",
        "open ",
        "enfoca",
        "pon en primer plano",
        "ponme en primer plano",
        "tráeme",
        "cambia a ",
        "lanza ",
        "inicia ",
        "ábreme",
    ];
    if !VERBS.iter().any(|v| p.contains(v)) {
        return None;
    }
    // De las apps conocidas mencionadas, elige el nombre MÁS LARGO (el más específico).
    known_app_names()
        .into_iter()
        .filter(|n| n.len() >= 3 && p.contains(&n.to_lowercase()))
        .max_by_key(|n| n.len())
}

/// ¿Ariel pregunta por sus apps / lo que tiene abierto / la ventana activa?
pub fn is_apps_query(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    const CUES: &[&str] = &[
        "qué apps",
        "que apps",
        "qué aplicaciones",
        "que aplicaciones",
        "aplicaciones abiertas",
        "apps abiertas",
        "qué tengo abierto",
        "que tengo abierto",
        "qué hay abierto",
        "que hay abierto",
        "en primer plano",
        "ventana activa",
        "app activa",
        "qué programa",
        "que programa",
        "qué estoy usando",
        "que estoy usando",
    ];
    CUES.iter().any(|c| p.contains(c))
}

/// Contexto para el prompt: AION responde desde lo que percibe AHORA, no de memoria.
pub fn grounding_note(apps: &[AppInfo]) -> String {
    if apps.is_empty() {
        return "LO QUE PERCIBO EN TU MAC (solo lectura): no consigo leer las apps abiertas ahora \
                mismo."
            .to_string();
    }
    let front = apps
        .iter()
        .find(|a| a.frontmost)
        .map(|a| a.name.as_str())
        .unwrap_or("ninguna identificada");
    let nombres: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
    format!(
        "LO QUE PERCIBO EN TU MAC AHORA (apps abiertas, solo lectura — responde desde esto):\n\
         En primer plano: {front}.\nAbiertas ({}): {}.",
        apps.len(),
        nombres.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_apps_query_detecta_apps_y_no_otras() {
        assert!(is_apps_query("¿qué apps tengo abiertas?"));
        assert!(is_apps_query("¿qué aplicaciones abiertas hay ahora?"));
        assert!(is_apps_query("¿qué tengo abierto?"));
        assert!(!is_apps_query("¿qué tiempo hace hoy?"));
        assert!(!is_apps_query("abre spotify"));
    }
}
