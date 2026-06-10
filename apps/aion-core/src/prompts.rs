//! **Prompts dinámicos**: biblioteca de "modos" (personas/instrucciones) + un
//! router que elige el adecuado según lo que el usuario necesita. En vez de un
//! prompt fijo, AION aplica el prompt que mejor encaja con la tarea (2026: el
//! prompt es un sistema, no texto fijo).

use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;

/// Devuelve la instrucción de modo (persona): la versión OPTIMIZADA por AION si
/// existe (auto-mejora persistida), o el valor por defecto.
pub fn persona(task: &str) -> String {
    if let Some(opt) = crate::prompt_store::current(task) {
        return opt;
    }
    persona_default(task).to_string()
}

/// Instrucción base por defecto (semilla que AION puede optimizar luego).
pub fn persona_default(task: &str) -> &'static str {
    match task {
        "investigacion" => {
            "MODO INVESTIGACIÓN: prioriza buscar en internet (web_search) y \
             leer fuentes (web_fetch); sé riguroso y cita de dónde sacas la información."
        }
        "creativo" => {
            "MODO CREATIVO: piensa de forma divergente y original; combina ideas y tu \
             conocimiento de formas inesperadas pero útiles; ofrece varias opciones."
        }
        "tecnico" => {
            "MODO TÉCNICO: sé preciso y estructurado; da pasos concretos, código o datos \
             exactos; nada de relleno."
        }
        "analisis" => {
            "MODO ANÁLISIS: razona paso a paso, contempla alternativas, riesgos y \
             trade-offs antes de concluir."
        }
        _ => "MODO CONVERSACIÓN: claro, directo y cercano, sin rodeos.",
    }
}

/// Clasifica la petición en una tarea para elegir el prompt. Heurística rápida
/// primero (sin latencia); si es ambigua, un clasificador LLM minúsculo.
pub async fn route(engine: &dyn LlmEngine, prompt: &str) -> String {
    let p = prompt.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| p.contains(w));

    if has(&[
        "investiga",
        "busca en internet",
        "buscar",
        "últimas noticias",
        "averigua",
        "fuentes",
    ]) {
        return "investigacion".into();
    }
    if has(&[
        "crea",
        "idea",
        "imagina",
        "inventa",
        "diseña",
        "brainstorm",
        "propón ideas",
        "creativo",
    ]) {
        return "creativo".into();
    }
    if has(&[
        "código",
        "codigo",
        "programa",
        "función",
        "funcion",
        "bug",
        "error",
        "implementa",
        "script",
    ]) {
        return "tecnico".into();
    }
    if has(&[
        "analiza",
        "compara",
        "pros y contras",
        "evalúa",
        "evalua",
        "por qué",
        "por que",
    ]) {
        return "analisis".into();
    }

    // Atajo: frases cortas/casuales → conversación directa, sin gastar una llamada LLM.
    if p.split_whitespace().count() < 12 {
        return "conversacion".into();
    }

    // Ambiguo → clasificador LLM minúsculo.
    let req = GenerateRequest {
        messages: vec![Message::user(format!(
            "Clasifica esta petición en UNA sola palabra de esta lista: \
             conversacion, investigacion, creativo, tecnico, analisis.\nPetición: {prompt}\nPalabra:"
        ))],
        think: false,
        temperature: Some(0.0),
        max_tokens: Some(6),
    };
    match engine.generate(req).await {
        Ok(m) => {
            let w = m.content.to_lowercase();
            for k in [
                "investigacion",
                "creativo",
                "tecnico",
                "analisis",
                "conversacion",
            ] {
                if w.contains(k) {
                    return k.into();
                }
            }
            "conversacion".into()
        }
        Err(_) => "conversacion".into(),
    }
}
