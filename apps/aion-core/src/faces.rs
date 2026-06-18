//! **Reconocimiento facial — el "quién es quién" (cerebro de identidades).**
//!
//! Este módulo es la parte de DECISIÓN: dado el *faceprint* (embedding 512-dim, p. ej. de ArcFace)
//! de una cara, decide a QUIÉN pertenece — un conocido enrolado o alguien nuevo. Implementa el modo
//! acordado con Ariel: **auto-detecta y nombras después** — una cara nueva se guarda como
//! "Persona N" (sin nombre) y Ariel le pone nombre cuando quiera; las siguientes veces que aparezca,
//! se reconoce y se refuerza su perfil.
//!
//! La parte de PERCEPCIÓN (cámara → Apple Vision detecta+alinea → ArcFace produce el embedding) es la
//! fase ML que se enchufa aquí llamando a `observe(embedding)`. Privacidad: los faceprints son datos
//! biométricos sensibles → viven SOLO en el Mac (`faces.jsonl`), nunca se exfiltran; el embedding no
//! se expone por la API (solo nombre/contadores).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Umbral de coseno para considerar que dos faceprints son la MISMA persona (ArcFace ~0.5).
const MATCH_THRESHOLD: f32 = 0.5;
/// Máximo de embeddings por persona (perfil multi-ángulo; se rota el más viejo).
const MAX_EMB_PER_PERSON: usize = 12;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Person {
    pub id: String,
    /// Nombre puesto por Ariel; None hasta que lo nombre (se muestra como "Persona N").
    pub name: Option<String>,
    /// Faceprints (embeddings) de esta persona. NO se exponen por la API.
    #[serde(default)]
    pub embeddings: Vec<Vec<f32>>,
    pub times_seen: u32,
    pub created_at: i64,
    pub last_seen: i64,
}

/// Vista pública (sin biometría) para la UI / endpoints.
#[derive(Serialize, Clone, Debug)]
pub struct PersonSummary {
    pub id: String,
    pub label: String,
    pub named: bool,
    pub times_seen: u32,
    pub last_seen: i64,
}

fn lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

fn path() -> PathBuf {
    crate::app_data_dir().join("faces.jsonl")
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn load() -> Vec<Person> {
    std::fs::read_to_string(path())
        .map(|s| {
            s.lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str::<Person>(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn save(items: &[Person]) {
    let body = items
        .iter()
        .filter_map(|p| serde_json::to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n");
    crate::write_atomic(&path(), &body);
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Mejor parecido de un faceprint contra una persona (máximo coseno entre sus embeddings).
fn best_sim(emb: &[f32], p: &Person) -> f32 {
    p.embeddings
        .iter()
        .map(|e| cosine(emb, e))
        .fold(0.0f32, f32::max)
}

/// **Observa una cara** (su faceprint): si coincide con un conocido, lo reconoce y refuerza su
/// perfil; si no, crea una persona nueva "Persona N" (sin nombre, para que Ariel la nombre luego).
/// Devuelve (id, etiqueta, reconocido_ya_conocido).
pub fn observe(emb: &[f32]) -> (String, String, bool) {
    if emb.is_empty() {
        return (String::new(), "—".into(), false);
    }
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut people = load();

    // ¿A quién se parece más?
    let mut best: Option<(usize, f32)> = None;
    for (i, p) in people.iter().enumerate() {
        let s = best_sim(emb, p);
        if best.is_none_or(|(_, bs)| s > bs) {
            best = Some((i, s));
        }
    }

    if let Some((i, s)) = best {
        if s >= MATCH_THRESHOLD {
            // Conocido: refuerza el perfil.
            let p = &mut people[i];
            p.embeddings.push(emb.to_vec());
            if p.embeddings.len() > MAX_EMB_PER_PERSON {
                p.embeddings.remove(0);
            }
            p.times_seen += 1;
            p.last_seen = now();
            let label = label_of(p);
            let id = p.id.clone();
            save(&people);
            return (id, label, true);
        }
    }

    // Nuevo: "Persona N".
    let n = people.len() + 1;
    let person = Person {
        id: uuid::Uuid::new_v4().to_string(),
        name: None,
        embeddings: vec![emb.to_vec()],
        times_seen: 1,
        created_at: now(),
        last_seen: now(),
    };
    let id = person.id.clone();
    let label = format!("Persona {n}");
    people.push(person);
    save(&people);
    (id, label, false)
}

fn label_of(p: &Person) -> String {
    p.name
        .clone()
        .unwrap_or_else(|| format!("Persona ({})", &p.id[..p.id.len().min(4)]))
}

/// Ariel le pone nombre a una persona detectada. Devuelve true si existía.
pub fn name_person(id: &str, name: &str) -> bool {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut people = load();
    if let Some(p) = people.iter_mut().find(|p| p.id == id) {
        p.name = Some(name.trim().to_string()).filter(|s| !s.is_empty());
        save(&people);
        true
    } else {
        false
    }
}

/// Ruta del helper Swift `face-probe` (sidecar, junto al binario aion-core en la app).
fn probe_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("face-probe")))
        .unwrap_or_else(|| PathBuf::from("face-probe"))
}

/// **Escanea con la cámara y reconoce.** Bajo demanda y con permiso (gobernanza Camera). Ejecuta el
/// helper Swift (captura + Apple Vision detecta + faceprint), y para cada cara decide quién es
/// (`observe`). Devuelve a quién reconoce. BLOQUEANTE (~4s): llamar desde spawn_blocking.
pub fn scan() -> serde_json::Value {
    if !crate::governance::request(
        crate::governance::Capability::Camera,
        "encender la cámara para reconocer quién está delante",
    )
    .allowed()
    {
        return serde_json::json!({ "error": "permiso de cámara no concedido (gobernanza)", "recognized": [] });
    }
    let out = match std::process::Command::new(probe_path()).output() {
        Ok(o) => o,
        Err(_) => {
            return serde_json::json!({ "error": "no encuentro el helper de cámara (face-probe)", "recognized": [] })
        }
    };
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    if let Some(err) = parsed.get("error").and_then(|e| e.as_str()) {
        return serde_json::json!({ "error": err, "recognized": [] });
    }
    let mut recognized = Vec::new();
    if let Some(faces) = parsed.get("faces").and_then(|f| f.as_array()) {
        for f in faces {
            let emb: Vec<f32> = f
                .get("embedding")
                .and_then(|e| e.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_f64().map(|v| v as f32))
                        .collect()
                })
                .unwrap_or_default();
            if emb.is_empty() {
                continue;
            }
            let (id, label, known) = observe(&emb);
            recognized.push(serde_json::json!({ "id": id, "label": label, "known": known }));
        }
    }
    serde_json::json!({ "error": serde_json::Value::Null, "recognized": recognized })
}

/// ¿Ariel pregunta por reconocimiento facial / quién está delante?
pub fn is_recognize_query(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    const CUES: &[&str] = &[
        "quién soy",
        "quien soy",
        "me reconoces",
        "reconóceme",
        "reconoceme",
        "quién está",
        "quien esta",
        "quién hay delante",
        "mírame",
        "mirame",
        "usa la cámara",
        "usa la camara",
        "con la cámara",
        "con la camara",
        "reconocimiento facial",
        "reconoce mi cara",
        "reconoce mi rostro",
    ];
    CUES.iter().any(|c| p.contains(c))
}

/// Formatea el resultado de un escaneo como CONTEXTO para el prompt (AION responde desde lo real).
pub fn recognize_note(scan: &serde_json::Value) -> String {
    if let Some(err) = scan.get("error").and_then(|e| e.as_str()) {
        return format!(
            "INTENTASTE reconocer con la cámara pero no pudiste: {err}. Dilo con franqueza \
             (quizá falta concederme el permiso de cámara, o no hay nadie delante)."
        );
    }
    let rec = scan
        .get("recognized")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    if rec.is_empty() {
        return "Encendiste la cámara pero NO detectaste ninguna cara ahora mismo. Dilo con franqueza."
            .into();
    }
    let mut s = String::from("LO QUE RECONOCES AHORA POR LA CÁMARA (real, responde desde esto):\n");
    for r in &rec {
        let label = r.get("label").and_then(|l| l.as_str()).unwrap_or("?");
        let known = r.get("known").and_then(|k| k.as_bool()).unwrap_or(false);
        if known {
            s.push_str(&format!("- Reconozco a {label}.\n"));
        } else {
            s.push_str(&format!(
                "- Veo a alguien que aún no tengo nombrado ({label}). Si Ariel me dice quién es, lo recuerdo.\n"
            ));
        }
    }
    s
}

/// Lista de personas conocidas (sin biometría), para la UI/endpoints.
pub fn list() -> Vec<PersonSummary> {
    let mut people = load();
    people.sort_by_key(|b| std::cmp::Reverse(b.last_seen));
    people
        .iter()
        .enumerate()
        .map(|(i, p)| PersonSummary {
            id: p.id.clone(),
            label: p
                .name
                .clone()
                .unwrap_or_else(|| format!("Persona {}", i + 1)),
            named: p.name.is_some(),
            times_seen: p.times_seen,
            last_seen: p.last_seen,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_basico() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0); // distinta dimensión → 0
    }

    #[test]
    fn match_threshold_separa_personas() {
        let p = Person {
            id: "a".into(),
            name: Some("Ariel".into()),
            embeddings: vec![vec![1.0, 0.0, 0.0]],
            times_seen: 1,
            created_at: 0,
            last_seen: 0,
        };
        // misma dirección → match alto
        assert!(best_sim(&[0.9, 0.1, 0.0], &p) >= MATCH_THRESHOLD);
        // ortogonal → no match
        assert!(best_sim(&[0.0, 0.0, 1.0], &p) < MATCH_THRESHOLD);
    }
}
