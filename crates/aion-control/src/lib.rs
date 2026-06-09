//! `aion-control` — las **manos y ojos** de AION sobre el computador.
//!
//! Expone un [`Computer`] que puede *ver* la pantalla (visión) y *actuar* con
//! teclado/ratón sobre cualquier app. **Cada acción pasa por el `Governor`** de
//! `aion-computer`: se autoriza, se confirma o se deniega, y queda auditada. Las
//! primitivas de SO (módulos `screen`/`input`) nunca se llaman sin pasar por aquí.

pub mod input;
pub mod screen;

pub use input::ControlIntent;

use aion_computer::{Action, Category, Decision, Governor, Reversibility};
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("pantalla: {0}")]
    Screen(String),
    #[error("entrada: {0}")]
    Input(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Resultado de intentar una acción de control.
#[derive(Debug, Clone)]
pub enum ControlOutcome {
    /// Ejecutada (o simulada en dry-run).
    Executed { dry_run: bool, summary: String },
    /// Requiere confirmación humana antes de ejecutar.
    NeedsConfirmation { reason: String, summary: String },
    /// Denegada por política (línea roja / pausa).
    Denied { reason: String },
    /// Falló al ejecutar.
    Failed { error: String },
}

/// Fachada de control del computador bajo gobernanza.
pub struct Computer {
    gov: Governor,
    /// Si es `true`, nunca ejecuta de verdad: solo informa qué haría.
    pub dry_run: bool,
}

impl Computer {
    /// Abre el control sobre un directorio de datos (política/auditoría/papelera).
    /// Arranca en **dry-run por seguridad**; actívalo explícitamente.
    pub fn open(data_dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        Ok(Self {
            gov: Governor::open(data_dir)?,
            dry_run: true,
        })
    }

    pub fn governor(&self) -> &Governor {
        &self.gov
    }
    pub fn governor_mut(&mut self) -> &mut Governor {
        &mut self.gov
    }

    /// **Ver** la pantalla: captura y devuelve PNG en base64 (para Gemma visión).
    /// Es una acción de lectura: el Governor la permite salvo pausa global.
    pub fn look(&self) -> Result<String, ControlError> {
        let action = Action::new(
            "screen.capture",
            Category::Read,
            Reversibility::Reversible,
            "pantalla",
            "Capturar la pantalla para verla",
        );
        match self.gov.authorize(&action) {
            Decision::Allow { .. } => {
                let b64 = screen::capture_base64()?;
                self.gov.record_execution(
                    &action,
                    &Decision::Allow { safeguards: vec![] },
                    "captura realizada",
                );
                Ok(b64)
            }
            Decision::Confirm { reason, .. } | Decision::Deny { reason } => {
                Err(ControlError::Screen(format!("captura no permitida: {reason}")))
            }
        }
    }

    /// **Actuar**: intenta una acción de teclado/ratón, pasando por el Governor.
    pub fn intend(&self, intent: ControlIntent) -> ControlOutcome {
        let summary = intent.summary();
        let action = Action::new(
            verb_for(&intent),
            Category::Control,
            Reversibility::Reversible,
            "ui",
            summary.clone(),
        );
        match self.gov.authorize(&action) {
            Decision::Deny { reason } => ControlOutcome::Denied { reason },
            Decision::Confirm { reason, .. } => {
                ControlOutcome::NeedsConfirmation { reason, summary }
            }
            Decision::Allow { .. } => self.run(&intent, &action, &summary),
        }
    }

    /// Ejecuta una intención que el usuario YA confirmó (HITL). Salta la decisión
    /// de confirmación pero deja registro y respeta dry-run y la pausa global.
    pub fn execute_confirmed(&self, intent: ControlIntent) -> ControlOutcome {
        let summary = intent.summary();
        let action = Action::new(
            verb_for(&intent),
            Category::Control,
            Reversibility::Reversible,
            "ui",
            summary.clone(),
        );
        // La pausa global manda incluso sobre una confirmación previa.
        if self.gov.policy.paused {
            return ControlOutcome::Denied {
                reason: "AION está en pausa (kill switch)".into(),
            };
        }
        self.run(&intent, &action, &summary)
    }

    fn run(&self, intent: &ControlIntent, action: &Action, summary: &str) -> ControlOutcome {
        if self.dry_run {
            self.gov.record_execution(
                action,
                &Decision::Allow { safeguards: vec![] },
                format!("DRY-RUN: {summary}"),
            );
            return ControlOutcome::Executed {
                dry_run: true,
                summary: summary.to_string(),
            };
        }
        match input::execute(intent) {
            Ok(()) => {
                self.gov.record_execution(
                    action,
                    &Decision::Allow { safeguards: vec![] },
                    format!("ejecutada: {summary}"),
                );
                ControlOutcome::Executed {
                    dry_run: false,
                    summary: summary.to_string(),
                }
            }
            Err(e) => {
                self.gov.record_execution(
                    action,
                    &Decision::Allow { safeguards: vec![] },
                    format!("fallo: {e}"),
                );
                ControlOutcome::Failed {
                    error: e.to_string(),
                }
            }
        }
    }
}

fn verb_for(intent: &ControlIntent) -> &'static str {
    match intent {
        ControlIntent::Click { .. } => "ui.click",
        ControlIntent::DoubleClick { .. } => "ui.double_click",
        ControlIntent::RightClick { .. } => "ui.right_click",
        ControlIntent::Type { .. } => "ui.type",
        ControlIntent::Key { .. } => "ui.key",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aion_computer::Posture;

    fn computer() -> (Computer, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("aion-ctrl-{}", uuid::Uuid::new_v4()));
        (Computer::open(&dir).unwrap(), dir)
    }

    #[test]
    fn conservative_click_needs_confirmation() {
        let (c, dir) = computer();
        // Postura conservadora por defecto: un clic exige confirmación.
        match c.intend(ControlIntent::Click { x: 10, y: 10 }) {
            ControlOutcome::NeedsConfirmation { .. } => {}
            other => panic!("esperaba confirmación, fue {other:?}"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn paused_denies_even_confirmed() {
        let (mut c, dir) = computer();
        c.governor_mut().set_paused(true).unwrap();
        match c.execute_confirmed(ControlIntent::Type { text: "x".into() }) {
            ControlOutcome::Denied { .. } => {}
            other => panic!("kill switch debe denegar, fue {other:?}"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn balanced_click_is_dry_run_executed() {
        let (mut c, dir) = computer();
        c.governor_mut().set_posture(Posture::Balanced).unwrap();
        // dry_run está activo por defecto → no toca el SO, pero "ejecuta".
        match c.intend(ControlIntent::Click { x: 5, y: 5 }) {
            ControlOutcome::Executed { dry_run: true, .. } => {}
            other => panic!("esperaba ejecución dry-run, fue {other:?}"),
        }
        // y queda auditado
        assert!(!c.governor().audit().all().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
