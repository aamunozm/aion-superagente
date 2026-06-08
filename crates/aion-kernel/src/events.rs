//! Bus de eventos pub/sub interno (patrón AutoAgents). Permite que orquestador,
//! cognición y telemetría se comuniquen de forma desacoplada.

use crate::types::Role;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

/// Eventos que circulan por el bus. El orquestador ReAct publica pensamientos,
/// acciones y observaciones; otros subsistemas se suscriben.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AionEvent {
    /// El núcleo arrancó.
    CoreStarted {
        kernel_version: String,
        at: DateTime<Utc>,
    },
    /// Un mensaje fue añadido a una conversación.
    MessageAdded { conversation_id: Uuid, role: Role },
    /// El agente emitió un pensamiento (razonamiento).
    ThoughtEmitted { agent: String, text: String },
    /// El agente solicita ejecutar una acción/skill.
    ActionRequested { agent: String, action: String },
    /// El agente recibió una observación tras una acción.
    ObservationReceived { agent: String, summary: String },
    /// Una acción fue bloqueada por política de seguridad.
    PolicyDenied { reason: String },
}

/// Bus de eventos basado en broadcast. Múltiples suscriptores reciben cada evento.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<AionEvent>,
}

impl EventBus {
    /// Crea un bus con la capacidad de buffer indicada.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publica un evento. Ignora el caso de no haber suscriptores.
    pub fn publish(&self, event: AionEvent) {
        let _ = self.tx.send(event);
    }

    /// Crea un nuevo suscriptor.
    pub fn subscribe(&self) -> broadcast::Receiver<AionEvent> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bus_delivers_events_to_subscribers() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        bus.publish(AionEvent::ThoughtEmitted {
            agent: "test".into(),
            text: "hola".into(),
        });
        let ev = rx.recv().await.unwrap();
        matches!(ev, AionEvent::ThoughtEmitted { .. });
    }
}
