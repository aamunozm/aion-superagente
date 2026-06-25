//! **Tablero Kanban por etapas** — un tablero por proyecto, para AVANZAR el trabajo con
//! visibilidad de flujo. Inspirado en el modelo de Linear (categorías de estado fijas + columnas
//! personalizables) y en las prácticas Kanban de agencia (WIP por columna, métricas de flujo,
//! plantillas por tipo de trabajo). 100% local; persistencia JSON junto al resto del proyecto.
//!
//! Decisiones de diseño (investigación 2026):
//! - **Categoría de estado** ([`Category`]) separada del **nombre de columna**: una columna es
//!   visual y renombrable, pero su CATEGORÍA (backlog/por-hacer/en-curso/revisión/hecho/cancelado)
//!   da la semántica estable para métricas (% completado, WIP, qué cuenta como «en curso»).
//! - **Límite WIP por columna** (Work In Progress): el corazón del Kanban. Evita el multitasking y
//!   revela cuellos de botella. La UI colorea la columna cuando se supera.
//! - **Log de actividad append-only con actor** (humano vs `aion`): transparencia total de quién
//!   hizo qué — requisito para que un AGENTE opere el tablero sin opacidad.
//! - **Entregables enlazados** a la tarjeta (documento/preventivo/auditoría/URL): el trabajo del
//!   proyecto (lo que genera el Studio) queda atado a la etapa que lo produjo.
//! - **Plantillas con tempística** ([`playbook`]): etapas + duración estimada + checklist de buenas
//!   prácticas por tipo de proyecto (p. ej. web+SEO de agencia).
//!
//! Persistencia (respeta `AION_PROJECTS_DIR` igual que [`crate::projects`]):
//! - `<id>/board.json`           → columnas + tarjetas
//! - `<id>/board_activity.jsonl` → historial append-only (una línea por evento)

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::PathBuf;

/// Categoría semántica de una columna. Es estable aunque el usuario renombre la columna; las
/// métricas (qué es «en curso», qué cuenta como «hecho») se calculan por aquí, no por el nombre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Ideas/pendientes sin empezar (no cuenta como trabajo activo).
    Backlog,
    /// Listo para empezar (comprometido pero no iniciado).
    Todo,
    /// Trabajo activo (cuenta para WIP).
    Doing,
    /// En revisión/aprobación (típico punto de human-in-the-loop con el cliente).
    Review,
    /// Terminado (cuenta para el % completado).
    Done,
    /// Cancelado/descartado (no cuenta ni como pendiente ni como hecho).
    Canceled,
}

impl Category {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "backlog" => Some(Self::Backlog),
            "todo" | "por-hacer" | "porhacer" | "pending" => Some(Self::Todo),
            "doing" | "en-curso" | "encurso" | "wip" | "in-progress" => Some(Self::Doing),
            "review" | "revision" | "revisión" => Some(Self::Review),
            "done" | "hecho" | "completed" => Some(Self::Done),
            "canceled" | "cancelled" | "cancelado" => Some(Self::Canceled),
            _ => None,
        }
    }
    /// Color por defecto on-brand para la cabecera de la columna.
    fn color(self) -> &'static str {
        match self {
            Self::Backlog => "#9aa1a8",
            Self::Todo => "#6b7280",
            Self::Doing => "#2563eb",
            Self::Review => "#b45309",
            Self::Done => "#2f9e6f",
            Self::Canceled => "#c0594e",
        }
    }
}

/// Una **columna** del tablero (estado). Visual + categoría semántica + límite WIP opcional.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub id: String,
    pub name: String,
    pub category: Category,
    /// Orden de la columna (izq→der). Menor = más a la izquierda.
    pub pos: f64,
    /// Límite de trabajo en curso. `None` = sin límite. La UI avisa al superarlo.
    #[serde(default)]
    pub wip: Option<u32>,
    #[serde(default)]
    pub color: String,
}

/// Un ítem de la checklist de una tarjeta (subtareas / criterios de aceptación).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

/// Un **entregable** enlazado a la tarjeta: lo que el proyecto produce y queda atado a su etapa.
/// `kind`: `documento` | `preventivo` | `proposta` | `seo` | `url` | `output`.
/// `reference`: id de salida del Studio, ruta de archivo, o URL, según el `kind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deliverable {
    pub kind: String,
    pub reference: String,
    pub title: String,
}

/// Una **tarjeta** (unidad de trabajo). Solo `title` es obligatorio; el resto se rellena a medida
/// que el trabajo avanza (tarjeta compacta: la UI muestra solo lo que tenga valor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub status_id: String,
    /// Orden dentro de la columna (menor = arriba).
    pub pos: f64,
    #[serde(default)]
    pub desc: String,
    /// 0 = sin prioridad · 1 = baja · 2 = media · 3 = alta · 4 = urgente.
    #[serde(default)]
    pub priority: u8,
    /// Estimación en días (tempística). `None` = sin estimar.
    #[serde(default)]
    pub estimate_days: Option<f64>,
    /// Fecha límite (ISO `YYYY-MM-DD`).
    #[serde(default)]
    pub due: Option<String>,
    /// Responsable: nombre de persona o `AION` si lo lleva el agente.
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub checklist: Vec<ChecklistItem>,
    #[serde(default)]
    pub deliverables: Vec<Deliverable>,
    /// Ids de tarjetas que BLOQUEAN a esta (dependencias).
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub created: String,
    #[serde(default)]
    pub updated: String,
}

/// El tablero completo de un proyecto.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Board {
    #[serde(default)]
    pub statuses: Vec<Status>,
    #[serde(default)]
    pub cards: Vec<Card>,
}

/// Un evento del log append-only. `actor`: `ariel` (humano) | `aion` (agente) | otro nombre.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: String,
    pub at: String,
    pub actor: String,
    /// Verbo corto: `creó` | `movió` | `editó` | `comentó` | `enlazó` | `checklist` | `borró`.
    pub action: String,
    #[serde(default)]
    pub card: String,
    #[serde(default)]
    pub detail: String,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn board_path(pid: &str) -> PathBuf {
    crate::projects::project_dir(pid).join("board.json")
}
fn activity_path(pid: &str) -> PathBuf {
    crate::projects::project_dir(pid).join("board_activity.jsonl")
}

fn read_json<T: DeserializeOwned + Default>(path: &PathBuf) -> T {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
        Err(_) => T::default(),
    }
}
fn write_json<T: Serialize>(path: &PathBuf, v: &T) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(body) = serde_json::to_string_pretty(v) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

// ── Carga / siembra ───────────────────────────────────────────────────────────

/// Carga el tablero tal cual está en disco (vacío si no existe).
pub fn load(pid: &str) -> Board {
    read_json(&board_path(pid))
}

fn save(pid: &str, b: &Board) {
    write_json(&board_path(pid), b);
}

/// Devuelve el tablero, **sembrándolo** con columnas por defecto si aún no tenía ninguna.
/// Idempotente: si ya hay columnas, no toca nada.
pub fn ensure(pid: &str) -> Board {
    ensure_template(pid, "generico")
}

/// Como [`ensure`] pero, si el tablero está vacío, lo siembra con la **plantilla de etapas**
/// indicada (`generico` | `web-seo` | `contenido`). No pisa un tablero ya existente.
pub fn ensure_template(pid: &str, template: &str) -> Board {
    let mut b = load(pid);
    if b.statuses.is_empty() {
        b.statuses = stage_template(template);
        save(pid, &b);
    }
    b
}

/// **Fija** las columnas de una plantilla, REEMPLAZANDO las que hubiera, SIEMPRE que el tablero aún
/// no tenga tarjetas (no se ha usado). Esto resuelve el caso real: al abrir el tablero de un
/// proyecto nuevo se auto-siembra el genérico ([`ensure`]); si luego el usuario elige «sembrar plan
/// web+SEO», queremos que las columnas pasen a las de esa plantilla, no que se ignore por «ya tiene
/// columnas». Si ya hay tarjetas, NO toca las columnas (no huérfana el trabajo). Devuelve `true` si
/// reemplazó.
pub fn set_template(pid: &str, template: &str) -> bool {
    let mut b = load(pid);
    if b.cards.is_empty() {
        b.statuses = stage_template(template);
        save(pid, &b);
        true
    } else {
        false
    }
}

// ── Columnas (estados) ──────────────────────────────────────────────────────────

pub fn list_statuses(pid: &str) -> Vec<Status> {
    let mut s = ensure(pid).statuses;
    s.sort_by(|a, b| {
        a.pos
            .partial_cmp(&b.pos)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    s
}

/// Añade una columna nueva al final. `category` se parsea con tolerancia; si no resuelve, `Todo`.
pub fn add_status(pid: &str, name: &str, category: &str, wip: Option<u32>) -> Status {
    let mut b = ensure(pid);
    let cat = Category::parse(category).unwrap_or(Category::Todo);
    let pos = b.statuses.iter().map(|s| s.pos).fold(0.0, f64::max) + 1.0;
    let st = Status {
        id: uid(),
        name: name.trim().to_string(),
        category: cat,
        pos,
        wip,
        color: cat.color().to_string(),
    };
    b.statuses.push(st.clone());
    save(pid, &b);
    st
}

fn resolve_status<'a>(b: &'a Board, target: &str) -> Option<&'a Status> {
    let t = target.trim();
    // 1) por id exacto · 2) por nombre (case-insensitive) · 3) por categoría.
    b.statuses
        .iter()
        .find(|s| s.id == t)
        .or_else(|| b.statuses.iter().find(|s| s.name.eq_ignore_ascii_case(t)))
        .or_else(|| Category::parse(t).and_then(|c| b.statuses.iter().find(|s| s.category == c)))
}

// ── Tarjetas ───────────────────────────────────────────────────────────────────

fn append_pos(b: &Board, status_id: &str) -> f64 {
    b.cards
        .iter()
        .filter(|c| c.status_id == status_id)
        .map(|c| c.pos)
        .fold(0.0, f64::max)
        + 1.0
}

/// Crea una tarjeta. `title` es obligatorio; `status` puede ser id, nombre o categoría (si no
/// resuelve, cae en la primera columna). Registra la actividad con el `actor`.
pub fn card_create(
    pid: &str,
    actor: &str,
    title: &str,
    status: &str,
    desc: &str,
) -> Result<Card, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("la tarjeta necesita un título".into());
    }
    let mut b = ensure(pid);
    let status_id = resolve_status(&b, status)
        .or_else(|| b.statuses.first())
        .map(|s| s.id.clone())
        .ok_or("el tablero no tiene columnas")?;
    let card = Card {
        id: uid(),
        title: title.to_string(),
        status_id: status_id.clone(),
        pos: append_pos(&b, &status_id),
        desc: desc.trim().to_string(),
        priority: 0,
        estimate_days: None,
        due: None,
        assignee: String::new(),
        labels: Vec::new(),
        checklist: Vec::new(),
        deliverables: Vec::new(),
        blocked_by: Vec::new(),
        created: now(),
        updated: now(),
    };
    b.cards.push(card.clone());
    save(pid, &b);
    log(pid, actor, "creó", &card.id, title);
    Ok(card)
}

/// Parche parcial de una tarjeta: solo se aplican los campos `Some(..)`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CardPatch {
    pub title: Option<String>,
    pub desc: Option<String>,
    pub priority: Option<u8>,
    pub estimate_days: Option<f64>,
    pub due: Option<String>,
    pub assignee: Option<String>,
    pub labels: Option<Vec<String>>,
    pub blocked_by: Option<Vec<String>>,
}

pub fn card_update(
    pid: &str,
    actor: &str,
    card_id: &str,
    patch: CardPatch,
) -> Result<Card, String> {
    let mut b = ensure(pid);
    let c = b
        .cards
        .iter_mut()
        .find(|c| c.id == card_id)
        .ok_or("tarjeta no encontrada")?;
    if let Some(v) = patch.title {
        if !v.trim().is_empty() {
            c.title = v.trim().to_string();
        }
    }
    if let Some(v) = patch.desc {
        c.desc = v;
    }
    if let Some(v) = patch.priority {
        c.priority = v.min(4);
    }
    if let Some(v) = patch.estimate_days {
        c.estimate_days = Some(v.max(0.0));
    }
    if let Some(v) = patch.due {
        c.due = if v.trim().is_empty() {
            None
        } else {
            Some(v.trim().to_string())
        };
    }
    if let Some(v) = patch.assignee {
        c.assignee = v.trim().to_string();
    }
    if let Some(v) = patch.labels {
        c.labels = v;
    }
    if let Some(v) = patch.blocked_by {
        c.blocked_by = v;
    }
    c.updated = now();
    let out = c.clone();
    save(pid, &b);
    log(pid, actor, "editó", card_id, &out.title);
    Ok(out)
}

/// Mueve una tarjeta a otra columna y, opcionalmente, la inserta ANTES de `before` (id de otra
/// tarjeta de esa columna). Sin `before`, va al final. Calcula la posición por punto medio.
pub fn card_move(
    pid: &str,
    actor: &str,
    card_id: &str,
    status: &str,
    before: Option<&str>,
) -> Result<Card, String> {
    let mut b = ensure(pid);
    let status_id = resolve_status(&b, status)
        .map(|s| s.id.clone())
        .ok_or("columna destino desconocida")?;
    let status_name = b
        .statuses
        .iter()
        .find(|s| s.id == status_id)
        .map(|s| s.name.clone())
        .unwrap_or_default();

    // Posición destino dentro de la columna (excluyendo la propia tarjeta).
    let mut col: Vec<(String, f64)> = b
        .cards
        .iter()
        .filter(|c| c.status_id == status_id && c.id != card_id)
        .map(|c| (c.id.clone(), c.pos))
        .collect();
    col.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let new_pos = match before {
        Some(bid) => match col.iter().position(|(id, _)| id == bid) {
            Some(0) => col[0].1 - 1.0,
            Some(i) => (col[i - 1].1 + col[i].1) / 2.0,
            None => col.last().map(|(_, p)| p + 1.0).unwrap_or(1.0),
        },
        None => col.last().map(|(_, p)| p + 1.0).unwrap_or(1.0),
    };

    let c = b
        .cards
        .iter_mut()
        .find(|c| c.id == card_id)
        .ok_or("tarjeta no encontrada")?;
    c.status_id = status_id;
    c.pos = new_pos;
    c.updated = now();
    let out = c.clone();
    save(pid, &b);
    log(pid, actor, "movió", card_id, &format!("→ {status_name}"));
    Ok(out)
}

/// Reemplaza la checklist completa de una tarjeta.
pub fn card_set_checklist(
    pid: &str,
    actor: &str,
    card_id: &str,
    items: Vec<ChecklistItem>,
) -> Result<Card, String> {
    let mut b = ensure(pid);
    let c = b
        .cards
        .iter_mut()
        .find(|c| c.id == card_id)
        .ok_or("tarjeta no encontrada")?;
    let done = items.iter().filter(|i| i.done).count();
    let total = items.len();
    c.checklist = items;
    c.updated = now();
    let out = c.clone();
    save(pid, &b);
    log(pid, actor, "checklist", card_id, &format!("{done}/{total}"));
    Ok(out)
}

/// Enlaza un entregable (documento/preventivo/auditoría/URL) a la tarjeta.
pub fn card_link_deliverable(
    pid: &str,
    actor: &str,
    card_id: &str,
    kind: &str,
    reference: &str,
    title: &str,
) -> Result<Card, String> {
    let mut b = ensure(pid);
    let c = b
        .cards
        .iter_mut()
        .find(|c| c.id == card_id)
        .ok_or("tarjeta no encontrada")?;
    c.deliverables.push(Deliverable {
        kind: kind.trim().to_string(),
        reference: reference.trim().to_string(),
        title: title.trim().to_string(),
    });
    c.updated = now();
    let out = c.clone();
    save(pid, &b);
    log(pid, actor, "enlazó", card_id, title);
    Ok(out)
}

pub fn card_delete(pid: &str, actor: &str, card_id: &str) -> Result<(), String> {
    let mut b = ensure(pid);
    let before = b.cards.len();
    let title = b
        .cards
        .iter()
        .find(|c| c.id == card_id)
        .map(|c| c.title.clone())
        .unwrap_or_default();
    b.cards.retain(|c| c.id != card_id);
    if b.cards.len() == before {
        return Err("tarjeta no encontrada".into());
    }
    save(pid, &b);
    log(pid, actor, "borró", card_id, &title);
    Ok(())
}

// ── Comentarios / actividad ──────────────────────────────────────────────────────

/// Añade un comentario a una tarjeta (queda en el log append-only, con su autor).
pub fn card_comment(pid: &str, actor: &str, card_id: &str, text: &str) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("comentario vacío".into());
    }
    log(pid, actor, "comentó", card_id, text);
    Ok(())
}

fn log(pid: &str, actor: &str, action: &str, card: &str, detail: &str) {
    let ev = Activity {
        id: uid(),
        at: now(),
        actor: actor.trim().to_string(),
        action: action.to_string(),
        card: card.to_string(),
        detail: detail.to_string(),
    };
    let path = activity_path(pid);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(line) = serde_json::to_string(&ev) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Últimos `limit` eventos del log (más recientes primero).
pub fn activity(pid: &str, limit: usize) -> Vec<Activity> {
    let text = std::fs::read_to_string(activity_path(pid)).unwrap_or_default();
    let mut all: Vec<Activity> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    all.reverse();
    all.truncate(limit);
    all
}

// ── Métricas de flujo ────────────────────────────────────────────────────────────

/// Estado de una columna para la UI: nombre, cuántas tarjetas tiene, su límite WIP y si lo supera.
#[derive(Debug, Clone, Serialize)]
pub struct WipState {
    pub status_id: String,
    pub name: String,
    pub count: usize,
    pub wip: Option<u32>,
    pub over: bool,
}

pub fn wip_state(pid: &str) -> Vec<WipState> {
    let b = ensure(pid);
    list_statuses(pid)
        .into_iter()
        .map(|s| {
            let count = b.cards.iter().filter(|c| c.status_id == s.id).count();
            let over = s.wip.map(|w| count as u32 > w).unwrap_or(false);
            WipState {
                status_id: s.id,
                name: s.name,
                count,
                wip: s.wip,
                over,
            }
        })
        .collect()
}

/// Progreso del tablero: (hechas, total computable, porcentaje 0–100). Las canceladas no cuentan
/// en el total (no son ni pendientes ni hechas); las `Done` cuentan como hechas.
pub fn progress(pid: &str) -> (usize, usize, u32) {
    let b = ensure(pid);
    let cat_of = |sid: &str| b.statuses.iter().find(|s| s.id == sid).map(|s| s.category);
    let mut done = 0usize;
    let mut total = 0usize;
    for c in &b.cards {
        match cat_of(&c.status_id) {
            Some(Category::Canceled) => {}
            Some(Category::Done) => {
                done += 1;
                total += 1;
            }
            _ => total += 1,
        }
    }
    let pct = if total == 0 {
        0
    } else {
        ((done as f64 / total as f64) * 100.0).round() as u32
    };
    (done, total, pct)
}

// ── Plantillas de etapas + tempística (prácticas recomendadas) ───────────────────

fn st(name: &str, cat: Category, pos: f64, wip: Option<u32>) -> Status {
    Status {
        id: uid(),
        name: name.to_string(),
        category: cat,
        pos,
        wip,
        color: cat.color().to_string(),
    }
}

/// Columnas iniciales según plantilla. Editable después por el usuario/agente.
pub fn stage_template(name: &str) -> Vec<Status> {
    use Category::*;
    match name.trim().to_lowercase().as_str() {
        // Proyecto web + SEO de agencia (el caso de PRONTO CLICK).
        "web-seo" | "web" | "seo" => vec![
            st("Brief & objetivos", Backlog, 1.0, None),
            st("Auditoría & research", Todo, 2.0, None),
            st("Propuesta / Preventivo", Todo, 3.0, None),
            st("Diseño & contenidos", Doing, 4.0, Some(2)),
            st("Desarrollo & SEO on-page", Doing, 5.0, Some(2)),
            st("Revisión cliente", Review, 6.0, Some(2)),
            st("Publicación & entrega", Done, 7.0, None),
        ],
        // Producción de contenidos.
        "contenido" | "content" => vec![
            st("Ideas", Backlog, 1.0, None),
            st("Redacción", Doing, 2.0, Some(3)),
            st("Edición & SEO", Review, 3.0, Some(2)),
            st("Aprobación", Review, 4.0, Some(2)),
            st("Publicado", Done, 5.0, None),
        ],
        // Genérico (por defecto).
        _ => vec![
            st("Backlog", Backlog, 1.0, None),
            st("Por hacer", Todo, 2.0, None),
            st("En curso", Doing, 3.0, Some(3)),
            st("Revisión", Review, 4.0, Some(2)),
            st("Hecho", Done, 5.0, None),
        ],
    }
}

/// Un paso del **playbook**: tarjeta recomendada con etapa, tempística (días) y checklist de
/// buenas prácticas. Es el «cómo entregar bien» que el agente puede sembrar en el tablero.
#[derive(Debug, Clone, Serialize)]
pub struct PlaybookStep {
    pub title: String,
    /// Columna destino por NOMBRE exacto (resuelve por nombre antes que por categoría, así cada
    /// tarjeta cae en su etapa concreta aunque varias columnas compartan categoría).
    pub stage: &'static str,
    pub estimate_days: f64,
    pub checklist: Vec<&'static str>,
}

/// Plan recomendado por tipo de proyecto: etapas en orden + tempística + prácticas. Devuelve vacío
/// si no hay playbook para ese nombre (entonces se siembra solo el tablero, sin tarjetas).
pub fn playbook(name: &str) -> Vec<PlaybookStep> {
    let step = |title: &str, stage, estimate_days, checklist: Vec<&'static str>| PlaybookStep {
        title: title.to_string(),
        stage,
        estimate_days,
        checklist,
    };
    match name.trim().to_lowercase().as_str() {
        "web-seo" | "web" | "seo" => vec![
            step(
                "Kickoff & brief con el cliente",
                "Brief & objetivos",
                1.0,
                vec![
                    "Objetivos de negocio y KPIs",
                    "Público y propuesta de valor",
                    "Accesos (dominio, hosting, analítica)",
                    "Marca y activos disponibles",
                ],
            ),
            step(
                "Auditoría técnica + SEO inicial",
                "Auditoría & research",
                3.0,
                vec![
                    "Rastreo e indexación",
                    "Core Web Vitals / velocidad",
                    "Investigación de keywords",
                    "Análisis de competencia",
                    "Inventario de contenidos",
                ],
            ),
            step(
                "Propuesta / preventivo",
                "Propuesta / Preventivo",
                2.0,
                vec![
                    "Alcance y entregables",
                    "Precio y forma de pago",
                    "Tempística por fases",
                    "Firmas de ambas partes y fecha",
                    "Cláusula de privacidad (GDPR)",
                ],
            ),
            step(
                "Arquitectura & wireframes",
                "Diseño & contenidos",
                3.0,
                vec![
                    "Mapa del sitio",
                    "Wireframes de plantillas",
                    "Flujo de conversión",
                ],
            ),
            step(
                "Diseño UI + contenidos",
                "Diseño & contenidos",
                5.0,
                vec!["Diseño on-brand", "Textos y multimedia", "Accesibilidad AA"],
            ),
            step(
                "Maquetación & SEO on-page",
                "Desarrollo & SEO on-page",
                5.0,
                vec![
                    "Titles y meta descripciones",
                    "Datos estructurados (schema)",
                    "Sitemap y robots",
                    "Optimización de velocidad",
                    "Alt text e imágenes",
                ],
            ),
            step(
                "QA + revisión del cliente",
                "Revisión cliente",
                2.0,
                vec![
                    "Pruebas cross-device",
                    "Enlaces y formularios",
                    "Aprobación del cliente",
                ],
            ),
            step(
                "Publicación + indexación",
                "Publicación & entrega",
                1.0,
                vec![
                    "Despliegue",
                    "Search Console + sitemap",
                    "Analítica activa",
                    "Copia de seguridad",
                ],
            ),
        ],
        _ => Vec::new(),
    }
}

/// Siembra el tablero con la plantilla de etapas Y las tarjetas del playbook (tempística +
/// checklist). Idempotente sobre las COLUMNAS (no las duplica); las tarjetas se añaden siempre que
/// el tablero no tuviera ninguna, para no duplicar al re-sembrar. Devuelve cuántas tarjetas creó.
pub fn seed_playbook(pid: &str, name: &str, actor: &str) -> usize {
    if !load(pid).cards.is_empty() {
        return 0; // ya hay trabajo: no re-sembrar ni tocar columnas.
    }
    // Tablero sin usar → FIJA las columnas de esta plantilla (reemplaza el genérico auto-sembrado al
    // abrir el tablero), para que las tarjetas del playbook caigan en sus etapas reales.
    set_template(pid, name);
    let mut n = 0;
    for s in playbook(name) {
        if let Ok(card) = card_create(pid, actor, &s.title, s.stage, "") {
            let items: Vec<ChecklistItem> = s
                .checklist
                .iter()
                .map(|t| ChecklistItem {
                    text: t.to_string(),
                    done: false,
                })
                .collect();
            let _ = card_set_checklist(pid, actor, &card.id, items);
            let _ = card_update(
                pid,
                actor,
                &card.id,
                CardPatch {
                    estimate_days: Some(s.estimate_days),
                    ..Default::default()
                },
            );
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    // Lock COMPARTIDO con los tests de `projects`: ambos aíslan con la misma env var global
    // `AION_PROJECTS_DIR`, así que deben serializarse entre sí (un lock por módulo no basta).
    use crate::projects::TEST_ENV_LOCK as LOCK;

    fn isolate() -> (String, String) {
        let dir = std::env::temp_dir().join(format!("aion-board-{}", uuid::Uuid::new_v4()));
        std::env::set_var("AION_PROJECTS_DIR", &dir);
        (
            dir.to_string_lossy().to_string(),
            uuid::Uuid::new_v4().to_string(),
        )
    }

    #[test]
    fn siembra_crea_mueve_y_mide() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, pid) = isolate();

        // Siembra por defecto = 5 columnas, 0 tarjetas.
        let b = ensure(&pid);
        assert_eq!(b.statuses.len(), 5);
        assert!(b.cards.is_empty());

        // Crear tarjeta en categoría "todo" → cae en "Por hacer".
        let c = card_create(&pid, "ariel", "Hacer la home", "todo", "").unwrap();
        let todo = list_statuses(&pid)
            .into_iter()
            .find(|s| s.category == Category::Todo)
            .unwrap();
        assert_eq!(c.status_id, todo.id);

        // Título obligatorio.
        assert!(card_create(&pid, "ariel", "   ", "todo", "").is_err());

        // Mover a "Hecho" → progreso 100%.
        let done = list_statuses(&pid)
            .into_iter()
            .find(|s| s.category == Category::Done)
            .unwrap();
        card_move(&pid, "aion", &c.id, &done.id, None).unwrap();
        let (d, total, pct) = progress(&pid);
        assert_eq!((d, total, pct), (1, 1, 100));

        // El log registró creó + movió, con actores distintos.
        let acts = activity(&pid, 10);
        assert!(acts
            .iter()
            .any(|a| a.action == "movió" && a.actor == "aion"));
        assert!(acts
            .iter()
            .any(|a| a.action == "creó" && a.actor == "ariel"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wip_se_supera_y_entregable_se_enlaza() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, pid) = isolate();
        ensure_template(&pid, "generico");
        let curso = list_statuses(&pid)
            .into_iter()
            .find(|s| s.category == Category::Doing)
            .unwrap(); // WIP = 3
        for i in 0..4 {
            card_create(&pid, "ariel", &format!("T{i}"), &curso.id, "").unwrap();
        }
        let w = wip_state(&pid)
            .into_iter()
            .find(|w| w.status_id == curso.id)
            .unwrap();
        assert!(w.over, "4 > WIP 3 debe marcar over");

        // Enlazar un entregable a una tarjeta.
        let card = load(&pid).cards.into_iter().next().unwrap();
        card_link_deliverable(
            &pid,
            "aion",
            &card.id,
            "preventivo",
            "PREV-2026-031",
            "Preventivo",
        )
        .unwrap();
        let updated = load(&pid)
            .cards
            .into_iter()
            .find(|c| c.id == card.id)
            .unwrap();
        assert_eq!(updated.deliverables.len(), 1);
        assert_eq!(updated.deliverables[0].kind, "preventivo");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn playbook_web_seo_siembra_etapas_y_tempistica() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, pid) = isolate();
        // Reproduce el caso REAL: al abrir el tablero se auto-siembra el genérico (5 columnas);
        // sembrar el plan web+SEO debe REEMPLAZAR esas columnas (no quedarse en el genérico).
        assert_eq!(
            ensure(&pid).statuses.len(),
            5,
            "genérico auto-sembrado al abrir"
        );
        let n = seed_playbook(&pid, "web-seo", "aion");
        assert_eq!(n, 8, "el playbook web-seo tiene 8 pasos");
        assert_eq!(
            list_statuses(&pid).len(),
            7,
            "7 etapas web-seo (reemplazó el genérico)"
        );
        // Todas las tarjetas tienen estimación (tempística) y checklist.
        let cards = load(&pid).cards;
        assert!(cards.iter().all(|c| c.estimate_days.is_some()));
        assert!(cards.iter().all(|c| !c.checklist.is_empty()));
        // Cada tarjeta cae en su COLUMNA NOMBRADA (no en la primera de la categoría): la de
        // «Propuesta / preventivo» debe estar en la columna «Propuesta / Preventivo», no en «Auditoría».
        let cols = list_statuses(&pid);
        let col_name = |sid: &str| {
            cols.iter()
                .find(|s| s.id == sid)
                .map(|s| s.name.clone())
                .unwrap_or_default()
        };
        let prop = cards
            .iter()
            .find(|c| c.title.contains("Propuesta"))
            .unwrap();
        assert_eq!(col_name(&prop.status_id), "Propuesta / Preventivo");
        let maq = cards
            .iter()
            .find(|c| c.title.contains("Maquetación"))
            .unwrap();
        assert_eq!(col_name(&maq.status_id), "Desarrollo & SEO on-page");
        // Re-sembrar no duplica.
        assert_eq!(seed_playbook(&pid, "web-seo", "aion"), 0);

        std::fs::remove_dir_all(&dir).ok();
    }
}
