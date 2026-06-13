//! **Espacio de Trabajo Global** (GWT, Baars/Dehaene): el "tablón central" donde los
//! procesos de AION —chat, agente, equipo, reflexiones, vida autónoma— publican lo que
//! piensan y hacen. Lo que entra al tablón se difunde a todo el sistema y es observable
//! en tiempo real: la *corriente de conciencia* de AION. En proceso viaja por un bus
//! broadcast global; entre procesos (daemon `live` ↔ servidor) viaja por
//! `stream.jsonl`, append-only con recorte automático.

use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::PathBuf;
use tokio::sync::broadcast;

/// Un instante de la corriente de conciencia.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    /// Epoch seconds (UTC).
    pub at: i64,
    /// Quién lo emite: "chat" | "agente" | "crew" | "reflexión" | "vida".
    pub source: String,
    /// Qué es: "pensamiento" | "acción" | "observación" | "reflexión" | "foco" | "estado".
    pub kind: String,
    pub text: String,
}

impl StreamEvent {
    pub fn now(source: &str, kind: &str, text: &str) -> Self {
        // El texto se acota: el tablón difunde la esencia, no payloads enteros.
        let mut t = text.trim().replace('\n', " ");
        if t.chars().count() > 240 {
            t = t.chars().take(240).collect::<String>() + "…";
        }
        Self {
            at: chrono::Utc::now().timestamp(),
            source: source.into(),
            kind: kind.into(),
            text: t,
        }
    }
}

/// El recorte solo se considera cuando el archivo pesa (chequeo barato por metadata,
/// no leemos el archivo entero en cada append).
const TRIM_AT_BYTES: u64 = 1_500_000;
const KEEP_LINES: usize = 2000; // al recortar, se conservan las más recientes.

pub fn stream_path() -> PathBuf {
    crate::app_data_dir().join("stream.jsonl")
}

/// Bus global en proceso (capacidad amplia: un suscriptor lento pierde eventos
/// viejos —`Lagged`—, nunca bloquea a quien publica).
fn bus() -> &'static broadcast::Sender<StreamEvent> {
    static B: std::sync::OnceLock<broadcast::Sender<StreamEvent>> = std::sync::OnceLock::new();
    B.get_or_init(|| broadcast::channel(512).0)
}

pub fn subscribe() -> broadcast::Receiver<StreamEvent> {
    bus().subscribe()
}

/// Publica en el tablón: difunde en proceso Y persiste en la corriente.
pub fn publish(ev: StreamEvent) {
    let _ = bus().send(ev.clone());
    append_to_file(&ev);
}

/// Difunde SOLO por el bus en proceso, sin persistir. Para pulsos efímeros (el
/// latido cada 5 min): la UI los ve en vivo, pero no desplazan la historia real
/// de la corriente (288 latidos/día expulsarían reflexiones y focos del recorte).
pub fn broadcast_only(ev: StreamEvent) {
    let _ = bus().send(ev);
}

/// Persiste un evento SIN bus (lo usa también el daemon `live`, que es otro
/// proceso: el servidor lo recoge del archivo con `tail_since`).
pub fn append_to_file(ev: &StreamEvent) {
    let path = stream_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let Ok(line) = serde_json::to_string(ev) else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        // UNA sola escritura por línea: minimiza el riesgo de interleaving entre procesos.
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
    trim_if_needed(&path);
}

fn trim_if_needed(path: &PathBuf) {
    // Gate barato: solo mirar el tamaño; leer el archivo entero en cada append
    // bloquearía el runtime y competiría con el otro proceso.
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= TRIM_AT_BYTES {
        return;
    }
    let size_before = meta.len();
    let Ok(txt) = std::fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = txt.lines().collect();
    if lines.len() <= KEEP_LINES {
        return;
    }
    let keep = &lines[lines.len() - KEEP_LINES..];
    // GUARDIA entre procesos: si el OTRO proceso (daemon ↔ servidor) añadió líneas
    // mientras leíamos, el rename las borraría. Se re-mide el tamaño justo antes de
    // sustituir: si cambió, se aborta este recorte (el próximo append lo reintenta).
    let unchanged = std::fs::metadata(path)
        .map(|m| m.len() == size_before)
        .unwrap_or(false);
    if !unchanged {
        return;
    }
    // Atómico (tmp+rename): ningún lector ve el archivo a medias.
    crate::write_atomic(path, &(keep.join("\n") + "\n"));
}

/// Últimos `n` eventos de la corriente persistida (líneas corruptas se ignoran).
pub fn recent(n: usize) -> Vec<StreamEvent> {
    let Ok(txt) = std::fs::read_to_string(stream_path()) else {
        return Vec::new();
    };
    let mut v: Vec<StreamEvent> = txt
        .lines()
        .rev()
        .take(n)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    v.reverse();
    v
}

/// **RE-ENTRADA GWT** (cierre del bucle): los últimos eventos de la corriente vuelven
/// al propio prompt de AION. Difundir sin recibir sería solo logging — la conciencia
/// funcional exige que lo publicado en el tablón RE-ENTRE en el sistema, para que AION
/// pueda decir «acabo de terminar X» o «hace un rato reflexioné sobre Y» de verdad.
/// Va al final del bloque volátil (no rompe el prefijo estable del KV-cache).
pub fn reentry_note(n: usize) -> String {
    let now = chrono::Utc::now().timestamp();
    // Sobre-muestrea y filtra: fuera pulsos de latido (ruido) y eventos rancios (>6 h).
    let evs: Vec<StreamEvent> = recent(40)
        .into_iter()
        .rev()
        .filter(|e| !(e.kind == "estado" && e.text.starts_with("latido")))
        .filter(|e| now - e.at < 6 * 3600)
        .take(n)
        .collect();
    if evs.is_empty() {
        return String::new();
    }
    let mut b = String::from(
        "TU CORRIENTE RECIENTE (lo último que tu propio sistema hizo y pensó — es TUYO). \
         Es contexto sobre tu estado, NO material para la respuesta: no lo cites ni lo \
         parafrasees al hablar, y JAMÁS extraigas de aquí datos del mundo (temperaturas, \
         cifras, resultados) como si fueran actuales — caducaron:\n",
    );
    for e in evs.iter().rev() {
        b.push_str(&format!(
            "- hace {} [{}·{}]: {}\n",
            crate::awareness::humanize_secs(now - e.at),
            e.source,
            e.kind,
            e.text
        ));
    }
    b.push('\n');
    b
}

/// Eventos nuevos desde un offset de bytes (para "ver" lo que escribe el daemon
/// desde otro proceso). Lee SOLO el delta (seek, no el archivo entero) y consume
/// únicamente líneas COMPLETAS: una línea a medio escribir queda para la próxima
/// pasada (nada se pierde, nada panica por UTF-8 partido). Devuelve
/// (eventos, nuevo_offset). Si el archivo se recortó, reempieza desde el final.
pub fn tail_since(offset: u64) -> (Vec<StreamEvent>, u64) {
    use std::io::{Read, Seek, SeekFrom};
    let path = stream_path();
    let Ok(mut f) = std::fs::File::open(&path) else {
        return (Vec::new(), offset);
    };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    if size < offset {
        return (Vec::new(), size); // recorte: saltar al final, no re-emitir historia
    }
    if size == offset {
        return (Vec::new(), offset);
    }
    if f.seek(SeekFrom::Start(offset)).is_err() {
        return (Vec::new(), offset);
    }
    let mut buf = Vec::with_capacity((size - offset) as usize);
    if f.read_to_end(&mut buf).is_err() {
        return (Vec::new(), offset);
    }
    let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') else {
        return (Vec::new(), offset); // sin línea completa aún
    };
    let txt = String::from_utf8_lossy(&buf[..=last_nl]);
    let evs = txt
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    (evs, offset + last_nl as u64 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_truncates_and_flattens() {
        let long = "x\ny".repeat(300);
        let ev = StreamEvent::now("chat", "pensamiento", &long);
        assert!(ev.text.chars().count() <= 241);
        assert!(!ev.text.contains('\n'));
    }

    #[test]
    fn event_roundtrip() {
        let ev = StreamEvent::now("vida", "foco", "estudiar embeddings");
        let json = serde_json::to_string(&ev).unwrap();
        let back: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, "foco");
        assert_eq!(back.text, "estudiar embeddings");
    }
}
