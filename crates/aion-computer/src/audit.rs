//! Registro de auditoría (audit log): TODA acción evaluada se anota — qué se pidió,
//! qué decidió la política, si se ejecutó y con qué resultado. Append-only JSONL,
//! revisable y filtrable. Es la base de la trazabilidad y de poder revertir.

use crate::action::Action;
use crate::policy::Decision;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: String,
    pub at: DateTime<Utc>,
    pub action: Action,
    pub decision: Decision,
    /// Si finalmente se ejecutó (tras confirmación si aplicaba).
    pub executed: bool,
    /// Resultado/observación tras ejecutar (o motivo de no ejecución).
    pub outcome: Option<String>,
}

/// Audit log persistente (JSONL, append-only).
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Self { path })
    }

    /// Registra una acción evaluada. Devuelve el id del registro.
    pub fn record(
        &self,
        action: &Action,
        decision: &Decision,
        executed: bool,
        outcome: Option<String>,
    ) -> std::io::Result<String> {
        use std::io::Write;
        let rec = AuditRecord {
            id: uuid::Uuid::new_v4().to_string(),
            at: Utc::now(),
            action: action.clone(),
            decision: decision.clone(),
            executed,
            outcome,
        };
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{}", serde_json::to_string(&rec)?)?;
        Ok(rec.id)
    }

    pub fn all(&self) -> std::io::Result<Vec<AuditRecord>> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::policy::Decision;

    #[test]
    fn records_and_reads_back() {
        let path = std::env::temp_dir().join(format!("aion-audit-{}.jsonl", uuid::Uuid::new_v4()));
        let log = AuditLog::open(&path).unwrap();
        let a = Action::email_send("a@b.com", "hola");
        let d = Decision::Confirm {
            reason: "test".into(),
            safeguards: vec![],
        };
        log.record(&a, &d, false, Some("a la espera de confirmación".into()))
            .unwrap();
        let all = log.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].action.verb, "email.send");
        assert!(!all[0].executed);
        std::fs::remove_file(&path).ok();
    }
}
