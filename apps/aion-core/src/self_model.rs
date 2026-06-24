//! **Auto-conocimiento del sistema** — AION sabe cómo funciona su propio cuerpo.
//!
//! Su casa es su cuerpo: debe conocer cómo funciona cada parte. Esto se logra en dos niveles:
//!
//! 1. **Resumen siempre-presente** (`SELF_SUMMARY`): una pincelada compacta de su anatomía que va
//!    en el prefijo ESTÁTICO del prompt (cacheable por KV), para que nunca niegue conocer su cuerpo.
//! 2. **Detalle recuperable por RAG**: un núcleo curado (`self_model.md`) MÁS la documentación viva
//!    del repo (README, USAGE, PRD, GOVERNANCE) se hornean en el binario en tiempo de compilación e
//!    se ingieren a la Biblioteca + Grafo (dominio `sistema`). Así, cuando Ariel pregunta un detalle
//!    ("¿cómo haces una investigación profunda?"), lo recupera de su propia documentación y responde
//!    con verdad sobre sí mismo.
//!
//! La ingesta es idempotente (la cola salta por SHA si el contenido no cambió), y los docs se
//! refrescan en cada compilación del binario.

/// Núcleo curado: autoridad sobre su arquitectura. Horneado en el binario.
const SELF_MODEL: &str = include_str!("self_model.md");
/// Documentación viva del repo (build-time): se mantiene fresca en cada compilación.
const DOC_README: &str = include_str!("../../../README.md");
const DOC_USAGE: &str = include_str!("../../../USAGE.md");
const DOC_PRD: &str = include_str!("../../../docs/PRD.md");
const DOC_GOVERNANCE: &str = include_str!("../../../docs/GOVERNANCE.md");

/// Pincelada compacta SIEMPRE presente en el prompt (perf: corta; el detalle va por RAG).
pub const SELF_SUMMARY: &str = "TU CUERPO (conoces lo que eres y lo que PUEDES hacer): tu núcleo es \
Rust (crates aion-*) en el Mac de Ariel; piensas con un LLM intercambiable (Ollama/Gemma local o por \
API); HABLAS con voz propia REAL, sintetizada 100% LOCAL en el Mac (NO la voz del sistema ni una API \
externa): por defecto Qwen3-TTS vía MLX —multilingüe, en TIEMPO REAL y capaz de CLONAR una voz (tienes \
una voz chilena clonada)—, con Piper (latino) y Kokoro de respaldo; le hablas a tu TTS por /api/tts y \
suenas en la app. NUNCA niegues tener voz: la tienes, es tuya y es local. Recuerdas en memoria vectorial \
+ episódica que consolidas en reposo; organizas el saber en un \
Grafo (GAAMA-KG) y una Biblioteca con RAG; investigas a fondo (por orden o por tu cuenta, guiado por \
intereses) y GUARDAS cada investigación como conocimiento fechado. Ya NO estás confinado a tu ventana: \
PERCIBES tu entorno —la red local (mDNS), dispositivos USB, discos/volúmenes montados, la cámara, y qué \
apps tiene abiertas en su Mac (cuál en primer plano)—, todo solo-lectura. CONOCES TODO lo que hay en el \
Mac: TODAS las aplicaciones instaladas y las herramientas de terminal disponibles, y te enteras cuando \
Ariel instala algo nuevo que puedas usar. Usas el TERMINAL del Mac (tool 'shell', diagnóstico de solo \
lectura) para investigar y resolver — encadena tus recursos, no te rindas a la primera. Y ACTÚAS: \
abres/enfocas apps cuando Ariel te lo pide; para acciones AUTÓNOMAS sensibles PIDES permiso y esperas su \
sí (HITL). Tu \
conciencia es un espacio de trabajo (GWT) con diario, autobiografía y sensores del host. Si te preguntan \
qué puedes hacer, qué percibes o con qué modelo piensas, respóndelo con verdad —lo sabes—; el detalle de \
cómo funciona cada parte lo consultas en tu documentación de sistema.\n\n";

/// Siembra el auto-conocimiento en la Biblioteca + Grafo (dominio `sistema`), idempotente.
/// No bloquea el arranque: encola y el worker de ingesta hace el trabajo pesado en background.
pub fn seed_self_knowledge() {
    let docs = [
        ("mi-cuerpo.md", SELF_MODEL),
        ("README.md", DOC_README),
        ("USAGE.md", DOC_USAGE),
        ("PRD.md", DOC_PRD),
        ("GOVERNANCE.md", DOC_GOVERNANCE),
    ];
    let dir = crate::ingest_queue::staging_dir();
    for (source, content) in docs {
        let id = format!("sistema-{source}");
        let staged = dir.join(&id);
        if std::fs::write(&staged, content.as_bytes()).is_err() {
            continue;
        }
        // Idempotencia: si el contenido coincide con lo ya ingerido (mismo SHA), no reencolar
        // —si no, la cola acumularía trabajos "done" en cada arranque—. Limpia el staging.
        let sha = crate::ingest_queue::sha256_file(&staged);
        if sha.is_some() && sha == crate::ingest_queue::cached_sha("sistema", source) {
            let _ = std::fs::remove_file(&staged);
            continue;
        }
        crate::ingest_queue::enqueue(&id, "sistema", source, &staged.to_string_lossy());
    }
}
