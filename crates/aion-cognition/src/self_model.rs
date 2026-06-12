//! Auto-modelo barato: el agente mantiene una estimación de su propia competencia
//! global mediante una media móvil exponencial de sus resultados. Es "auto-percepción"
//! ligera, inspirada en el beneficio comprobado del self-modeling.

pub struct SelfModel {
    competence: f32,
    alpha: f32,
    observations: u64,
}

impl Default for SelfModel {
    fn default() -> Self {
        Self::new(0.2)
    }
}

impl SelfModel {
    /// `alpha` = factor de la media móvil exponencial (0..1); mayor = más reactivo.
    pub fn new(alpha: f32) -> Self {
        Self {
            competence: 0.5,
            alpha: alpha.clamp(0.01, 1.0),
            observations: 0,
        }
    }

    /// Restaura un auto-modelo persistido (competencia + observaciones previas),
    /// para que la auto-percepción sobreviva a los reinicios.
    pub fn from_state(competence: f32, observations: u64) -> Self {
        Self {
            competence: competence.clamp(0.0, 1.0),
            alpha: 0.2,
            observations,
        }
    }

    /// Observa un resultado propio (éxito/fallo) y actualiza la auto-estimación.
    pub fn observe(&mut self, success: bool) {
        let target = if success { 1.0 } else { 0.0 };
        self.competence += self.alpha * (target - self.competence);
        self.observations += 1;
    }

    pub fn competence(&self) -> f32 {
        self.competence
    }

    pub fn observations(&self) -> u64 {
        self.observations
    }

    /// Introspección: descripción honesta del estado del propio agente.
    pub fn introspect(&self) -> String {
        let nivel = match self.competence {
            c if c >= 0.8 => "alta",
            c if c >= 0.5 => "media",
            _ => "baja",
        };
        format!(
            "Auto-percepción: competencia {nivel} ({:.0}%) tras {} observaciones",
            self.competence * 100.0,
            self.observations
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competence_rises_with_success() {
        let mut m = SelfModel::new(0.3);
        for _ in 0..20 {
            m.observe(true);
        }
        assert!(m.competence() > 0.9);
        assert_eq!(m.observations(), 20);
        assert!(m.introspect().contains("alta"));
    }

    #[test]
    fn competence_falls_with_failure() {
        let mut m = SelfModel::new(0.3);
        for _ in 0..20 {
            m.observe(false);
        }
        assert!(m.competence() < 0.1);
    }
}
