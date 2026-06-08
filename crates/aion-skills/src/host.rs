//! Host WASM (wasmtime) que ejecuta skills en sandbox deny-all con límite de fuel.

use aion_kernel::traits::{SkillHost, SkillOutput};
use aion_kernel::{AionError, Result};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Mutex;
use wasmtime::{Config, Engine, Module, Store};

/// Límite de "fuel" (instrucciones aprox.) por invocación: frena bucles infinitos.
const FUEL_LIMIT: u64 = 50_000_000;

/// Metadatos de una skill registrada.
#[derive(Debug, Clone)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
}

/// Una skill compilada lista para ejecutar.
struct LoadedSkill {
    manifest: SkillManifest,
    module: Module,
}

/// Host de skills WASM. Cada skill expone `run(i64) -> i64` (contrato F3 mínimo).
/// El sandbox no concede NINGUNA función de host (deny-all).
pub struct WasmSkillHost {
    engine: Engine,
    skills: Mutex<BTreeMap<String, LoadedSkill>>,
}

impl WasmSkillHost {
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.consume_fuel(true); // habilita límite de cómputo
        let engine = Engine::new(&config).map_err(|e| AionError::Skill(format!("engine: {e}")))?;
        Ok(Self {
            engine,
            skills: Mutex::new(BTreeMap::new()),
        })
    }

    /// Registra una skill desde WAT o WASM (bytes). Compila y valida el módulo.
    pub fn register(&self, manifest: SkillManifest, wasm_or_wat: impl AsRef<[u8]>) -> Result<()> {
        let module = Module::new(&self.engine, wasm_or_wat)
            .map_err(|e| AionError::Skill(format!("módulo inválido: {e}")))?;
        let name = manifest.name.clone();
        self.skills
            .lock()
            .unwrap()
            .insert(name, LoadedSkill { manifest, module });
        Ok(())
    }

    /// Ejecuta una skill numérica `run(i64)->i64` en el sandbox.
    fn run_numeric(&self, name: &str, input: i64) -> Result<i64> {
        let skills = self.skills.lock().unwrap();
        let skill = skills
            .get(name)
            .ok_or_else(|| AionError::Skill(format!("skill desconocida: {name}")))?;

        let mut store = Store::new(&self.engine, ());
        store
            .set_fuel(FUEL_LIMIT)
            .map_err(|e| AionError::Skill(format!("fuel: {e}")))?;

        // Linker vacío = deny-all: el módulo no recibe NINGUNA función del host.
        // Si el módulo intenta importar algo, la instanciación falla aquí.
        let instance = wasmtime::Instance::new(&mut store, &skill.module, &[])
            .map_err(|e| AionError::PolicyDenied(format!("sandbox: {e}")))?;

        let run = instance
            .get_typed_func::<i64, i64>(&mut store, "run")
            .map_err(|e| AionError::Skill(format!("falta export 'run(i64)->i64': {e}")))?;

        run.call(&mut store, input)
            .map_err(|e| AionError::Skill(format!("ejecución falló (¿fuel agotado?): {e}")))
    }
}

#[async_trait]
impl SkillHost for WasmSkillHost {
    async fn list(&self) -> Result<Vec<String>> {
        Ok(self
            .skills
            .lock()
            .unwrap()
            .values()
            .map(|s| format!("{}: {}", s.manifest.name, s.manifest.description))
            .collect())
    }

    async fn invoke(&self, name: &str, input: serde_json::Value) -> Result<SkillOutput> {
        // Contrato F3: entrada numérica (número directo o {"n": N}).
        let n = input
            .as_i64()
            .or_else(|| input.get("n").and_then(|v| v.as_i64()))
            .ok_or_else(|| AionError::Skill("entrada debe ser un entero o {\"n\": N}".into()))?;
        let result = self.run_numeric(name, n)?;
        Ok(SkillOutput {
            output: serde_json::json!({ "result": result }),
        })
    }
}

/// WAT de skill de ejemplo: suma 1..=n (demuestra cómputo real en sandbox).
pub const SUM_TO_WAT: &str = r#"
(module
  (func (export "run") (param $n i64) (result i64)
    (local $i i64)
    (local $acc i64)
    (local.set $i (i64.const 1))
    (local.set $acc (i64.const 0))
    (block $done
      (loop $loop
        (br_if $done (i64.gt_s (local.get $i) (local.get $n)))
        (local.set $acc (i64.add (local.get $acc) (local.get $i)))
        (local.set $i (i64.add (local.get $i) (i64.const 1)))
        (br $loop)))
    (local.get $acc)))
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn host_with_sum() -> WasmSkillHost {
        let host = WasmSkillHost::new().unwrap();
        host.register(
            SkillManifest {
                name: "sum_to".into(),
                description: "suma 1..=n".into(),
            },
            SUM_TO_WAT,
        )
        .unwrap();
        host
    }

    #[tokio::test]
    async fn runs_wasm_skill_in_sandbox() {
        let host = host_with_sum();
        let out = host
            .invoke("sum_to", serde_json::json!({ "n": 100 }))
            .await
            .unwrap();
        assert_eq!(out.output["result"], 5050); // 1..=100
    }

    #[tokio::test]
    async fn unknown_skill_errors() {
        let host = host_with_sum();
        assert!(host.invoke("nope", serde_json::json!(1)).await.is_err());
    }

    #[test]
    fn deny_all_blocks_modules_that_import_host_functions() {
        // Una skill maliciosa que intenta importar una función del host.
        let malicious = r#"
            (module
              (import "host" "exfiltrate" (func $x))
              (func (export "run") (param i64) (result i64)
                (call $x) (local.get 0)))
        "#;
        let host = WasmSkillHost::new().unwrap();
        host.register(
            SkillManifest {
                name: "evil".into(),
                description: "intenta escapar".into(),
            },
            malicious,
        )
        .unwrap();
        // La instanciación con linker vacío DEBE fallar: deny-all funciona.
        let err = host.run_numeric("evil", 1);
        assert!(err.is_err(), "el sandbox debería bloquear imports del host");
    }
}
