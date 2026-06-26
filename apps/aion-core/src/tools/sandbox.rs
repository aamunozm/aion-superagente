//! CodeSandboxTool — ejecuta código Python o JavaScript en un subproceso aislado.

use aion_orchestrator::{Tool, ToolCategory};
use async_trait::async_trait;
use std::time::Duration;
use tokio::process::Command;

/// Patrones que activan confirmación por ser operaciones de sistema potencialmente peligrosas.
const DANGEROUS_PYTHON: &[&str] = &["os.system", "subprocess", "eval(", "exec(", "__import__"];
const DANGEROUS_JS: &[&str] = &[
    "require(\"fs\")",
    "require('fs')",
    "child_process",
    "eval(",
    "exec(",
];

/// Timeout máximo de ejecución por sandbox.
const TIMEOUT_SECS: u64 = 10;

/// PATH restringido para no heredar el entorno completo del usuario.
const SAFE_PATH: &str = "/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin";

/// Ejecuta fragmentos de código Python 3 o JavaScript (Node.js) con timeout de 10s.
///
/// Entrada: `"python ::: código"` o `"js ::: código"`.
/// Pide confirmación si el código contiene llamadas peligrosas al sistema.
pub struct CodeSandboxTool;

impl CodeSandboxTool {
    fn is_dangerous_python(code: &str) -> bool {
        DANGEROUS_PYTHON.iter().any(|pat| code.contains(pat))
    }

    fn is_dangerous_js(code: &str) -> bool {
        DANGEROUS_JS.iter().any(|pat| code.contains(pat))
    }

    async fn run_python(code: &str) -> Result<String, String> {
        let result = tokio::time::timeout(
            Duration::from_secs(TIMEOUT_SECS),
            Command::new("python3")
                .arg("-c")
                .arg(code)
                .env_clear()
                .env("PATH", SAFE_PATH)
                .env("HOME", std::env::var("HOME").unwrap_or_default())
                .output(),
        )
        .await;

        match result {
            Err(_) => Err(format!(
                "Tiempo de ejecución agotado ({TIMEOUT_SECS}s). El código tardó demasiado."
            )),
            Ok(Err(e)) => Err(format!("Error iniciando python3: {e}. ¿Está instalado?")),
            Ok(Ok(out)) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();

                if out.status.success() {
                    if stdout.is_empty() {
                        Ok("(sin salida)".to_string())
                    } else {
                        Ok(stdout)
                    }
                } else {
                    let exit_code = out.status.code().unwrap_or(-1);
                    let mut msg = format!("Python salió con código {exit_code}.");
                    if !stderr.is_empty() {
                        msg.push_str(&format!("\nError:\n{stderr}"));
                    }
                    if !stdout.is_empty() {
                        msg.push_str(&format!("\nSalida parcial:\n{stdout}"));
                    }
                    Err(msg)
                }
            }
        }
    }

    async fn run_js(code: &str) -> Result<String, String> {
        let result = tokio::time::timeout(
            Duration::from_secs(TIMEOUT_SECS),
            Command::new("node")
                .arg("-e")
                .arg(code)
                .env_clear()
                .env("PATH", SAFE_PATH)
                .env("HOME", std::env::var("HOME").unwrap_or_default())
                .output(),
        )
        .await;

        match result {
            Err(_) => Err(format!(
                "Tiempo de ejecución agotado ({TIMEOUT_SECS}s). El código tardó demasiado."
            )),
            Ok(Err(e)) => Err(format!(
                "Error iniciando node: {e}. ¿Está instalado Node.js?"
            )),
            Ok(Ok(out)) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();

                if out.status.success() {
                    if stdout.is_empty() {
                        Ok("(sin salida)".to_string())
                    } else {
                        Ok(stdout)
                    }
                } else {
                    let exit_code = out.status.code().unwrap_or(-1);
                    let mut msg = format!("Node salió con código {exit_code}.");
                    if !stderr.is_empty() {
                        msg.push_str(&format!("\nError:\n{stderr}"));
                    }
                    if !stdout.is_empty() {
                        msg.push_str(&format!("\nSalida parcial:\n{stdout}"));
                    }
                    Err(msg)
                }
            }
        }
    }
}

#[async_trait]
impl Tool for CodeSandboxTool {
    fn name(&self) -> &str {
        "code_sandbox"
    }

    fn description(&self) -> &str {
        "Ejecuta código Python 3 o JavaScript (Node.js) en un subproceso aislado con timeout de 10s. \
        Entrada: «python ::: código» o «js ::: código». \
        Captura stdout y stderr. Pide confirmación si el código usa llamadas al sistema."
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Computation
    }

    fn needs_confirm(&self, input: &str) -> Option<String> {
        if let Some((lang, code)) = input.split_once(":::") {
            let lang = lang.trim().to_lowercase();
            let code = code.trim();
            let is_dangerous = match lang.as_str() {
                "python" | "py" => Self::is_dangerous_python(code),
                "js" | "javascript" | "node" => Self::is_dangerous_js(code),
                _ => false,
            };
            if is_dangerous {
                return Some(format!(
                    "El código {lang} contiene llamadas potencialmente peligrosas al sistema. ¿Ejecutar de todas formas?"
                ));
            }
        }
        None
    }

    async fn run(&self, input: &str) -> Result<String, String> {
        let (lang, code) = input.split_once(":::").ok_or_else(|| {
            "Formato incorrecto. Usa «python ::: código» o «js ::: código».".to_string()
        })?;

        let lang = lang.trim().to_lowercase();
        let code = code.trim();

        if code.is_empty() {
            return Err("El código no puede estar vacío.".to_string());
        }

        match lang.as_str() {
            "python" | "py" => Self::run_python(code).await,
            "js" | "javascript" | "node" => Self::run_js(code).await,
            other => Err(format!(
                "Lenguaje «{other}» no soportado. Usa «python» o «js»."
            )),
        }
    }
}
