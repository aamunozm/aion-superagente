//! `aion-computer` — control del computador **bajo gobernanza**.
//!
//! Toda capacidad sobre el PC (archivos, apps, email, shell, compras…) se modela
//! como una [`Action`] que DEBE pasar por el [`Governor`] antes de ejecutarse. El
//! Governor aplica una [`Policy`] determinista (permitir / confirmar / denegar),
//! lo registra en el [`AuditLog`] y ofrece una papelera reversible ([`AionTrash`]).
//!
//! **La seguridad vive aquí, no en el modelo.** Un LLM sin censura solo *propone*;
//! el Governor *dispone*. Ni un prompt ni un email malicioso pueden saltárselo.

pub mod action;
pub mod audit;
pub mod policy;
pub mod trash;

pub use action::{Action, Category, Reversibility};
pub use audit::{AuditLog, AuditRecord};
pub use policy::{Decision, Policy, Posture, Safeguard};
pub use trash::{AionTrash, TrashEntry};

use std::path::PathBuf;

/// Orquestador de gobernanza: política + auditoría + papelera, persistido en disco.
pub struct Governor {
    pub policy: Policy,
    audit: AuditLog,
    trash: AionTrash,
    policy_path: PathBuf,
}

impl Governor {
    /// Abre el Governor sobre un directorio de datos (crea lo que falte). Carga la
    /// política guardada o usa la conservadora por defecto.
    pub fn open(data_dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)?;
        let policy_path = data_dir.join("policy.json");
        let policy = match std::fs::read_to_string(&policy_path) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
            Err(_) => Policy::default(),
        };
        let audit = AuditLog::open(data_dir.join("audit.jsonl"))?;
        let trash = AionTrash::open(data_dir.join("trash"))?;
        Ok(Self {
            policy,
            audit,
            trash,
            policy_path,
        })
    }

    /// Evalúa una acción y registra la decisión (aún sin ejecutar). El llamador
    /// debe respetar la decisión: ejecutar solo si es `Allow`, o tras confirmación
    /// humana si es `Confirm`; nunca si es `Deny`.
    pub fn authorize(&self, action: &Action) -> Decision {
        let decision = self.policy.evaluate(action);
        let pending = matches!(decision, Decision::Confirm { .. });
        let outcome = match &decision {
            Decision::Allow { .. } => Some("autorizada".into()),
            Decision::Confirm { reason, .. } => Some(format!("a la espera de confirmación: {reason}")),
            Decision::Deny { reason } => Some(format!("denegada: {reason}")),
        };
        let _ = self.audit.record(action, &decision, false, outcome);
        let _ = pending;
        decision
    }

    /// Registra que una acción confirmada/permitida se ejecutó y su resultado.
    pub fn record_execution(&self, action: &Action, decision: &Decision, outcome: impl Into<String>) {
        let _ = self
            .audit
            .record(action, decision, true, Some(outcome.into()));
    }

    /// Activa/desactiva el kill switch (pausa global) y persiste.
    pub fn set_paused(&mut self, paused: bool) -> std::io::Result<()> {
        self.policy.paused = paused;
        self.save_policy()
    }

    /// Cambia la postura de seguridad y persiste.
    pub fn set_posture(&mut self, posture: Posture) -> std::io::Result<()> {
        self.policy.posture = posture;
        self.save_policy()
    }

    pub fn save_policy(&self) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.policy)?;
        let tmp = self.policy_path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.policy_path)
    }

    pub fn audit(&self) -> &AuditLog {
        &self.audit
    }

    pub fn trash(&self) -> &AionTrash {
        &self.trash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governor_persists_policy_and_audits() {
        let dir = std::env::temp_dir().join(format!("aion-gov-{}", uuid::Uuid::new_v4()));
        {
            let mut g = Governor::open(&dir).unwrap();
            assert_eq!(g.policy.posture, Posture::Conservative);
            g.set_posture(Posture::Balanced).unwrap();
            // Una compra siempre exige confirmación y queda auditada.
            let d = g.authorize(&Action::purchase("teclado", "80€"));
            assert!(matches!(d, Decision::Confirm { .. }));
        }
        // Reabrir: la postura persiste y el audit tiene el registro.
        let g2 = Governor::open(&dir).unwrap();
        assert_eq!(g2.policy.posture, Posture::Balanced);
        assert_eq!(g2.audit().all().unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
