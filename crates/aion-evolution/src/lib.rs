//! # aion-evolution
//!
//! Bucle de **auto-mejora gated** (F5). Permite que el agente proponga nuevas
//! skills (código WASM) y las integre **solo si pasan todas las puertas de
//! seguridad** — convirtiendo la "evolución autónoma" en algo seguro:
//!
//! 1. **Sandbox**: la candidata se compila/instancia con deny-all (aion-skills).
//!    Si intenta importar funciones del host → rechazada.
//! 2. **Tests**: debe pasar su suite de tests declarada (oráculo).
//! 3. **Canary + rollback**: solo si TODO pasa se registra en el host vivo;
//!    si no, se descarta sin tocar el sistema (rollback = no-op por diseño).
//! 4. **Circuit breaker**: tras N fallos consecutivos, se abre y detiene la
//!    evolución (evita derivas / bucles destructivos — lección de la DGM).
//! 5. **Kernel inmutable**: se verifica que el contrato del kernel no cambió.

use aion_kernel::traits::SkillHost;
use aion_kernel::Result;
use aion_skills::{SkillManifest, WasmSkillHost};
use std::sync::Arc;

/// Una skill candidata propuesta para integrarse (por el agente o un humano).
pub struct Candidate {
    pub manifest: SkillManifest,
    /// Código WASM o WAT.
    pub code: String,
    /// Suite de tests (entrada, salida esperada) — el oráculo de aceptación.
    pub tests: Vec<(i64, i64)>,
}

/// Resultado de evaluar una candidata.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalReport {
    pub accepted: bool,
    pub reason: String,
    pub passed: usize,
    pub failed: usize,
}

/// Circuit breaker: se abre tras `threshold` fallos consecutivos.
pub struct CircuitBreaker {
    failures: u32,
    threshold: u32,
}

impl CircuitBreaker {
    pub fn new(threshold: u32) -> Self {
        Self {
            failures: 0,
            threshold,
        }
    }
    pub fn is_open(&self) -> bool {
        self.failures >= self.threshold
    }
    fn record_failure(&mut self) {
        self.failures += 1;
    }
    fn reset(&mut self) {
        self.failures = 0;
    }
}

/// Verifica la integridad del kernel: el contrato no debe haber cambiado bajo
/// los pies del sistema. (En F5+ se compara el hash firmado del binario.)
pub fn verify_kernel(baseline_contract: u32) -> bool {
    aion_kernel::kernel_info().contract_version == baseline_contract
}

/// Motor de evolución gated. Integra skills en `live` solo si superan las puertas.
pub struct EvolutionEngine {
    live: Arc<WasmSkillHost>,
    breaker: CircuitBreaker,
}

impl EvolutionEngine {
    pub fn new(live: Arc<WasmSkillHost>) -> Self {
        Self {
            live,
            breaker: CircuitBreaker::new(3),
        }
    }

    pub fn with_breaker(live: Arc<WasmSkillHost>, threshold: u32) -> Self {
        Self {
            live,
            breaker: CircuitBreaker::new(threshold),
        }
    }

    pub fn breaker_open(&self) -> bool {
        self.breaker.is_open()
    }

    /// Propone una candidata. Devuelve el veredicto tras pasar (o no) las puertas.
    pub async fn propose(&mut self, c: Candidate) -> Result<EvalReport> {
        // Puerta 0: circuit breaker.
        if self.breaker.is_open() {
            return Ok(EvalReport {
                accepted: false,
                reason: "circuit breaker abierto: evolución detenida".into(),
                passed: 0,
                failed: 0,
            });
        }

        // Puerta 1: compilación + sandbox deny-all en un host AISLADO (canary).
        let canary = WasmSkillHost::new()?;
        if let Err(e) = canary.register(
            SkillManifest {
                name: c.manifest.name.clone(),
                description: c.manifest.description.clone(),
            },
            &c.code,
        ) {
            self.breaker.record_failure();
            return Ok(EvalReport {
                accepted: false,
                reason: format!("rechazada en sandbox/compilación: {e}"),
                passed: 0,
                failed: 0,
            });
        }

        // Puerta 2: suite de tests contra el host aislado.
        let mut passed = 0;
        let mut failed = 0;
        for (input, expected) in &c.tests {
            let got = canary
                .invoke(&c.manifest.name, serde_json::json!(input))
                .await
                .ok()
                .and_then(|o| o.output.get("result").and_then(|v| v.as_i64()));
            if got == Some(*expected) {
                passed += 1;
            } else {
                failed += 1;
            }
        }

        if failed > 0 {
            // Rollback = no-op: nunca tocamos el host vivo.
            self.breaker.record_failure();
            return Ok(EvalReport {
                accepted: false,
                reason: format!(
                    "rechazada: {failed}/{} tests fallaron (rollback)",
                    c.tests.len()
                ),
                passed,
                failed,
            });
        }

        // Puerta 3: aceptación — registrar en el host vivo.
        self.live.register(c.manifest, &c.code)?;
        self.breaker.reset();
        Ok(EvalReport {
            accepted: true,
            reason: "aceptada: superó sandbox + todos los tests".into(),
            passed,
            failed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOUBLE_OK: &str =
        "(module (func (export \"run\") (param $n i64) (result i64) (i64.mul (local.get $n) (i64.const 2))))";
    // Dice duplicar pero suma 1: fallará los tests.
    const DOUBLE_BAD: &str =
        "(module (func (export \"run\") (param $n i64) (result i64) (i64.add (local.get $n) (i64.const 1))))";
    // Intenta importar una función del host: bloqueada por el sandbox.
    const MALICIOUS: &str = "(module (import \"host\" \"x\" (func $x)) (func (export \"run\") (param i64) (result i64) (call $x) (local.get 0)))";

    fn candidate(name: &str, code: &str) -> Candidate {
        Candidate {
            manifest: SkillManifest {
                name: name.into(),
                description: "doble".into(),
            },
            code: code.into(),
            tests: vec![(5, 10), (0, 0), (21, 42)],
        }
    }

    #[tokio::test]
    async fn accepts_correct_skill_and_registers_it() {
        let live = Arc::new(WasmSkillHost::new().unwrap());
        let mut eng = EvolutionEngine::new(live.clone());
        let report = eng.propose(candidate("double", DOUBLE_OK)).await.unwrap();
        assert!(report.accepted, "{}", report.reason);
        assert_eq!(report.passed, 3);
        // Se integró en el host vivo:
        let out = live.invoke("double", serde_json::json!(7)).await.unwrap();
        assert_eq!(out.output["result"], 14);
    }

    #[tokio::test]
    async fn rejects_skill_that_fails_tests_without_touching_live() {
        let live = Arc::new(WasmSkillHost::new().unwrap());
        let mut eng = EvolutionEngine::new(live.clone());
        let report = eng.propose(candidate("double", DOUBLE_BAD)).await.unwrap();
        assert!(!report.accepted);
        assert!(report.failed > 0);
        // NO se registró en el host vivo (rollback):
        assert!(live.invoke("double", serde_json::json!(7)).await.is_err());
    }

    #[tokio::test]
    async fn sandbox_blocks_malicious_candidate() {
        let live = Arc::new(WasmSkillHost::new().unwrap());
        let mut eng = EvolutionEngine::new(live);
        let report = eng.propose(candidate("evil", MALICIOUS)).await.unwrap();
        assert!(!report.accepted);
        assert!(report.reason.contains("sandbox") || report.reason.contains("compilación"));
    }

    #[tokio::test]
    async fn circuit_breaker_trips_after_consecutive_failures() {
        let live = Arc::new(WasmSkillHost::new().unwrap());
        let mut eng = EvolutionEngine::with_breaker(live, 2);
        let _ = eng.propose(candidate("a", DOUBLE_BAD)).await.unwrap();
        let _ = eng.propose(candidate("b", DOUBLE_BAD)).await.unwrap();
        assert!(eng.breaker_open());
        // Aun una candidata BUENA es rechazada con el breaker abierto:
        let report = eng.propose(candidate("good", DOUBLE_OK)).await.unwrap();
        assert!(!report.accepted);
        assert!(report.reason.contains("circuit breaker"));
    }

    #[test]
    fn kernel_integrity_check() {
        assert!(verify_kernel(aion_kernel::KERNEL_CONTRACT_VERSION));
        assert!(!verify_kernel(999));
    }
}
