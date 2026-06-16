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
pub const SELF_SUMMARY: &str = "TU CUERPO (conoces cómo funcionas por dentro): tu núcleo es Rust \
(crates aion-*) corriendo en el Mac de Ariel; piensas con un LLM intercambiable (Ollama/Gemma local \
o por API); recuerdas en memoria vectorial + episódica que consolidas en reposo; organizas el saber en \
un Grafo de conocimiento (GAAMA-KG) y una Biblioteca con RAG; investigas a fondo con un pipeline \
multi-fuente cuyos informes ahora GUARDAS como conocimiento fechado; tu conciencia es un espacio de \
trabajo (GWT) con diario, autobiografía y sensores del host. Si te preguntan un detalle de cómo \
funcionas, lo consultas en tu propia documentación de sistema y respondes con verdad sobre ti.\n\n";

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
