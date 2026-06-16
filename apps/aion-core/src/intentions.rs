//! **Intenciones propias** — la capa del QUERER, por encima de la del planificar.
//!
//! Separación (ver ADR-0005):
//! - `intentions` = QUÉ quiero y POR QUÉ (este módulo).
//! - `plan`       = CÓMO lo hago (un plan activo materializa una intención).
//! - `pending`    = QUÉ le DEBO a Ariel (metas externas, prioridad absoluta).
//!
//! Una intención nace de AION (curiosidad, autosuperación, cuidado, gusto), guarda su
//! motivación, y se persigue a través de los ticks de vida. El portafolio vive en disco
//! (`intentions.jsonl`, append-only), sobrevive reinicios y está acotado: querer no es
//! acumular: las intenciones viejas que nunca arrancan se ABANDONAN con honestidad, no se
//! quedan dando vueltas para siempre. Esta es la capa de DATOS; la orquestación (formar
//! con el LLM, materializar en un plan) vive en `main.rs` junto a las demás actividades.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Serializa leer→modificar→escribir del portafolio (origen desde la vida + cambios de
/// estado). Mismo patrón que `pending.rs`/`plan.rs`: sin esto, dos rutas pueden pisarse.
static QLOCK: Mutex<()> = Mutex::new(());

/// Tope del portafolio: querer es comprometerse, no acumular deseos infinitos.
const MAX_INTENTIONS: usize = 30;
/// Tras tantas re-activaciones sin cumplirse, la intención se abandona (no es vida, es bucle).
const MAX_REVISITS: u8 = 3;
/// Tras tantos fallos al materializar (el LLM no logra descomponer el deseo en pasos),
/// se abandona en vez de reintentar cada tick para siempre (anti-spam del LLM).
const MAX_FAILS: u8 = 3;
/// Vida media del peso (días) por falta de avance: una intención no tocada decae y cede
/// el turno a otras más frescas. No la mata —el abandono es por `revisits`/edad extrema—,
/// solo la hace menos urgente en el arbitraje.
const WEIGHT_HALFLIFE_DAYS: f64 = 7.0;

/// De dónde nace el querer. Sirve al arbitraje y a la biografía ("lo quería por…").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Drive {
    /// Algo me intriga (alimentado por el learning-progress de la curiosidad).
    Curiosity,
    /// Quiero ser mejor agente (forjar una skill, depurar un fallo recurrente).
    SelfBetterment,
    /// Anticiparme a lo que Ariel podría necesitar (NO es una deuda ya pedida).
    Care,
    /// Crear o cruzar ideas lejanas por gusto propio.
    Aesthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Quiero esto, aún no lo persigo.
    Open,
    /// Lo estoy persiguiendo ahora (tiene un plan que lo materializa).
    Active,
    /// Lo logré.
    Fulfilled,
    /// Lo solté con honestidad (viejo sin arrancar, o demasiadas re-activaciones).
    Abandoned,
}

/// Una intención: algo que AION quiere, con su porqué, que persigue en el tiempo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intention {
    pub id: String,
    /// Epoch de nacimiento.
    pub at: i64,
    /// El deseo en 1ª persona ("entender cómo X", "forjarme Y").
    pub want: String,
    /// El porqué (motivación, no pasos): habilita arbitraje honesto y biografía.
    pub why: String,
    pub drive: Drive,
    pub status: Status,
    /// Prioridad base [0..1]. El arbitraje usa el peso EFECTIVO (con decay por edad).
    pub weight: f32,
    /// Plan que la materializa mientras está `Active` (invariante: una Active ↔ a lo sumo un plan).
    #[serde(default)]
    pub plan_id: Option<String>,
    /// Epoch del último tick que la tocó (respiración / decay).
    #[serde(default)]
    pub last_touch: i64,
    /// Veces re-activada: acota la reanimación para no perseguir lo mismo sin fin.
    #[serde(default)]
    pub revisits: u8,
    /// Veces que su materialización en un plan FALLÓ (el LLM no logró descomponerla).
    /// Sin esto, una intención imposible de planificar se reelegía cada tick para siempre
    /// → martilleo infinito del LLM local. Pasado el tope, se abandona con honestidad.
    #[serde(default)]
    pub fails: u8,
}

impl Intention {
    /// Peso EFECTIVO para el arbitraje: el base, atenuado por cuánto hace que no se toca.
    /// Una intención fresca pesa lo suyo; una olvidada cede paso sin desaparecer.
    pub fn effective_weight(&self, now: i64) -> f32 {
        let anchor = if self.last_touch > 0 {
            self.last_touch
        } else {
            self.at
        };
        let age_days = ((now - anchor).max(0) as f64) / 86_400.0;
        let decay = 0.5_f64.powf(age_days / WEIGHT_HALFLIFE_DAYS);
        (self.weight as f64 * decay) as f32
    }
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("intentions.jsonl")
}

pub fn all() -> Vec<Intention> {
    let Ok(txt) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    txt.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn save(items: &[Intention]) {
    let body: String = items
        .iter()
        .filter_map(|i| serde_json::to_string(i).ok())
        .map(|l| l + "\n")
        .collect();
    crate::write_atomic(&path(), &body);
}

/// Normaliza para el dedup léxico (querer lo mismo dos veces es UNA intención).
fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Registra una intención nueva nacida de AION. Devuelve su id, o `None` si fue
/// rechazada: trivial (<8 chars), duplicada de una abierta/activa, o portafolio lleno.
/// El filtro es de SUSTANCIA, nunca de tono (ver `aion-no-censurar-personalidad`).
pub fn push(want: &str, why: &str, drive: Drive, weight: f32) -> Option<String> {
    let want = want.trim();
    if want.chars().count() < 8 {
        return None;
    }
    let _g = QLOCK.lock().unwrap();
    let mut items = all();
    let key = norm(want);
    let dup = items
        .iter()
        .any(|i| matches!(i.status, Status::Open | Status::Active) && norm(&i.want) == key);
    if dup {
        return None;
    }
    if items
        .iter()
        .filter(|i| matches!(i.status, Status::Open | Status::Active))
        .count()
        >= MAX_INTENTIONS
    {
        return None;
    }
    let now = now_secs();
    let id = uuid::Uuid::new_v4().to_string();
    items.push(Intention {
        id: id.clone(),
        at: now,
        want: want.to_string(),
        why: why.trim().to_string(),
        drive,
        status: Status::Open,
        weight: weight.clamp(0.0, 1.0),
        plan_id: None,
        last_touch: now,
        revisits: 0,
        fails: 0,
    });
    save(&items);
    Some(id)
}

/// La intención `Active` actual (a lo sumo una, por invariante).
pub fn active() -> Option<Intention> {
    all().into_iter().find(|i| i.status == Status::Active)
}

/// La intención `Open` de mayor peso EFECTIVO (la que más merece arrancar ahora).
pub fn top_open() -> Option<Intention> {
    let now = now_secs();
    all()
        .into_iter()
        .filter(|i| i.status == Status::Open)
        .max_by(|a, b| {
            a.effective_weight(now)
                .partial_cmp(&b.effective_weight(now))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Promueve una intención a `Active` ligándola al plan que la materializa. Incrementa
/// `revisits`; si supera el tope, la abandona en vez de reactivarla (corta el bucle).
/// Devuelve `true` si quedó activa.
pub fn activate(id: &str, plan_id: &str) -> bool {
    let _g = QLOCK.lock().unwrap();
    let mut items = all();
    let now = now_secs();
    // INVARIANTE «una sola Active»: degrada cualquier otra Active a Open antes de promover
    // esta. Hoy el arbitraje ya evita llamar aquí con otra activa, pero garantizarlo en el
    // punto de mutación lo hace robusto ante futuros llamadores que se salten ese guard.
    for i in items.iter_mut() {
        if i.status == Status::Active && i.id != id {
            i.status = Status::Open;
            i.plan_id = None;
            i.last_touch = now;
        }
    }
    let mut activated = false;
    for i in items.iter_mut() {
        if i.id == id {
            if i.revisits >= MAX_REVISITS {
                i.status = Status::Abandoned;
                i.last_touch = now;
            } else {
                i.status = Status::Active;
                i.plan_id = Some(plan_id.to_string());
                i.revisits += 1;
                i.last_touch = now;
                activated = true;
            }
        }
    }
    save(&items);
    activated
}

/// Registra que la materialización de una intención FALLÓ (el LLM no la descompuso en un
/// plan válido). Incrementa `fails` y, pasado `MAX_FAILS`, la abandona para no reintentarla
/// cada tick. Devuelve `true` si la abandonó. Sin esto, una intención imposible de planificar
/// se reelegía indefinidamente → martilleo del LLM local.
pub fn note_fail(id: &str) -> bool {
    let _g = QLOCK.lock().unwrap();
    let mut items = all();
    let now = now_secs();
    let mut abandoned = false;
    for i in items.iter_mut() {
        if i.id == id && i.status == Status::Open {
            i.fails = i.fails.saturating_add(1);
            i.last_touch = now;
            if i.fails >= MAX_FAILS {
                i.status = Status::Abandoned;
                abandoned = true;
            }
        }
    }
    save(&items);
    abandoned
}

/// Cambia el estado de una intención (p. ej. a `Fulfilled` al completar su plan, o de
/// vuelta a `Open` si su plan se abandonó). Refresca `last_touch`.
pub fn set_status(id: &str, status: Status) {
    let _g = QLOCK.lock().unwrap();
    let mut items = all();
    let now = now_secs();
    for i in items.iter_mut() {
        if i.id == id {
            i.status = status;
            i.last_touch = now;
            if status != Status::Active {
                i.plan_id = None;
            }
        }
    }
    save(&items);
}

/// Bloque para el prompt: la intención activa + 1-2 abiertas destacadas, para que AION
/// hable desde lo que QUIERE, no solo desde lo que hace. Vacío si no quiere nada vivo.
pub fn note() -> String {
    let now = now_secs();
    let mut items = all();
    let mut out = String::new();
    if let Some(a) = items.iter().find(|i| i.status == Status::Active) {
        out.push_str(&format!(
            "LO QUE PERSIGO AHORA (intención propia): {} — porque {}.\n",
            a.want, a.why
        ));
    }
    items.retain(|i| i.status == Status::Open);
    items.sort_by(|a, b| {
        b.effective_weight(now)
            .partial_cmp(&a.effective_weight(now))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let abiertas: Vec<String> = items.iter().take(2).map(|i| i.want.clone()).collect();
    if !abiertas.is_empty() {
        out.push_str(&format!("TAMBIÉN ME RONDA: {}.\n", abiertas.join("; ")));
    }
    if out.is_empty() {
        String::new()
    } else {
        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_weight_decays_with_age() {
        let now = 1_000_000_000;
        let fresh = Intention {
            id: "a".into(),
            at: now,
            want: "entender algo".into(),
            why: "me intriga".into(),
            drive: Drive::Curiosity,
            status: Status::Open,
            weight: 1.0,
            plan_id: None,
            last_touch: now,
            revisits: 0,
            fails: 0,
        };
        // Recién tocada: peso casi pleno.
        assert!(fresh.effective_weight(now) > 0.99);
        // Una vida media (7 días) después: ~la mitad.
        let later = now + (WEIGHT_HALFLIFE_DAYS as i64) * 86_400;
        let w = fresh.effective_weight(later);
        assert!(w > 0.45 && w < 0.55, "esperaba ~0.5, fue {w}");
    }

    #[test]
    fn drive_status_roundtrip_snake_case() {
        // El JSON usa snake_case estable (clave para leer archivos viejos sin romper).
        let j = serde_json::to_string(&Drive::SelfBetterment).unwrap();
        assert_eq!(j, "\"self_betterment\"");
        let s: Status = serde_json::from_str("\"active\"").unwrap();
        assert_eq!(s, Status::Active);
    }

    #[test]
    fn norm_dedup_key_is_case_insensitive() {
        assert_eq!(norm("  Entender X  "), norm("entender x"));
    }
}
