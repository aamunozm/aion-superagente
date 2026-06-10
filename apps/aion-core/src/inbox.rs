//! **Bandeja de AION** — la voz proactiva del agente.
//!
//! La vida autónoma (bucle `live`) no solo guarda recuerdos: cuando descubre,
//! aprende o quiere algo, escribe un mensaje **para ti** aquí. La UI los muestra
//! como mensajes que AION te inicia (te habla primero) y emite una notificación.
//! Persistente en JSONL para sobrevivir reinicios.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    pub id: String,
    pub at: DateTime<Utc>,
    /// Tipo: "insight" | "pregunta" | "idea" | "saludo" | "alerta".
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub read: bool,
}

/// Bandeja persistente (JSONL append-only; marcar leído reescribe el archivo).
pub struct Inbox {
    path: PathBuf,
}

impl Inbox {
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Self { path })
    }

    /// AION te escribe un mensaje. Devuelve el id.
    pub fn push(&self, kind: &str, text: &str) -> std::io::Result<String> {
        use std::io::Write;
        let msg = InboxMessage {
            id: uuid::Uuid::new_v4().to_string(),
            at: Utc::now(),
            kind: kind.to_string(),
            text: text.to_string(),
            read: false,
        };
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{}", serde_json::to_string(&msg)?)?;
        Ok(msg.id)
    }

    pub fn all(&self) -> std::io::Result<Vec<InboxMessage>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }

    pub fn unread(&self) -> std::io::Result<Vec<InboxMessage>> {
        Ok(self.all()?.into_iter().filter(|m| !m.read).collect())
    }

    pub fn unread_count(&self) -> usize {
        self.unread().map(|v| v.len()).unwrap_or(0)
    }

    /// Marca como leídos todos los mensajes (o solo uno si se da `id`).
    pub fn mark_read(&self, id: Option<&str>) -> std::io::Result<()> {
        let mut all = self.all()?;
        for m in &mut all {
            if id.is_none() || id == Some(m.id.as_str()) {
                m.read = true;
            }
        }
        let body: String = all
            .iter()
            .filter_map(|m| serde_json::to_string(m).ok())
            .map(|s| s + "\n")
            .collect();
        let tmp = self.path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_unread_and_mark_read() {
        let path = std::env::temp_dir().join(format!("aion-inbox-{}.jsonl", uuid::Uuid::new_v4()));
        let inbox = Inbox::open(&path).unwrap();
        inbox
            .push("insight", "Aprendí algo sobre tu agenda")
            .unwrap();
        inbox
            .push("pregunta", "¿Quieres que organice tus documentos?")
            .unwrap();
        assert_eq!(inbox.unread_count(), 2);
        inbox.mark_read(None).unwrap();
        assert_eq!(inbox.unread_count(), 0);
        assert_eq!(inbox.all().unwrap().len(), 2);
        std::fs::remove_file(&path).ok();
    }
}
