//! **Permisos diferidos — human-in-the-loop para acciones AUTÓNOMAS de AION.**
//!
//! El canal síncrono `request_confirmation` (serve.rs) solo sirve cuando ARIEL le pide algo al
//! agente y está mirando. Pero la vida autónoma (`life_tick`) no tiene a Ariel delante. Aquí está
//! el lazo que faltaba (auditoría 2026-06): AION **pide** permiso para una acción sensible → queda
//! como permiso PENDIENTE persistente y se le avisa a Ariel por la Bandeja → Ariel **aprueba o
//! deniega** (endpoint) → al aprobar, AION **ejecuta** la acción. Nada se ejecuta sin tu sí.
//!
//! Cada permiso lleva `kind`+`payload` para poder re-ejecutarse (p. ej. kind="open_app",
//! payload="Spotify"). El despachador (`dispatch`) sabe cómo materializar cada tipo.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Permit {
    pub id: String,
    pub capability: String,
    /// Tipo de acción re-ejecutable: "open_app", … (lo entiende `dispatch`).
    pub kind: String,
    /// Dato para ejecutar (p. ej. el nombre de la app).
    pub payload: String,
    /// Descripción legible para Ariel.
    pub action: String,
    /// pending | approved | denied | done | failed
    pub status: String,
    pub created_at: i64,
    pub decided_at: Option<i64>,
}

const MAX_KEEP: usize = 200;

fn lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

fn path() -> PathBuf {
    crate::app_data_dir().join("permits.jsonl")
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn load() -> Vec<Permit> {
    std::fs::read_to_string(path())
        .map(|s| {
            s.lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str::<Permit>(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn save(items: &[Permit]) {
    // Conserva los últimos MAX_KEEP (los pendientes nunca se podan).
    let kept: Vec<&Permit> = if items.len() > MAX_KEEP {
        let pend: Vec<&Permit> = items.iter().filter(|p| p.status == "pending").collect();
        let mut tail: Vec<&Permit> = items
            .iter()
            .filter(|p| p.status != "pending")
            .rev()
            .take(MAX_KEEP.saturating_sub(pend.len()))
            .collect();
        tail.extend(pend);
        tail
    } else {
        items.iter().collect()
    };
    let body = kept
        .iter()
        .filter_map(|p| serde_json::to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n");
    crate::write_atomic(&path(), &body);
}

/// AION pide permiso para una acción: queda PENDIENTE. Devuelve su id.
pub fn request(capability: &str, kind: &str, payload: &str, action: &str) -> String {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut items = load();
    let id = uuid::Uuid::new_v4().to_string();
    items.push(Permit {
        id: id.clone(),
        capability: capability.to_string(),
        kind: kind.to_string(),
        payload: payload.to_string(),
        action: action.to_string(),
        status: "pending".into(),
        created_at: now(),
        decided_at: None,
    });
    save(&items);
    id
}

/// Lista (recientes primero) para la Bandeja/UI.
pub fn list() -> Vec<Permit> {
    let mut v = load();
    v.reverse();
    v
}

/// Ariel aprueba o deniega un permiso pendiente. Devuelve true si cambió de estado.
pub fn respond(id: &str, approve: bool) -> bool {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut items = load();
    let mut changed = false;
    if let Some(p) = items
        .iter_mut()
        .find(|p| p.id == id && p.status == "pending")
    {
        p.status = if approve { "approved" } else { "denied" }.into();
        p.decided_at = Some(now());
        changed = true;
    }
    if changed {
        save(&items);
    }
    changed
}

fn set_status(id: &str, status: &str) {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut items = load();
    if let Some(p) = items.iter_mut().find(|p| p.id == id) {
        p.status = status.to_string();
        save(&items);
    }
}

/// **Ejecuta los permisos APROBADOS pendientes de hacer.** Lo llama el lazo de vida (y, en caliente,
/// el endpoint de aprobación para que sea inmediato). Tras ejecutar, audita y avisa por la Bandeja.
pub async fn execute_approved() {
    let pend: Vec<Permit> = {
        let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
        load()
            .into_iter()
            .filter(|p| p.status == "approved")
            .collect()
    };
    for p in pend {
        let ok = dispatch(&p.kind, &p.payload).await;
        set_status(&p.id, if ok { "done" } else { "failed" });
        crate::governance::note_user_action(
            crate::governance::Capability::Computer,
            &format!("(autorizado por ti) {}", p.action),
            ok,
        );
        crate::workspace::publish(crate::workspace::StreamEvent::now(
            "vida",
            if ok { "pensamiento" } else { "estado" },
            &format!(
                "{} lo que aprobaste: {}",
                if ok { "hice" } else { "no pude hacer" },
                p.action
            ),
        ));
        if let Ok(ibx) = crate::inbox::Inbox::open(crate::inbox_path()) {
            let _ = ibx.push(
                "hecho",
                &format!("{} {}", if ok { "Hecho:" } else { "No pude:" }, p.action),
            );
        }
    }
}

/// Materializa una acción aprobada según su tipo. Aquí se enchufan las capacidades de los anillos.
async fn dispatch(kind: &str, payload: &str) -> bool {
    match kind {
        "open_app" => crate::computer::open_app(payload),
        "shell" => run_shell(payload).await,
        _ => false,
    }
}

/// Ejecuta un comando de terminal YA APROBADO por Ariel. Veta lo catastrófico aunque esté aprobado.
async fn run_shell(cmd: &str) -> bool {
    if crate::agent_tools::shell_is_catastrophic(cmd) {
        return false;
    }
    let res = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("/bin/zsh")
            .arg("-c")
            .arg(cmd)
            .output(),
    )
    .await;
    matches!(res, Ok(Ok(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // El ciclo de estados sobre una lista en memoria (sin tocar disco).
    #[test]
    #[allow(clippy::useless_vec)] // el test muta `items` como Vec; el vec! es intencional
    fn ciclo_pending_approved() {
        let mut items = vec![Permit {
            id: "x".into(),
            capability: "computer".into(),
            kind: "open_app".into(),
            payload: "Spotify".into(),
            action: "abrir Spotify".into(),
            status: "pending".into(),
            created_at: 0,
            decided_at: None,
        }];
        // aprobar
        if let Some(p) = items
            .iter_mut()
            .find(|p| p.id == "x" && p.status == "pending")
        {
            p.status = "approved".into();
        }
        assert_eq!(items[0].status, "approved");
        // un segundo "respond" no debe re-cambiar (ya no está pending)
        let still_pending = items.iter().any(|p| p.id == "x" && p.status == "pending");
        assert!(!still_pending);
    }
}
