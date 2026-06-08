//! Audit log persistente: registro append-only de acciones relevantes
//! (acciones del agente, veredictos de auto-evolución, auth…). JSONL local.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// Una entrada del audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub ts: String,     // RFC3339
    pub actor: String,  // p.ej. "agent", "evolution", "auth"
    pub action: String, // p.ej. "tool_call", "candidate_accepted"
    pub detail: String,
}

/// Audit log append-only sobre un archivo JSONL.
pub struct AuditLog {
    path: PathBuf,
    lock: Mutex<()>,
}

impl AuditLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// Audit log por defecto (configurable con `AION_AUDIT`).
    pub fn default_local() -> Self {
        let path = std::env::var("AION_AUDIT").unwrap_or_else(|_| "data/audit.jsonl".into());
        Self::new(path)
    }

    /// Registra una acción. Best-effort: nunca interrumpe el flujo principal.
    pub fn record(&self, actor: &str, action: &str, detail: impl Into<String>) {
        let entry = AuditEntry {
            ts: chrono::Utc::now().to_rfc3339(),
            actor: actor.to_string(),
            action: action.to_string(),
            detail: detail.into(),
        };
        tracing::info!(actor = %entry.actor, action = %entry.action, "audit");
        let _guard = self.lock.lock().unwrap();
        if let Some(dir) = self.path.parent() {
            if !dir.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(dir);
            }
        }
        if let Ok(line) = serde_json::to_string(&entry) {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    /// Lee todas las entradas del audit log.
    pub fn read_all(&self) -> Vec<AuditEntry> {
        let Ok(file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<AuditEntry>(&l).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("aion_audit_{}", std::process::id()));
        let path = dir.join("audit.jsonl");
        let log = AuditLog::new(&path);
        log.record("evolution", "candidate_accepted", "square");
        log.record("agent", "tool_call", "calculator(2+2)");
        let all = log.read_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].actor, "evolution");
        assert_eq!(all[1].action, "tool_call");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
