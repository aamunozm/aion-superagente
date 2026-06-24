//! Herramientas que el agente puede invocar, y su registro.
//!
//! En F3 las skills auto-generadas (WASM/Extism) se exponen como `Tool`. Aquí,
//! herramientas nativas de ejemplo (calculadora) que ya dan capacidades reales.

use crate::calc;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ─── Categoría ────────────────────────────────────────────────────────────────

/// Categoría funcional de una herramienta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCategory {
    Memory,
    Filesystem,
    Network,
    Web,
    Browser,
    System,
    Creation,
    Intelligence,
    External,
    Computation,
}

// ─── Métricas ─────────────────────────────────────────────────────────────────

/// Métricas acumuladas de uso de una herramienta individual.
#[derive(Debug, Default, Clone)]
pub struct ToolMetrics {
    pub total_calls: u64,
    pub success_count: u64,
    pub total_latency_ms: u64,
    /// Reputación bayesiana en [0, 1]. Prior 0.8 con peso 10.
    pub reputation: f64,
}

impl ToolMetrics {
    /// Tasa de éxito observada. Devuelve 1.0 si aún no hay llamadas.
    pub fn success_rate(&self) -> f64 {
        if self.total_calls == 0 {
            1.0
        } else {
            self.success_count as f64 / self.total_calls as f64
        }
    }

    /// Latencia media en ms. Devuelve 0.0 si no hay llamadas.
    pub fn avg_latency_ms(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_latency_ms as f64 / self.total_calls as f64
        }
    }

    /// Actualiza la reputación usando estimación bayesiana con prior 0.8 y peso 10.
    /// reputation = (success_count + 8) / (total_calls + 10)
    pub fn update_reputation(&mut self) {
        self.reputation = (self.success_count as f64 + 8.0) / (self.total_calls as f64 + 10.0);
    }
}

// ─── Trait Tool ───────────────────────────────────────────────────────────────

/// Una herramienta invocable por el agente.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn run(&self, input: &str) -> Result<String, String>;

    /// Si esta invocación requiere CONFIRMACIÓN del usuario (acción sensible: login,
    /// compra/pago, algo irreversible), devuelve la descripción a mostrar. El bucle
    /// del agente pedirá el OK antes de ejecutar (human-in-the-loop).
    fn needs_confirm(&self, _input: &str) -> Option<String> {
        None
    }

    /// Esquema JSON de la herramienta (OpenAI function-calling compatible).
    /// Implementación por defecto: esquema vacío / no declarado.
    fn schema(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Categoría funcional de la herramienta.
    /// Implementación por defecto: `Computation`.
    fn category(&self) -> ToolCategory {
        ToolCategory::Computation
    }
}

// ─── Registro ─────────────────────────────────────────────────────────────────

/// Registro de herramientas disponibles para el agente.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
    // Mutabilidad interior: el bucle del agente sostiene `&ToolRegistry` (inmutable) y aun
    // así graba el resultado de cada llamada (reputación bayesiana, estilo Hermes). Arc para
    // que el registro siga siendo Clone; Mutex porque la contención es nula (una escritura
    // corta por herramienta ejecutada).
    metrics: Arc<Mutex<HashMap<String, ToolMetrics>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra una herramienta e inicializa sus métricas.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.metrics
            .lock()
            .unwrap()
            .entry(name.clone())
            .or_default();
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Número de herramientas registradas.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Registra el resultado de una invocación y actualiza la reputación.
    /// Toma `&self` (mutabilidad interior) para poder grabar desde el bucle del agente.
    pub fn record_call(&self, name: &str, success: bool, latency_ms: u64) {
        let mut g = self.metrics.lock().unwrap();
        let m = g.entry(name.to_string()).or_default();
        m.total_calls += 1;
        if success {
            m.success_count += 1;
        }
        m.total_latency_ms += latency_ms;
        m.update_reputation();
    }

    /// Instantánea de métricas ordenadas por reputación descendente.
    pub fn metrics_snapshot(&self) -> Vec<(String, ToolMetrics)> {
        let mut v: Vec<(String, ToolMetrics)> = self
            .metrics
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        v.sort_by(|a, b| {
            b.1.reputation
                .partial_cmp(&a.1.reputation)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v
    }

    /// Lista herramientas ordenadas por reputación descendente con porcentaje.
    /// Útil para modo diagnóstico/debug.
    pub fn describe_sorted_by_reputation(&self) -> String {
        let metrics = self.metrics.lock().unwrap();
        let mut entries: Vec<(&str, f64)> = self
            .tools
            .keys()
            .map(|name| {
                let rep = metrics.get(name).map(|m| m.reputation).unwrap_or(0.8);
                (name.as_str(), rep)
            })
            .collect();
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
            .into_iter()
            .map(|(name, rep)| {
                let desc = self.tools.get(name).map(|t| t.description()).unwrap_or("");
                format!("- {} [{:.0}%]: {}", name, rep * 100.0, desc)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Selección adaptativa: devuelve las `n` herramientas más relevantes para
    /// la consulta dada. La relevancia combina coincidencia de palabras clave
    /// (nombre + descripción) con la reputación como desempate.
    pub fn top_tools_for(&self, query: &str, n: usize) -> Vec<Arc<dyn Tool>> {
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower.split_whitespace().collect();

        let metrics = self.metrics.lock().unwrap();
        let mut scored: Vec<(usize, f64, Arc<dyn Tool>)> = self
            .tools
            .values()
            .map(|tool| {
                let haystack = format!("{} {}", tool.name(), tool.description()).to_lowercase();
                let hits = keywords.iter().filter(|kw| haystack.contains(*kw)).count();
                let rep = metrics
                    .get(tool.name())
                    .map(|m| m.reputation)
                    .unwrap_or(0.8);
                (hits, rep, Arc::clone(tool))
            })
            .collect();

        // Ordenar por (hits desc, reputación desc)
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
        });

        scored.into_iter().take(n).map(|(_, _, t)| t).collect()
    }

    /// Descripción para el prompt del agente.
    ///
    /// Las herramientas con reputación < 0.6 y más de 5 llamadas acumuladas
    /// reciben el marcador `[⚠]` en la línea de descripción.
    pub fn describe(&self) -> String {
        // Descripción COMPLETA de cada herramienta: incluye el FORMATO DE ENTRADA (p. ej.
        // files_list «escritorio pdf»). Recortarla rompía las llamadas (el modelo no sabía
        // cómo invocar la herramienta → fallaba y daba vueltas). La latencia se reduce por
        // otras vías (vía rápida conversacional, KV q8, modelo caliente), no aquí.
        let metrics = self.metrics.lock().unwrap();
        self.tools
            .values()
            .map(|t| {
                let name = t.name();
                let warn = match metrics.get(name) {
                    Some(m) if m.total_calls > 5 && m.reputation < 0.6 => " [⚠]",
                    _ => "",
                };
                format!("- {}: {}{}", name, t.description(), warn)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// **Progressive disclosure**: descripción del catálogo ENFOCADA en una tarea. Da la
    /// descripción COMPLETA (con su formato de entrada) de las `n` herramientas más
    /// relevantes a `query`, y lista SOLO por nombre el resto —que el agente puede invocar
    /// igualmente. Abarata el contexto cuando hay muchas herramientas (clave para LLMs
    /// locales con ventana modesta). Con `≤ n` herramientas equivale a [`describe`](Self::describe).
    pub fn describe_relevant(&self, query: &str, n: usize) -> String {
        if self.tools.len() <= n {
            return self.describe();
        }
        // `top_tools_for` toma y libera su propio lock antes de volver: sin solape con el de abajo.
        let top: std::collections::BTreeSet<String> = self
            .top_tools_for(query, n)
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        let metrics = self.metrics.lock().unwrap();
        let mut full: Vec<String> = Vec::new();
        let mut rest: Vec<&str> = Vec::new();
        for t in self.tools.values() {
            let name = t.name();
            if top.contains(name) {
                let warn = match metrics.get(name) {
                    Some(m) if m.total_calls > 5 && m.reputation < 0.6 => " [⚠]",
                    _ => "",
                };
                full.push(format!("- {}: {}{}", name, t.description(), warn));
            } else {
                rest.push(name);
            }
        }
        let mut out = full.join("\n");
        if !rest.is_empty() {
            out.push_str(&format!(
                "\n\nOtras herramientas que también tienes (invócalas por su nombre si la tarea lo pide): {}",
                rest.join(", ")
            ));
        }
        out
    }
}

// ─── Calculadora ──────────────────────────────────────────────────────────────

/// Calculadora aritmética determinista. Corrige la incapacidad del LLM para
/// la aritmética exacta delegando el cálculo a código.
pub struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }
    fn description(&self) -> &str {
        "Evalúa una expresión aritmética (+ - * / y paréntesis). Entrada: la expresión, p.ej. 47*89-1234"
    }
    async fn run(&self, input: &str) -> Result<String, String> {
        calc::eval(input).map(|v| {
            if v.fract() == 0.0 {
                format!("{}", v as i64)
            } else {
                format!("{v}")
            }
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Computation
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "string",
            "description": "arithmetic expression"
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn calculator_tool_runs() {
        let t = CalculatorTool;
        assert_eq!(t.run("47*89-1234").await.unwrap(), "2949");
    }

    #[test]
    fn registry_describes_tools() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        assert!(r.describe().contains("calculator"));
        assert!(r.get("calculator").is_some());
    }

    #[test]
    fn metrics_bayesian_reputation() {
        let mut m = ToolMetrics::default();
        // Sin llamadas: tasa de éxito = 1.0, reputación = 8/10 = 0.8
        assert_eq!(m.success_rate(), 1.0);
        m.update_reputation();
        assert!((m.reputation - 0.8).abs() < 1e-9);

        // 10 llamadas, 5 éxitos → (5+8)/(10+10) = 0.65
        m.total_calls = 10;
        m.success_count = 5;
        m.update_reputation();
        assert!((m.reputation - 0.65).abs() < 1e-9);
    }

    #[test]
    fn registry_record_call_and_warn() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));

        // 6 llamadas, todas fallidas → reputación baja
        for _ in 0..6 {
            r.record_call("calculator", false, 10);
        }
        let desc = r.describe();
        assert!(desc.contains("[⚠]"), "debería mostrar advertencia: {desc}");
    }

    #[test]
    fn metrics_snapshot_sorted_by_reputation() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        r.record_call("calculator", true, 5);
        let snap = r.metrics_snapshot();
        assert!(!snap.is_empty());
        // Verificar que está ordenado desc
        for w in snap.windows(2) {
            assert!(w[0].1.reputation >= w[1].1.reputation);
        }
    }

    #[test]
    fn describe_sorted_by_reputation_contains_percentage() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        let desc = r.describe_sorted_by_reputation();
        assert!(desc.contains("calculator"));
        assert!(desc.contains('%'), "debe mostrar porcentaje: {desc}");
    }

    #[test]
    fn top_tools_for_returns_relevant() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        // "aritmética" aparece en la descripción
        let top = r.top_tools_for("expresión aritmética", 3);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].name(), "calculator");
    }

    #[test]
    fn top_tools_for_empty_query_returns_all_up_to_n() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        // Sin coincidencias de keywords → 0 hits, igual devuelve la herramienta
        let top = r.top_tools_for("xyz irrelevante", 5);
        // Con 0 hits simplemente ordena por reputación; puede devolver todas
        assert!(top.len() <= 5);
    }

    #[test]
    fn calculator_schema_is_valid() {
        let t = CalculatorTool;
        let s = t.schema();
        assert_eq!(s["type"], "string");
        assert_eq!(s["description"], "arithmetic expression");
    }

    struct Dummy(&'static str, &'static str);
    #[async_trait]
    impl Tool for Dummy {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            self.1
        }
        async fn run(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
    }

    #[test]
    fn describe_relevant_focuses_and_lists_rest() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        r.register(Arc::new(Dummy(
            "weather",
            "clima y temperatura actual de una ciudad",
        )));
        r.register(Arc::new(Dummy(
            "files_list",
            "lista archivos de una carpeta del usuario",
        )));
        // n=1 con consulta de clima → 'weather' con descripción completa; el resto por nombre.
        let d = r.describe_relevant("qué clima hace hoy", 1);
        assert!(
            d.contains("- weather: clima"),
            "la relevante va completa: {d}"
        );
        assert!(
            d.contains("Otras herramientas"),
            "el resto se lista por nombre: {d}"
        );
        // una no-relevante NO aparece con su descripción completa
        assert!(
            !d.contains("- files_list: lista archivos"),
            "no debe ir completa: {d}"
        );
    }

    #[test]
    fn describe_relevant_small_registry_equals_describe() {
        let mut r = ToolRegistry::new();
        r.register(Arc::new(CalculatorTool));
        // Con pocas herramientas (≤ n) se comporta igual que describe(): cero pérdida.
        assert_eq!(r.describe_relevant("lo que sea", 5), r.describe());
    }

    #[test]
    fn registry_len_counts() {
        let mut r = ToolRegistry::new();
        assert_eq!(r.len(), 0);
        r.register(Arc::new(CalculatorTool));
        assert_eq!(r.len(), 1);
    }
}
