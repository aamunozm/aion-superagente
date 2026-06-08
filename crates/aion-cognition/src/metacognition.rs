//! Metacognición: calibración de confianza. ¿Cuando AION dice estar 80% seguro,
//! acierta el 80% de las veces? Se mide con el **Brier score** (0 = perfecto,
//! 0.25 = azar, 1 = pésimo). Mantiene a AION honesto sobre sus propios límites.

pub struct Calibration {
    records: Vec<(f32, bool)>, // (confianza predicha 0..1, resultado real)
}

impl Default for Calibration {
    fn default() -> Self {
        Self::new()
    }
}

impl Calibration {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Registra una predicción: confianza declarada y si acertó.
    pub fn record(&mut self, confidence: f32, correct: bool) {
        self.records.push((confidence.clamp(0.0, 1.0), correct));
    }

    pub fn samples(&self) -> usize {
        self.records.len()
    }

    /// Brier score = media de (confianza − resultado)². Menor = mejor calibrado.
    pub fn brier_score(&self) -> Option<f32> {
        if self.records.is_empty() {
            return None;
        }
        let s: f32 = self
            .records
            .iter()
            .map(|&(c, ok)| {
                let o = if ok { 1.0 } else { 0.0 };
                (c - o) * (c - o)
            })
            .sum();
        Some(s / self.records.len() as f32)
    }

    /// Veredicto legible de la calibración.
    pub fn verdict(&self) -> String {
        match self.brier_score() {
            None => "sin datos de calibración".into(),
            Some(b) if b <= 0.1 => format!("bien calibrado (Brier {b:.3})"),
            Some(b) if b <= 0.25 => format!("calibración aceptable (Brier {b:.3})"),
            Some(b) => format!("mal calibrado — exceso de confianza (Brier {b:.3})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_calibration_low_brier() {
        let mut c = Calibration::new();
        c.record(1.0, true);
        c.record(0.0, false);
        c.record(1.0, true);
        assert!(c.brier_score().unwrap() < 1e-6);
        assert!(c.verdict().contains("bien calibrado"));
    }

    #[test]
    fn overconfident_high_brier() {
        let mut c = Calibration::new();
        for _ in 0..10 {
            c.record(0.99, false); // muy seguro y siempre se equivoca
        }
        assert!(c.brier_score().unwrap() > 0.9);
        assert!(c.verdict().contains("mal calibrado"));
    }
}
