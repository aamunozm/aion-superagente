//! # aion-cognition
//!
//! Subsistemas cognitivos de AION (basados en investigación verificada, jun 2026):
//!
//! - [`CuriosityEngine`] — **motivación intrínseca por *learning progress*** (LP),
//!   la señal canónica de curiosidad (línea Oudeyer/FLOWERS, MAGELLAN): el agente
//!   prioriza objetivos donde *está mejorando*, evitando los ya dominados (LP≈0)
//!   y los inabordables (LP≈0).
//! - [`SelfModel`] — auto-modelo barato: el agente estima su propia competencia
//!   (inspirado en el beneficio comprobado del self-modeling, Royal Society A).
//! - [`Calibration`] — metacognición: ¿qué tan calibrada está su confianza?
//!   (Brier score). Honesto sobre sus límites.
//!
//! Todo determinista y testeable; alimenta al orquestador para decidir QUÉ
//! aprender a continuación (auto-objetivos) y cuánto fiarse de sí mismo.

mod curiosity;
mod metacognition;
mod self_model;

pub use curiosity::CuriosityEngine;
pub use metacognition::Calibration;
pub use self_model::SelfModel;
