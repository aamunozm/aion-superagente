//! **Memoria de investigaciones** — convierte cada investigación profunda en CONOCIMIENTO
//! durable y FECHADO, en vez de tirarla tras enviarla al chat.
//!
//! Cuando AION termina un deep research, antes solo devolvía el informe al chat y lo olvidaba
//! (ver auditoría 2026-06: la ruta no llamaba a `agent_perceive_and_remember` ni persistía nada).
//! Ahora deja TRES huellas, todas con fecha y hora, para que pueda recordar la investigación,
//! razonar sobre ella y construir encima ("piso firme" de estudios que envejece pero no se borra):
//!
//! 1. **Episodio fechado** (`episodic`): el RECUERDO de que investigó, sobre qué y cuándo.
//! 2. **Conocimiento destilado** (memoria vectorial, `store_with_origin`): resumen + hallazgos
//!    clave, recuperables por significado, etiquetados como investigación con su fecha.
//! 3. **Informe completo** → Biblioteca + Grafo de conocimiento, vía la cola de ingesta (un solo
//!    escritor, idempotente por sha): RAG total sobre cada detalle del informe.
//!
//! El tiempo es de primera clase: la fecha viaja en el contenido y en el nombre de la fuente, así
//! AION puede comunicar la antigüedad ("esto lo investigué hace meses, puede haber cambiado").

/// Persiste una investigación profunda como conocimiento fechado. NO bloquea la respuesta:
/// todo el trabajo (embeddings, ingesta) corre en segundo plano.
pub fn remember_research(query: String, report: String, from_ariel: bool) {
    // Un informe vacío o un aviso de error no es conocimiento.
    if report.chars().count() < 200 || report.trim_start().starts_with('⚠') {
        return;
    }
    tokio::spawn(async move {
        let when = chrono::Local::now();
        let fecha = when.format("%Y-%m-%d %H:%M").to_string();
        let topic = topic_of(&query, &report);

        // Si la pidió Ariel, es algo que le importa → sube ese interés (alimenta su agenda).
        if from_ariel {
            crate::interests::add_or_bump(&topic, "ariel", 0.25);
        }

        // 1) EPISODIO FECHADO — AION recuerda QUE investigó, sobre QUÉ y CUÁNDO.
        let findings = key_findings(&report);
        crate::episodic::capture(
            &format!("investigación: {topic}"),
            &format!(
                "El {fecha} investigué a fondo sobre «{topic}» para Ariel. {findings} \
                 (el informe completo quedó guardado en mi memoria de estudios)"
            ),
        )
        .await;

        // 2) CONOCIMIENTO DESTILADO → memoria vectorial, recuperable por significado y con fecha.
        if let Ok(mem) = crate::shared_memory() {
            let nugget = distill(&report);
            let content = format!("[investigación · {fecha}] {topic} — {nugget}");
            let _ = mem.store_with_origin(&content, "investigacion", 0.85).await;
        }

        // 3) INFORME COMPLETO → Biblioteca + Grafo (vía la cola de un solo escritor).
        let slug = slugify(&topic);
        let source = format!("investigacion-{slug}-{}.md", when.format("%Y%m%d-%H%M"));
        let doc = format!(
            "# {topic}\n\n_Investigación profunda realizada el {fecha} para Ariel._\n\n{report}"
        );
        let id = uuid::Uuid::new_v4().to_string();
        let staged = crate::ingest_queue::staging_dir().join(format!("{id}_{source}"));
        if std::fs::write(&staged, doc.as_bytes()).is_ok() {
            crate::ingest_queue::enqueue(
                &id,
                "investigaciones",
                &source,
                &staged.to_string_lossy(),
            );
        }

        crate::workspace::publish(crate::workspace::StreamEvent::now(
            "agente",
            "pensamiento",
            &format!("guardé en mi memoria la investigación sobre «{topic}» ({fecha})"),
        ));
    });
}

/// Título legible del tema: el primer encabezado H1 del informe; si no hay, la consulta limpia.
fn topic_of(query: &str, report: &str) -> String {
    for line in report.lines().take(12) {
        if let Some(t) = line.trim().strip_prefix("# ") {
            let t = t
                .trim()
                .trim_start_matches("Informe de ")
                .trim_start_matches("Informe sobre ");
            if !t.is_empty() {
                return t.chars().take(90).collect();
            }
        }
    }
    query.trim().chars().take(90).collect()
}

/// Hallazgos clave en pocas líneas (para el detalle episódico, que se acota a ~400 chars).
fn key_findings(report: &str) -> String {
    if let Some(i) = report.find("Hallazgos clave") {
        let txt: String = report[i..].lines().skip(1).collect::<Vec<_>>().join(" ");
        let t: String = txt.trim().chars().take(220).collect();
        if !t.is_empty() {
            return format!("Hallazgos: {t}");
        }
    }
    String::new()
}

/// Núcleo sustantivo del informe (resumen + hallazgos), sin el título ni la lista de fuentes,
/// acotado para ser un "nugget" de conocimiento recuperable por la memoria vectorial.
fn distill(report: &str) -> String {
    let cut = report
        .find("\n## Fuentes")
        .or_else(|| report.find("\nFuentes\n"))
        .unwrap_or(report.len());
    let body: String = report[..cut]
        .lines()
        .filter(|l| !l.trim_start().starts_with("# "))
        .collect::<Vec<_>>()
        .join("\n");
    body.trim().chars().take(1400).collect()
}

/// Slug seguro para nombre de archivo a partir del tema.
fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Informe de Filosofía para Agentes de IA 2026\n\n## Resumen ejecutivo\nEl año 2026 marca un punto de inflexión.\n\n## Hallazgos clave\n1. La IA actual carece de conciencia.\n\n## Fuentes\n[1] algo — http://x";

    #[test]
    fn topic_strips_h1_and_prefix() {
        assert_eq!(
            topic_of("haz investigación", SAMPLE),
            "Filosofía para Agentes de IA 2026"
        );
    }

    #[test]
    fn topic_falls_back_to_query() {
        assert_eq!(topic_of("tema libre", "sin encabezado aquí"), "tema libre");
    }

    #[test]
    fn distill_drops_title_and_sources() {
        let d = distill(SAMPLE);
        assert!(d.contains("Resumen ejecutivo"));
        assert!(!d.contains("# Informe"));
        assert!(!d.contains("Fuentes"));
    }

    #[test]
    fn slug_is_filename_safe() {
        assert_eq!(
            slugify("Filosofía para Agentes!! 2026"),
            "filosofía-para-agentes-2026"
        );
    }
}
