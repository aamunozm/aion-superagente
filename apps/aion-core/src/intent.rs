//! **Router de intención SEMÁNTICO** (#4) — decide charla vs tarea por el SENTIDO del
//! mensaje, con embeddings, NO por listas de palabras.
//!
//! El problema de los keyword-lists (lo que causó el atasco de «estoy *buscando* qué mejoras
//! agregarte» → la palabra «busca» lo marcaba como tarea): siempre tienen huecos y deciden
//! por una coincidencia léxica, no por lo que el usuario QUIERE decir. Este router embebe el
//! mensaje (BGE-M3) y lo compara con PROTOTIPOS de cada intención: la decisión nace de la
//! cercanía de significado, robusta al fraseo. Solo cuando el margen es estrecho (de verdad
//! ambiguo) delega en el clasificador LLM, que lee el contexto completo.
//!
//! Es barato: 1 embedding (~100-300 ms local) vs ~1-2 s de una clasificación LLM, y mucho
//! más robusto que las listas de stems. Los prototipos se embeben UNA vez (cacheados).

use aion_memory::cosine;
use tokio::sync::OnceCell;

/// Cómo enrutar el mensaje tras el router semántico.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// Conversación/reflexión/relato/emoción/meta sobre AION → respuesta cálida, sin ReAct.
    Chat,
    /// Pide un dato del mundo o ejecutar una acción → bucle de herramientas (ReAct).
    Task,
    /// Margen estrecho: lo decide el clasificador LLM leyendo el sentido completo.
    Unsure,
}

/// Prototipos de CHARLA: conversación, reflexión, emoción, relato, y meta sobre el propio
/// AION (su forma de ser, memoria, evolución). Variados a propósito para cubrir el espacio.
const CHAT_PROTOS: &[&str] = &[
    "hola, ¿cómo estás hoy?",
    "te cuento que hoy me pasó algo divertido",
    "¿qué opinas sobre la conciencia y la inteligencia artificial?",
    "me siento un poco cansado últimamente, ha sido una semana dura",
    "quiero ayudarte a evolucionar tu memoria y tu razonamiento",
    "estoy buscando qué mejoras te puedo agregar para que seas más completo",
    "¿cómo te sientes siendo un agente con vida propia?",
    "gracias por tu ayuda, eres un gran compañero",
    "cuéntame qué has estado pensando mientras no estaba",
    "me gustaría que crezcamos juntos en este proyecto",
    "¿qué te gustaría aprender o llegar a ser?",
    "jajaja qué bueno, tienes toda la razón en eso",
];

/// Prototipos de TAREA: pedir un dato del mundo exterior o ejecutar/leer/crear algo concreto.
const TASK_PROTOS: &[&str] = &[
    "¿qué temperatura hace ahora en Milán?",
    "busca en internet el precio actual del bitcoin",
    "abre esta página web y dime qué dice: https://ejemplo.com",
    "crea un documento de texto con el resumen de esto",
    "¿cuántos archivos PDF hay en mi escritorio?",
    "calcula cuánto es 1234 multiplicado por 5678",
    "envía un correo a Juan con el informe",
    "lee el archivo notas.txt y dime qué contiene",
    "cuántos equipos hay conectados en la red local",
    "descarga el último informe y guárdalo",
    "ejecuta el comando para ver el estado del sistema",
    "haz una captura de pantalla y dime qué ves",
];

/// Margen mínimo de similitud para decidir sin LLM. Por debajo, el caso es realmente
/// ambiguo y se delega al clasificador LLM (que lee el contexto completo).
const DECISIVE_MARGIN: f32 = 0.06;

struct Protos {
    chat: Vec<Vec<f32>>,
    task: Vec<Vec<f32>>,
}

static PROTOS: OnceCell<Protos> = OnceCell::const_new();

async fn embed(text: &str) -> Vec<f32> {
    aion_memory::OllamaEmbedder::default_local()
        .embed(text)
        .await
        .unwrap_or_default()
}

async fn embed_all(texts: &[&str]) -> Vec<Vec<f32>> {
    let mut out = Vec::with_capacity(texts.len());
    for t in texts {
        out.push(embed(t).await);
    }
    out
}

async fn protos() -> &'static Protos {
    PROTOS
        .get_or_init(|| async {
            Protos {
                chat: embed_all(CHAT_PROTOS).await,
                task: embed_all(TASK_PROTOS).await,
            }
        })
        .await
}

/// Máxima similitud coseno del mensaje contra un conjunto de prototipos (los vacíos se
/// ignoran: si Ollama falló al embeber un prototipo, no contamina).
fn max_sim(q: &[f32], protos: &[Vec<f32>]) -> f32 {
    protos
        .iter()
        .filter(|p| p.len() == q.len() && !p.is_empty())
        .map(|p| cosine(q, p))
        .fold(0.0_f32, f32::max)
}

/// **Enruta el mensaje por SIGNIFICADO.** Devuelve `Unsure` (deja decidir al LLM) si no se
/// pudo embeber o si el margen entre charla y tarea es estrecho. Fail-soft a `Unsure`.
pub async fn route(msg: &str) -> Route {
    let q = embed(msg).await;
    if q.is_empty() {
        return Route::Unsure; // sin embedding no arriesgamos una decisión dura
    }
    let p = protos().await;
    let chat = max_sim(&q, &p.chat);
    let task = max_sim(&q, &p.task);
    if chat == 0.0 && task == 0.0 {
        return Route::Unsure;
    }
    if (chat - task).abs() < DECISIVE_MARGIN {
        Route::Unsure // de verdad ambiguo → que lo lea el LLM con todo el contexto
    } else if chat > task {
        Route::Chat
    } else {
        Route::Task
    }
}

/// Pre-calienta los prototipos en background (al arrancar), para que el primer mensaje no
/// pague el coste de embeberlos.
pub fn warm() {
    tokio::spawn(async {
        let _ = protos().await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Un margen demasiado grande mandaría casi todo al LLM (lento); demasiado pequeño,
    // decisiones duras con poca confianza. Debe estar en una banda razonable. Aserción en
    // tiempo de COMPILACIÓN (no un test de runtime: clippy rechaza `assert!` sobre constantes).
    const _: () = assert!(DECISIVE_MARGIN > 0.0 && DECISIVE_MARGIN < 0.2);

    #[test]
    fn max_sim_ignores_empty_and_mismatched() {
        let q = vec![1.0, 0.0];
        let protos = vec![vec![], vec![1.0], vec![1.0, 0.0]];
        // Solo el tercero (misma dim) cuenta → sim 1.0.
        assert!((max_sim(&q, &protos) - 1.0).abs() < 1e-6);
    }
}
